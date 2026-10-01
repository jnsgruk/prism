use super::{github_settings, github_source};
use crate::common::wiremock_helpers::*;
use ps_core::ingestion::{IngestionContext, Source};
use ps_core::models::{ContributionType, Platform};
use time::OffsetDateTime;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

async fn context(ctx: &SourceTestContext) -> IngestionContext {
    let mut context = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            github_settings(&ctx.mock_server.uri(), &["testorg", "secondorg"]),
            Some("test-token".into()),
            None,
            None,
        )
        .await;
    ctx.with_person_scope(
        &mut context,
        "alice",
        None,
        "2025-03-01",
        time("2025-03-31T00:00:00Z"),
    )
    .await;
    context
}
async fn stored_count(ctx: &SourceTestContext, ing: &IngestionContext) -> i64 {
    let person_id = ing
        .request
        .as_ref()
        .unwrap()
        .scope
        .person_id()
        .unwrap()
        .into_inner();
    let (_, count) = ctx
        .repos
        .metrics
        .list_person_contributions(&ps_core::repo::metrics::ListPersonContributionsParams {
            person_id,
            platform: None,
            contribution_type: None,
            since: None,
            until: None,
            sort_field: None,
            sort_desc: false,
            page_size: 100,
            offset: 0,
            state: None,
            search: None,
        })
        .await
        .unwrap();
    count
}
fn time(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).unwrap()
}
fn pr(owner: &str, repo: &str, author: &str, reviews: &[serde_json::Value]) -> serde_json::Value {
    let mut pr = graphql_pr_node(
        owner,
        repo,
        42,
        author,
        "Parent PR",
        "OPEN",
        "2020-01-01T00:00:00Z",
        "2026-01-01T00:00:00Z",
        1,
        1,
        reviews,
    );
    pr["repository"]["isArchived"] = false.into();
    pr
}
async fn mount_search(ctx: &SourceTestContext, body: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&ctx.mock_server)
        .await;
}

