use crate::common::fixtures::create_person_with_identity;
use crate::common::wiremock_helpers::*;
use ps_core::ingestion::Source;
use ps_core::models::{ContributionType, Platform};
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

fn github_source() -> ps_workers::features::ingestion::github::source::GitHubSource {
    ps_workers::features::ingestion::github::source::GitHubSource
}

/// Build settings JSON pointing at the mock server with the given orgs.
fn github_settings(mock_uri: &str, orgs: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "base_url": mock_uri,
        "orgs": orgs,
    })
}

/// Build a minimal initial cursor for the TeamRepos phase.
fn team_repos_cursor(repos: &[(&str, &str)], watermark: Option<&str>) -> String {
    let repo_targets: Vec<serde_json::Value> = repos
        .iter()
        .map(|(owner, repo)| {
            serde_json::json!({
                "owner": owner,
                "repo": repo,
            })
        })
        .collect();
    serde_json::json!({
        "phase": "TeamRepos",
        "repo_index": 0,
        "graphql_cursor": null,
        "watermark": watermark,
        "repos": repo_targets,
        "max_updated_at": watermark,
        "orgs": [],
        "search_user_index": 0,
        "search_graphql_cursor": null,
        "search_users": [],
        "ingested_repos": [],
        "last_rate_limit_remaining": null,
        "failed_items": [],
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// fetch_batch tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_batch_parses_graphql_prs() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    let pr_node = graphql_pr_node(
        "testorg",
        "myrepo",
        42,
        "alice",
        "Add feature X",
        "OPEN",
        "2025-03-01T10:00:00Z",
        "2025-03-15T12:00:00Z",
        100,
        20,
        &[graphql_review_node(
            "bob",
            "APPROVED",
            "2025-03-14T10:00:00Z",
            9001,
        )],
    );
    let response = graphql_search_response(&[pr_node], false, None);

    // Mount GraphQL search mock
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .mount(&ctx.mock_server)
        .await;

    // Mount empty PR files response (diff fetch)
    Mock::given(method("GET"))
        .and(path("/repos/testorg/myrepo/pulls/42/files"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!([]))
                .append_header("x-ratelimit-remaining", "4999")
                .append_header("x-ratelimit-limit", "5000")
                .append_header("x-ratelimit-reset", "9999999999"),
        )
        .mount(&ctx.mock_server)
        .await;

    let source = github_source();
    let cursor = team_repos_cursor(&[("testorg", "myrepo")], Some("2025-03-01T00:00:00Z"));

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("fetch_batch");

    // Should have 1 PR + 1 review = 2 items
    assert_eq!(result.items.len(), 2);

    let pr = result
        .items
        .iter()
        .find(|i| i.contribution_type == ContributionType::PullRequest)
        .expect("PR item");
    assert_eq!(pr.platform_id.as_str(), "testorg/myrepo/pull/42");
    assert_eq!(pr.platform_username.as_str(), "alice");
    assert_eq!(pr.title.as_deref(), Some("Add feature X"));

    let review = result
        .items
        .iter()
        .find(|i| i.contribution_type == ContributionType::PrReview)
        .expect("review item");
    assert_eq!(review.platform_id.as_str(), "testorg/myrepo/review/9001");
    assert_eq!(review.platform_username.as_str(), "bob");

    ctx.teardown().await;
}

#[tokio::test]
async fn fetch_batch_handles_pagination() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    // First page: has_next_page = true
    let pr1 = graphql_pr_node(
        "testorg",
        "repo1",
        1,
        "alice",
        "PR 1",
        "OPEN",
        "2025-03-01T10:00:00Z",
        "2025-03-10T10:00:00Z",
        10,
        5,
        &[],
    );
    let page1 = graphql_search_response(&[pr1], true, Some("cursor-abc"));

    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&page1))
        .expect(1)
        .mount(&ctx.mock_server)
        .await;

    let source = github_source();
    let cursor = team_repos_cursor(&[("testorg", "repo1")], None);

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("fetch page 1");
    assert_eq!(result.items.len(), 1);
    assert!(
        result.next_cursor.is_some(),
        "should have next_cursor for pagination"
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn fetch_batch_advances_repo_index_after_last_page() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    // Single page, no more results
    let pr = graphql_pr_node(
        "testorg",
        "repo1",
        1,
        "alice",
        "PR 1",
        "MERGED",
        "2025-03-01T10:00:00Z",
        "2025-03-10T10:00:00Z",
        5,
        2,
        &[],
    );
    let response = graphql_search_response(&[pr], false, None);

    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .mount(&ctx.mock_server)
        .await;

    let source = github_source();
    // Two repos — after completing repo1, cursor should advance repo_index
    let cursor = team_repos_cursor(&[("testorg", "repo1"), ("testorg", "repo2")], None);

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("fetch_batch");
    assert!(result.next_cursor.is_some());

    // Parse the next cursor — repo_index should be 1
    let next_cur: serde_json::Value =
        serde_json::from_str(result.next_cursor.as_deref().unwrap()).unwrap();
    assert_eq!(next_cur["repo_index"], 1);

    ctx.teardown().await;
}

