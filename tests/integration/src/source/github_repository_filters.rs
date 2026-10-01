use ps_core::{ingestion::Source, models::Platform};
use ps_workers::infra::registry::create_source;
use serde_json::Value;
use time::OffsetDateTime;
use wiremock::{
    Mock, Request, ResponseTemplate,
    matchers::{method, path},
};

use super::ongoing_tracking::{drive, person, timestamp};
use crate::common::wiremock_helpers::{
    SourceTestContext, graphql_pr_node, graphql_review_node, graphql_search_response,
};

fn pr(owner: &str, repo: &str, number: u32, archived: Option<bool>) -> Value {
    let event = timestamp(OffsetDateTime::now_utc() - time::Duration::hours(1));
    let mut node = graphql_pr_node(
        owner,
        repo,
        number,
        "manual",
        "Saved person's PR",
        "OPEN",
        &event,
        &event,
        1,
        0,
        &[graphql_review_node(
            "manual",
            "APPROVED",
            &event,
            100 + u64::from(number),
        )],
    );
    node["repository"]["isArchived"] = serde_json::to_value(archived).unwrap();
    node["reviews"]["totalCount"] = 2.into();
    node["reviews"]["pageInfo"] = serde_json::json!({"hasNextPage":true,"endCursor":"review-next"});
    node
}

fn review_page(number: u64) -> Value {
    serde_json::json!({
        "data":{"repository":{"pullRequest":{"reviews":{
            "totalCount":2,"pageInfo":{"hasNextPage":false,"endCursor":null},
            "nodes":[graphql_review_node("manual", "APPROVED", &timestamp(OffsetDateTime::now_utc() - time::Duration::hours(1)), 200+number)]
        }}}},"extensions":{"rateLimit":{"remaining":4900,"limit":5000,"resetAt":"2099-01-01T00:00:00Z"}}
    })
}

fn legacy_search_response(request: &Request, first: &[Value]) -> ResponseTemplate {
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    let variables = &body["variables"];
    let query = variables["query"].as_str().unwrap_or_default();
    // Supplementary discovery returns no data; all stored rows must originate
    // in ordinary legacy author search, whose filters used to be bypassed.
    let response = if query.contains("created:") {
        graphql_search_response(&[], false, None)
    } else if let Some(number) = variables["number"].as_u64() {
        review_page(number)
    } else if variables["cursor"] == "author-next" {
        graphql_search_response(
            &[pr("testorg", "allowed-next", 6, Some(false))],
            false,
            None,
        )
    } else {
        graphql_search_response(first, true, Some("author-next"))
    };
    ResponseTemplate::new(200).set_body_json(response)
}

