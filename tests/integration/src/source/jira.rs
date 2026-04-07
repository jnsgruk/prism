use crate::common::fixtures::create_person_with_identity;
use crate::common::wiremock_helpers::*;
use ps_core::ingestion::Source;
use ps_core::models::{ContributionType, Platform};
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, ResponseTemplate};

fn jira_source() -> ps_workers::features::ingestion::jira::source::JiraSource {
    ps_workers::features::ingestion::jira::source::JiraSource
}

fn jira_settings(mock_uri: &str, projects: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "base_url": mock_uri,
        "projects": projects,
        "api_mode": "cloud",
    })
}

/// Build a Jira cursor JSON string for fetch_batch.
fn jira_cursor(mock_uri: &str, projects: &[&str], watermark: Option<&str>) -> String {
    serde_json::json!({
        "watermark": watermark,
        "projects": projects,
        "project_index": 0,
        "next_page_token": null,
        "max_updated_at": watermark,
        "base_url": mock_uri,
        "story_points_field": null,
        "api_mode": "cloud",
        "failed_items": [],
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// fetch_batch tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_batch_parses_issues() {
    let ctx = SourceTestContext::new().await;

    let settings = jira_settings(&ctx.mock_server.uri(), &["PROJ"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "jira",
            Platform::Jira,
            settings,
            Some("test-token".into()),
            Some("test@example.com".into()),
            None,
        )
        .await;

    let issue = jira_issue_node(
        "PROJ-123",
        "Implement feature Y",
        "indeterminate",
        "user-account-1",
        "2025-03-01T10:00:00.000+00:00",
        "2025-03-15T12:00:00.000+00:00",
    );
    let response = jira_search_response(&[issue], true, None);

    Mock::given(method("GET"))
        .and(path_regex("/rest/api/3/search/jql.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .mount(&ctx.mock_server)
        .await;

    let source = jira_source();
    let cursor = jira_cursor(
        &ctx.mock_server.uri(),
        &["PROJ"],
        Some("2025-03-01T00:00:00+00:00"),
    );

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("fetch_batch");

    assert_eq!(result.items.len(), 1);
    let item = &result.items[0];
    assert_eq!(item.contribution_type, ContributionType::JiraTicket);
    assert_eq!(item.platform_id.as_str(), "PROJ-123");
    assert_eq!(item.title.as_deref(), Some("Implement feature Y"));
    assert_eq!(item.platform_username.as_str(), "user-account-1");

    ctx.teardown().await;
}

#[tokio::test]
async fn fetch_batch_pagination_token_advances() {
    let ctx = SourceTestContext::new().await;

    let settings = jira_settings(&ctx.mock_server.uri(), &["PROJ"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "jira",
            Platform::Jira,
            settings,
            Some("test-token".into()),
            Some("test@example.com".into()),
            None,
        )
        .await;

    let issue = jira_issue_node(
        "PROJ-1",
        "First issue",
        "new",
        "user-1",
        "2025-03-01T10:00:00.000+00:00",
        "2025-03-01T10:00:00.000+00:00",
    );
    // Not last page, has next token
    let response = jira_search_response(&[issue], false, Some("next-page-token-abc"));

    Mock::given(method("GET"))
        .and(path_regex("/rest/api/3/search/jql.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .mount(&ctx.mock_server)
        .await;

    let source = jira_source();
    let cursor = jira_cursor(&ctx.mock_server.uri(), &["PROJ"], None);

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("fetch_batch");
    assert!(
        result.next_cursor.is_some(),
        "should have next cursor for pagination"
    );

    let next_cur: serde_json::Value =
        serde_json::from_str(result.next_cursor.as_deref().unwrap()).unwrap();
    assert_eq!(next_cur["next_page_token"], "next-page-token-abc");

    ctx.teardown().await;
}

#[tokio::test]
async fn fetch_batch_advances_to_next_project() {
    let ctx = SourceTestContext::new().await;

    let settings = jira_settings(&ctx.mock_server.uri(), &["PROJ1", "PROJ2"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "jira",
            Platform::Jira,
            settings,
            Some("test-token".into()),
            Some("test@example.com".into()),
            None,
        )
        .await;

    let issue = jira_issue_node(
        "PROJ1-1",
        "Issue in PROJ1",
        "done",
        "user-1",
        "2025-03-01T10:00:00.000+00:00",
        "2025-03-10T10:00:00.000+00:00",
    );
    // Last page for PROJ1
    let response = jira_search_response(&[issue], true, None);

    Mock::given(method("GET"))
        .and(path_regex("/rest/api/3/search/jql.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .mount(&ctx.mock_server)
        .await;

    let source = jira_source();
    let cursor = jira_cursor(&ctx.mock_server.uri(), &["PROJ1", "PROJ2"], None);

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("fetch_batch");
    assert!(result.next_cursor.is_some());

    let next_cur: serde_json::Value =
        serde_json::from_str(result.next_cursor.as_deref().unwrap()).unwrap();
    assert_eq!(next_cur["project_index"], 1, "should advance to PROJ2");

    ctx.teardown().await;
}

#[tokio::test]
async fn all_projects_exhausted_returns_none() {
    let ctx = SourceTestContext::new().await;

    let settings = jira_settings(&ctx.mock_server.uri(), &["PROJ"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "jira",
            Platform::Jira,
            settings,
            Some("test-token".into()),
            Some("test@example.com".into()),
            None,
        )
        .await;

    let source = jira_source();
    // project_index past the end
    let cursor = serde_json::json!({
        "watermark": null,
        "projects": ["PROJ"],
        "project_index": 1,
        "next_page_token": null,
        "max_updated_at": null,
        "base_url": ctx.mock_server.uri(),
        "story_points_field": null,
        "api_mode": "cloud",
        "failed_items": [],
    })
    .to_string();

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("fetch_batch");
    assert!(result.next_cursor.is_none());
    assert!(result.items.is_empty());

    ctx.teardown().await;
}

#[tokio::test]
async fn failed_project_isolation() {
    let ctx = SourceTestContext::new().await;

    let settings = jira_settings(&ctx.mock_server.uri(), &["BAD", "GOOD"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "jira",
            Platform::Jira,
            settings,
            Some("test-token".into()),
            Some("test@example.com".into()),
            None,
        )
        .await;

    // Return 403 for the search (BAD project)
    Mock::given(method("GET"))
        .and(path_regex("/rest/api/3/search/jql.*"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
        .mount(&ctx.mock_server)
        .await;

    let source = jira_source();
    let cursor = jira_cursor(&ctx.mock_server.uri(), &["BAD", "GOOD"], None);

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("should not hard-fail");
    assert!(result.items.is_empty());

    // Should advance past BAD to GOOD
    let next_cur: serde_json::Value =
        serde_json::from_str(result.next_cursor.as_deref().unwrap()).unwrap();
    assert_eq!(next_cur["project_index"], 1);
    assert_eq!(next_cur["failed_items"].as_array().unwrap().len(), 1);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// store_batch tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn store_batch_upserts_jira_tickets() {
    let ctx = SourceTestContext::new().await;

    let settings = jira_settings(&ctx.mock_server.uri(), &["PROJ"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "jira",
            Platform::Jira,
            settings,
            Some("test-token".into()),
            Some("test@example.com".into()),
            None,
        )
        .await;

    // Create a person with Jira identity
    create_person_with_identity(&ctx.pool, "Jira User", &Platform::Jira, "user-account-1").await;

    let items = vec![ps_core::ingestion::ContributionInput {
        platform: Platform::Jira,
        contribution_type: ContributionType::JiraTicket,
        platform_id: "PROJ-123".into(),
        platform_username: "user-account-1".into(),
        title: Some("Test ticket".into()),
        url: Some(format!("{}/browse/PROJ-123", ctx.mock_server.uri())),
        state: Some(ps_core::models::ContributionState::InProgress),
        created_at: time::OffsetDateTime::now_utc(),
        updated_at: None,
        closed_at: None,
        metrics: serde_json::json!({"issue_type": "Story"}),
        metadata: serde_json::json!({}),
        content: None,
        state_history: None,
        enrichment_content: None,
    }];

    let source = jira_source();
    let stored = source
        .store_batch(&ing_ctx, &items)
        .await
        .expect("store_batch");
    assert_eq!(stored, 1);

    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM activity.contributions WHERE platform_id = 'PROJ-123'",
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(count.0, 1);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// watermark tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn watermark_advances_per_source() {
    let ctx = SourceTestContext::new().await;

    let settings = jira_settings(&ctx.mock_server.uri(), &["PROJ"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "jira",
            Platform::Jira,
            settings,
            Some("test-token".into()),
            Some("test@example.com".into()),
            None,
        )
        .await;

    let source = jira_source();
    source
        .advance_watermark(&ing_ctx, "2025-03-15T12:00:00+00:00", 5)
        .await
        .expect("advance_watermark");

    let wm = ctx.repos.activity.get_watermark("jira").await.unwrap();
    assert_eq!(wm.as_deref(), Some("2025-03-15T12:00:00+00:00"));

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// plan tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn plan_loads_watermark() {
    let ctx = SourceTestContext::new().await;

    let settings = jira_settings(&ctx.mock_server.uri(), &["PROJ"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "jira",
            Platform::Jira,
            settings,
            Some("test-token".into()),
            Some("test@example.com".into()),
            None,
        )
        .await;

    ctx.repos
        .activity
        .upsert_watermark("jira", "2025-03-01T00:00:00+00:00", 100)
        .await
        .unwrap();

    let source = jira_source();
    let plan = source.plan(&ing_ctx).await.expect("plan");
    assert_eq!(plan.watermark.as_deref(), Some("2025-03-01T00:00:00+00:00"));

    ctx.teardown().await;
}

#[tokio::test]
async fn plan_defaults_watermark_when_none() {
    let ctx = SourceTestContext::new().await;

    let settings = jira_settings(&ctx.mock_server.uri(), &["PROJ"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "jira",
            Platform::Jira,
            settings,
            Some("test-token".into()),
            Some("test@example.com".into()),
            None,
        )
        .await;

    let source = jira_source();
    let plan = source.plan(&ing_ctx).await.expect("plan");
    // Should have a default watermark (30 days ago)
    assert!(plan.watermark.is_some());

    ctx.teardown().await;
}