#[tokio::test]
async fn failed_repo_recorded_in_cursor() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    // Return 403 for GraphQL
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
        .mount(&ctx.mock_server)
        .await;

    let source = github_source();
    let cursor = team_repos_cursor(&[("testorg", "forbidden-repo"), ("testorg", "repo2")], None);

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("should not hard-fail");
    assert!(result.items.is_empty());

    // Failed items should be recorded, cursor advances past the failed repo
    let next_cur: serde_json::Value =
        serde_json::from_str(result.next_cursor.as_deref().unwrap()).unwrap();
    assert_eq!(next_cur["repo_index"], 1);
    assert_eq!(next_cur["failed_items"].as_array().unwrap().len(), 1);

    ctx.teardown().await;
}

#[tokio::test]
async fn two_phase_transitions_correctly() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    // Empty response for no more repos — triggers transition
    let empty = graphql_search_response(&[], false, None);

    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&empty))
        .mount(&ctx.mock_server)
        .await;

    let source = github_source();
    // Cursor with repo_index past all repos (empty repos list) triggers member search
    let cursor = team_repos_cursor(&[], None);

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("fetch_batch");
    // With no team members in DB, member search returns immediately with no cursor
    assert!(result.next_cursor.is_none());

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// store_batch tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn store_batch_upserts_contributions() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    // Create a person with github identity so store_batch can resolve
    create_person_with_identity(&ctx.pool, "Alice Dev", &Platform::Github, "alice").await;

    let items = vec![ps_core::ingestion::ContributionInput {
        platform: Platform::Github,
        contribution_type: ContributionType::PullRequest,
        platform_id: "testorg/myrepo/pull/42".into(),
        platform_username: "alice".into(),
        title: Some("Add feature X".into()),
        url: Some("https://github.com/testorg/myrepo/pull/42".into()),
        state: Some(ps_core::models::ContributionState::Open),
        created_at: time::OffsetDateTime::now_utc(),
        updated_at: None,
        closed_at: None,
        metrics: serde_json::json!({}),
        metadata: serde_json::json!({"repo": "testorg/myrepo"}),
        content: None,
        state_history: None,
        enrichment_content: None,
    }];

    let source = github_source();
    let stored = source
        .store_batch(&ing_ctx, &items)
        .await
        .expect("store_batch");
    assert_eq!(stored, 1);

    // Verify the row exists in DB
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM activity.contributions WHERE platform_id = 'testorg/myrepo/pull/42'",
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(count.0, 1);

    ctx.teardown().await;
}

#[tokio::test]
async fn store_batch_skips_unresolved_identities() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    // No person/identity in DB — store should skip
    let items = vec![ps_core::ingestion::ContributionInput {
        platform: Platform::Github,
        contribution_type: ContributionType::PullRequest,
        platform_id: "testorg/myrepo/pull/99".into(),
        platform_username: "unknown-user".into(),
        title: Some("Unknown PR".into()),
        url: None,
        state: None,
        created_at: time::OffsetDateTime::now_utc(),
        updated_at: None,
        closed_at: None,
        metrics: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: None,
        state_history: None,
        enrichment_content: None,
    }];

    let source = github_source();
    let stored = source
        .store_batch(&ing_ctx, &items)
        .await
        .expect("store_batch");
    assert_eq!(stored, 0, "unresolved identity should be skipped");

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// watermark tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn watermark_advances_after_store() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    let source = github_source();
    source
        .advance_watermark(&ing_ctx, "2025-03-15T12:00:00Z", 10)
        .await
        .expect("advance_watermark");

    let wm = ctx.repos.activity.get_watermark("github").await.unwrap();
    assert_eq!(wm.as_deref(), Some("2025-03-15T12:00:00Z"));

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// plan tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn plan_falls_back_to_org_discovery() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    // Mock the REST org repos endpoint for fallback discovery
    let repos_body = serde_json::json!([{
        "name": "repo1",
        "full_name": "testorg/repo1",
        "owner": { "login": "testorg" },
        "archived": false,
    }]);

    Mock::given(method("GET"))
        .and(path("/orgs/testorg/repos"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(&repos_body)
                .append_header("x-ratelimit-remaining", "4999")
                .append_header("x-ratelimit-limit", "5000")
                .append_header("x-ratelimit-reset", "9999999999"),
        )
        .mount(&ctx.mock_server)
        .await;

    let source = github_source();
    let plan = source.plan(&ing_ctx).await.expect("plan");

    assert_eq!(plan.source_name, "github");
    assert_eq!(plan.repos.len(), 1);
    assert_eq!(plan.repos[0].owner, "testorg");
    assert_eq!(plan.repos[0].repo, "repo1");
    // Watermark should be set (default lookback)
    assert!(plan.watermark.is_some());

    ctx.teardown().await;
}

#[tokio::test]
async fn plan_with_existing_watermark() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    // Set a watermark first
    ctx.repos
        .activity
        .upsert_watermark("github", "2025-03-01T00:00:00Z", 50)
        .await
        .unwrap();

    // Mock REST org repos
    Mock::given(method("GET"))
        .and(path("/orgs/testorg/repos"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!([{
                    "name": "repo1",
                    "full_name": "testorg/repo1",
                    "owner": { "login": "testorg" },
                    "archived": false,
                }]))
                .append_header("x-ratelimit-remaining", "4999")
                .append_header("x-ratelimit-limit", "5000")
                .append_header("x-ratelimit-reset", "9999999999"),
        )
        .mount(&ctx.mock_server)
        .await;

    let source = github_source();
    let plan = source.plan(&ing_ctx).await.expect("plan");
    assert_eq!(plan.watermark.as_deref(), Some("2025-03-01T00:00:00Z"));

    ctx.teardown().await;
}