async fn mount_files(ctx: &SourceTestContext, repo: &str, number: u32) {
    Mock::given(method("GET"))
        .and(path(format!("/repos/testorg/{repo}/pulls/{number}/files")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .expect(1)
        .mount(&ctx.mock_server)
        .await;
}

#[tokio::test]
async fn ordinary_saved_account_member_search_filters_repositories_before_diffs_and_review_pages() {
    let ctx = SourceTestContext::new().await;
    let ingestion = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            serde_json::json!({
                "base_url":ctx.mock_server.uri(), "orgs":["testorg"],
                "exclude_repos":["ExClUdEd", "TeStOrG/Qualified"]
            }),
            Some("fake-token".into()),
            None,
            None,
        )
        .await;
    let identity = person(&ctx, Platform::Github, "manual", None).await;
    assert!(
        ctx.repos
            .org
            .get_person(identity.person_id.into_inner())
            .await
            .unwrap()
            .unwrap()
            .team_id
            .is_none()
    );
    Mock::given(method("GET"))
        .and(path("/orgs/testorg/repos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&ctx.mock_server)
        .await;
    let first = vec![
        pr("testorg", "allowed", 1, Some(false)),
        pr("testorg", "excluded", 2, Some(false)),
        pr("testorg", "qualified", 3, Some(false)),
        pr("testorg", "archived", 4, Some(true)),
        pr("foreign", "outside", 5, Some(false)),
    ];
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(move |request: &Request| legacy_search_response(request, &first))
        .mount(&ctx.mock_server)
        .await;
    mount_files(&ctx, "allowed", 1).await;
    mount_files(&ctx, "allowed-next", 6).await;
    let source = create_source(&Platform::Github).unwrap();
    let plan = source.plan(&ingestion).await.unwrap();
    let final_cursor = drive(
        &ingestion,
        source.as_ref(),
        source.initial_cursor(&ingestion, &plan),
    )
    .await;
    assert!(
        ps_workers::features::ingestion::lib::finalise::extract_failed_items(&final_cursor)
            .is_empty()
    );
    let keys = [
        "testorg/allowed/pull/1",
        "testorg/allowed/review/101",
        "testorg/allowed/review/201",
        "testorg/allowed-next/pull/6",
        "testorg/allowed-next/review/106",
        "testorg/allowed-next/review/206",
        "testorg/excluded/pull/2",
        "testorg/excluded/review/102",
        "testorg/excluded/review/202",
        "testorg/qualified/pull/3",
        "testorg/qualified/review/103",
        "testorg/qualified/review/203",
        "testorg/archived/pull/4",
        "testorg/archived/review/104",
        "testorg/archived/review/204",
        "foreign/outside/pull/5",
        "foreign/outside/review/105",
        "foreign/outside/review/205",
    ]
    .map(str::to_owned);
    let stored = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids(&Platform::Github.to_string(), &keys)
        .await
        .unwrap();
    assert_eq!(
        stored.len(),
        6,
        "only allowed PRs and both review pages should be stored"
    );
    assert!(
        stored
            .iter()
            .all(|(_, key)| key.starts_with("testorg/allowed"))
    );
    let requests = ctx.mock_server.received_requests().await.unwrap();
    let reviews: Vec<_> = requests
        .iter()
        .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
        .filter(|body| body["variables"]["number"].is_number())
        .collect();
    assert_eq!(reviews.len(), 2);
    assert!(reviews.iter().all(|body| {
        body["variables"]["repo"]
            .as_str()
            .unwrap()
            .starts_with("allowed")
    }));
    assert!(
        requests
            .iter()
            .filter(|request| request.url.path().contains("/pulls/"))
            .all(|request| request.url.path().starts_with("/repos/testorg/allowed")),
        "excluded repositories must never fetch diffs"
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn member_search_unknown_archive_status_keeps_coverage_incomplete_unless_archive_filter_disabled()
 {
    for exclude_archived in [true, false] {
        let ctx = SourceTestContext::new().await;
        let ingestion = ctx.build_ingestion_ctx("github", Platform::Github, serde_json::json!({
            "base_url":ctx.mock_server.uri(), "orgs":["testorg"], "exclude_archived":exclude_archived
        }), Some("fake-token".into()), None, None).await;
        person(&ctx, Platform::Github, "manual", None).await;
        let mut node = pr("testorg", "unknown", 1, None);
        node["reviews"]["pageInfo"] = serde_json::json!({"hasNextPage":false});
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(graphql_search_response(
                    &[node],
                    false,
                    None,
                )),
            )
            .mount(&ctx.mock_server)
            .await;
        if !exclude_archived {
            mount_files(&ctx, "unknown", 1).await;
        }
        let source = ps_workers::features::ingestion::github::source::GitHubSource;
        let plan = ps_core::ingestion::IngestionPlan {
            source_name: "github".into(),
            repos: vec![],
            items: vec![],
            watermark: None,
            discovery_cutoff: None,
        };
        let first = source
            .fetch_batch(&ingestion, &source.initial_cursor(&ingestion, &plan))
            .await
            .unwrap();
        assert_eq!(first.items.is_empty(), exclude_archived);
        source.store_batch(&ingestion, &first.items).await.unwrap();
        let done = source
            .fetch_batch(&ingestion, first.next_cursor.as_deref().unwrap())
            .await
            .unwrap();
        assert!(done.next_cursor.is_none());
        let cursor: Value = serde_json::from_str(
            done.etag
                .as_deref()
                .unwrap_or(first.next_cursor.as_deref().unwrap()),
        )
        .unwrap();
        assert_eq!(
            cursor["failed_items"].as_array().unwrap().is_empty(),
            !exclude_archived
        );
        assert_eq!(
            cursor["completed_max_updated_at"].is_null(),
            exclude_archived
        );
        if exclude_archived {
            assert!(
                ctx.mock_server
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .all(|request| request.url.path() == "/graphql")
            );
        }
        ctx.teardown().await;
    }
}