#[tokio::test]
async fn direct_person_search_two_orgs_both_phases_filters_and_deduplicates() {
    let ctx = SourceTestContext::new().await;
    let mut ing = context(&ctx).await;
    ing.source_config.settings["exclude_repos"] = serde_json::json!(["excluded"]);
    let review = graphql_review_node("alice", "APPROVED", "2025-03-15T00:00:00Z", 9001);
    let authored = {
        let mut pr = pr("testorg", "allowed", "alice", std::slice::from_ref(&review));
        pr["createdAt"] = "2025-03-01T00:00:00Z".into();
        pr
    };
    let mut archived = pr(
        "testorg",
        "archived",
        "other",
        std::slice::from_ref(&review),
    );
    archived["repository"]["isArchived"] = true.into();
    let mut deleted = review.clone();
    deleted["author"] = serde_json::Value::Null;
    let mut pending = review.clone();
    pending["state"] = "PENDING".into();
    let nodes = [
        authored,
        pr(
            "testorg",
            "allowed-old",
            "other",
            &[
                review.clone(),
                deleted,
                pending,
                graphql_review_node("alice", "APPROVED", "2025-04-01T00:00:00Z", 9002),
                graphql_review_node("bob", "APPROVED", "2025-03-15T00:00:00Z", 9003),
            ],
        ),
        archived,
        pr(
            "testorg",
            "excluded",
            "other",
            std::slice::from_ref(&review),
        ),
        pr(
            "thirdorg",
            "outside",
            "other",
            std::slice::from_ref(&review),
        ),
    ];
    mount_search(&ctx, graphql_search_response(&nodes, false, None)).await;
    let source = github_source();
    let plan = source.plan(&ing).await.unwrap();
    assert!(plan.repos.is_empty());
    let mut cursor = source.initial_cursor(&ing, &plan);
    ing.source_config.settings["orgs"] = serde_json::json!(["thirdorg"]);
    let mut stored = 0;
    for _ in 0..5 {
        let result = source.fetch_batch(&ing, &cursor).await.unwrap();
        assert!(
            result
                .items
                .iter()
                .all(|item| item.platform_username.as_str() == "alice")
        );
        assert!(
            result
                .items
                .iter()
                .all(|item| !item.platform_id.contains("excluded")
                    && !item.platform_id.contains("archived")
                    && !item.platform_id.contains("thirdorg"))
        );
        stored += source.store_batch(&ing, &result.items).await.unwrap();
        let Some(next) = result.next_cursor else {
            break;
        };
        cursor = next;
    }
    assert!(stored >= 3);
    assert_eq!(
        stored_count(&ctx, &ing).await,
        3,
        "authored/reviewed duplicates persist once"
    );
    let requests = ctx.mock_server.received_requests().await.unwrap();
    let searches: Vec<_> = requests.iter().filter(|request| request.method == "POST").map(|request| serde_json::from_slice::<serde_json::Value>(&request.body).unwrap()["variables"]["query"].as_str().unwrap().to_string()).collect();
    assert_eq!(searches.len(), 4);
    for org in ["testorg", "secondorg"] {
        for qualifier in ["author:alice", "reviewed-by:alice"] {
            assert!(
                searches.iter().any(
                    |query| query.contains(&format!("org:{org} ")) && query.contains(qualifier)
                )
            );
        }
    }
    assert!(searches.iter().all(|query| !query.contains("updated:")));
    // Only author PR diffs may use REST; there is no org/team discovery.
    assert!(
        requests
            .iter()
            .all(|request| request.method == "POST" || request.url.path().ends_with("/files"))
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn selected_review_later_pages_survive_rate_limits_and_resume() {
    let ctx = SourceTestContext::new().await;
    let ing = context(&ctx).await;
    let mut parent = pr(
        "testorg",
        "allowed",
        "other",
        &[graphql_review_node(
            "bob",
            "APPROVED",
            "2025-03-15T00:00:00Z",
            1,
        )],
    );
    parent["reviews"]["pageInfo"] =
        serde_json::json!({"hasNextPage": true, "endCursor": "inline-end"});
    parent["reviews"]["totalCount"] = 205.into();
    mount_search(&ctx, graphql_search_response(&[parent], false, None)).await;
    let source = github_source();
    let plan = source.plan(&ing).await.unwrap();
    let initial = source.initial_cursor(&ing, &plan);
    let result = source.fetch_batch(&ing, &initial).await.unwrap();
    assert!(result.items.is_empty());
    let cursor = result.next_cursor.unwrap();
    ctx.mock_server.reset().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(
            ResponseTemplate::new(429)
                .append_header("x-ratelimit-remaining", "0")
                .append_header("x-ratelimit-reset", "9999999999"),
        )
        .mount(&ctx.mock_server)
        .await;
    let paused = source.fetch_batch(&ing, &cursor).await.unwrap();
    assert_eq!(paused.next_cursor.as_deref(), Some(cursor.as_str()));
    assert_eq!(paused.rate_limit.unwrap().remaining, 0);
    ctx.mock_server.reset().await;
    let reviews: Vec<_> = (10..110)
        .map(|id| {
            graphql_review_node(
                if id == 109 { "alice" } else { "bob" },
                "APPROVED",
                "2025-03-15T00:00:00Z",
                id,
            )
        })
        .collect();
    Mock::given(method("POST")).and(body_partial_json(serde_json::json!({"variables": {"cursor": "inline-end"}}))).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": {"repository": {"pullRequest": {"reviews": {"totalCount": 205, "pageInfo": {"hasNextPage": true, "endCursor": "page-two"}, "nodes": reviews}}}}}))).mount(&ctx.mock_server).await;
    let page = source.fetch_batch(&ing, &cursor).await.unwrap();
    assert_eq!(page.items.len(), 1);
    let review = &page.items[0];
    assert_eq!(review.contribution_type, ContributionType::PrReview);
    assert_eq!(review.platform_id.as_str(), "testorg/allowed/review/109");
    assert_eq!(review.metadata["pr_platform_id"], "testorg/allowed/pull/42");
    assert!(
        review
            .url
            .as_ref()
            .unwrap()
            .ends_with("#pullrequestreview-109")
    );
    source.store_batch(&ing, &page.items).await.unwrap();
    let next = page.next_cursor.unwrap();
    ctx.mock_server.reset().await;
    let repeated = graphql_review_node("alice", "APPROVED", "2025-03-15T00:00:00Z", 109);
    let upper = graphql_review_node("alice", "COMMENTED", "2025-03-31T00:00:00Z", 110);
    Mock::given(method("POST")).and(body_partial_json(serde_json::json!({"variables": {"cursor": "page-two"}}))).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": {"repository": {"pullRequest": {"reviews": {"totalCount": 205, "pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": [repeated, upper, null]}}}}}))).mount(&ctx.mock_server).await;
    let last = source.fetch_batch(&ing, &next).await.unwrap();
    assert_eq!(last.items.len(), 2);
    source.store_batch(&ing, &last.items).await.unwrap();
    assert_eq!(
        stored_count(&ctx, &ing).await,
        2,
        "repeated review pages deduplicate by stable review key"
    );
    let decoded: serde_json::Value = serde_json::from_str(&last.next_cursor.unwrap()).unwrap();
    assert_eq!(decoded["pending_reviews"], serde_json::json!([]));
    ctx.teardown().await;
}

#[tokio::test]
async fn saturated_search_splits_and_minimum_partition_records_incomplete() {
    let ctx = SourceTestContext::new().await;
    let ing = context(&ctx).await;
    let mut response = graphql_search_response(&[], false, None);
    response["data"]["search"]["issueCount"] = 1200.into();
    mount_search(&ctx, response).await;
    let source = github_source();
    let plan = source.plan(&ing).await.unwrap();
    let initial = source.initial_cursor(&ing, &plan);
    let split = source.fetch_batch(&ing, &initial).await.unwrap();
    let mut decoded: serde_json::Value = serde_json::from_str(&split.next_cursor.unwrap()).unwrap();
    assert_eq!(decoded["person"]["partitions"].as_array().unwrap().len(), 2);
    assert!(decoded["failed_items"].as_array().unwrap().is_empty());
    decoded["person"]["partitions"] = serde_json::json!([{"start": 0, "end": 0}]);
    let minimum = source
        .fetch_batch(&ing, &decoded.to_string())
        .await
        .unwrap();
    let decoded: serde_json::Value = serde_json::from_str(&minimum.next_cursor.unwrap()).unwrap();
    assert_eq!(decoded["failed_items"].as_array().unwrap().len(), 1);
    assert!(
        decoded["failed_items"][0]["error"]
            .as_str()
            .unwrap()
            .contains("incomplete")
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn malformed_nodes_and_permanent_org_errors_are_explicit() {
    let ctx = SourceTestContext::new().await;
    let ing = context(&ctx).await;
    let source = github_source();
    let plan = source.plan(&ing).await.unwrap();
    let cursor = source.initial_cursor(&ing, &plan);
    mount_search(
        &ctx,
        graphql_search_response(
            &[
                serde_json::Value::Null,
                serde_json::json!({"number": "bad"}),
                serde_json::json!({}),
            ],
            false,
            None,
        ),
    )
    .await;
    let malformed = source.fetch_batch(&ing, &cursor).await.unwrap();
    let decoded: serde_json::Value = serde_json::from_str(&malformed.next_cursor.unwrap()).unwrap();
    assert_eq!(decoded["failed_items"].as_array().unwrap().len(), 3);
    ctx.mock_server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"errors": [{"message": "Could not resolve organisation"}]}),
        ))
        .mount(&ctx.mock_server)
        .await;
    let error = source.fetch_batch(&ing, &cursor).await.unwrap();
    let decoded: serde_json::Value = serde_json::from_str(&error.next_cursor.unwrap()).unwrap();
    assert_eq!(decoded["failed_items"].as_array().unwrap().len(), 1);
    ctx.teardown().await;
}

#[tokio::test]
async fn empty_search_and_retrying_server_failure_terminate_without_discovery() {
    let ctx = SourceTestContext::new().await;
    let ing = context(&ctx).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(graphql_search_response(&[], false, None)),
        )
        .with_priority(10)
        .mount(&ctx.mock_server)
        .await;
    let source = github_source();
    let plan = source.plan(&ing).await.unwrap();
    let mut cursor = source.initial_cursor(&ing, &plan);
    let mut complete = false;
    for _ in 0..5 {
        let result = source.fetch_batch(&ing, &cursor).await.unwrap();
        assert!(result.items.is_empty());
        match result.next_cursor {
            Some(next) => cursor = next,
            None => {
                complete = true;
                break;
            }
        }
    }
    assert!(complete);
    assert!(
        ctx.mock_server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method == "POST")
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn all_scope_review_continuations_emit_every_submitted_reviewer() {
    let ctx = SourceTestContext::new().await;
    let ing = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            github_settings(&ctx.mock_server.uri(), &["testorg"]),
            Some("token".into()),
            None,
            None,
        )
        .await;
    let mut parent = pr("testorg", "allowed", "other", &[]);
    parent["reviews"]["totalCount"] = 101.into();
    parent["reviews"]["pageInfo"] =
        serde_json::json!({"hasNextPage":true, "endCursor":"first-end"});
    mount_search(&ctx, graphql_search_response(&[parent], false, None)).await;
    let source = github_source();
    let first = source
        .fetch_batch(
            &ing,
            &super::team_repos_cursor(&[("testorg", "allowed")], None),
        )
        .await
        .unwrap();
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].metrics["review_count"], 101);
    ctx.mock_server.reset().await;
    let reviews: Vec<_> = (1..102)
        .map(|id| {
            graphql_review_node(
                if id % 2 == 0 { "alice" } else { "bob" },
                "APPROVED",
                "2025-03-15T00:00:00Z",
                id,
            )
        })
        .collect();
    let response = serde_json::json!({"data":{"repository":{"pullRequest":{"reviews":{"totalCount":101,"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":reviews}}}}});
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .with_priority(10)
        .mount(&ctx.mock_server)
        .await;
    let continuation = source
        .fetch_batch(&ing, &first.next_cursor.unwrap())
        .await
        .unwrap();
    assert_eq!(continuation.items.len(), 101);
    assert!(
        continuation
            .items
            .iter()
            .any(|item| item.platform_username.as_str() == "alice")
    );
    assert!(
        continuation
            .items
            .iter()
            .any(|item| item.platform_username.as_str() == "bob")
    );
    assert!(
        continuation.items[0].enrichment_content.as_ref().unwrap()["inline_comments_truncated"]
            .as_bool()
            .unwrap()
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn person_search_429_preserves_frozen_target_and_empty_orgs_fail() {
    let ctx = SourceTestContext::new().await;
    let mut ing = context(&ctx).await;
    let source = github_source();
    let plan = source.plan(&ing).await.unwrap();
    let cursor = source.initial_cursor(&ing, &plan);
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .append_header("x-ratelimit-remaining", "0")
                .append_header("x-ratelimit-reset", "9999999999"),
        )
        .mount(&ctx.mock_server)
        .await;
    let paused = source.fetch_batch(&ing, &cursor).await.unwrap();
    assert_eq!(paused.next_cursor.as_deref(), Some(cursor.as_str()));
    assert_eq!(paused.rate_limit.unwrap().remaining, 0);
    ing.source_config.settings["orgs"] = serde_json::json!([]);
    assert!(source.plan(&ing).await.is_err());
    ing.source_config.settings["orgs"] = serde_json::json!(["org updated:>2000"]);
    assert!(source.plan(&ing).await.is_err());
    ctx.teardown().await;
}

#[path = "github_errors.rs"]
mod errors;

#[path = "github_cursor_binding.rs"]
mod cursor_binding;
