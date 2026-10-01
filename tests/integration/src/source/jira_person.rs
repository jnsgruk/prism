//! Person-mode Jira fixtures use real PostgreSQL and the Cloud wire protocol.

use ps_core::ingestion::{IngestionContext, Source};
use ps_core::models::ContributionState;
use time::OffsetDateTime;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::{jira_settings, jira_source};
use crate::common::wiremock_helpers::{SourceTestContext, jira_issue_node, jira_search_response};

const ACCOUNT: &str = "557058:opaque-account";
const SINCE: &str = "2025-03-01";
const UPPER: &str = "2025-03-15T12:00:00Z";

async fn scoped_context(ctx: &SourceTestContext, projects: &[&str]) -> IngestionContext {
    let mut ingestion = ctx
        .build_ingestion_ctx(
            "jira",
            ps_core::models::Platform::Jira,
            jira_settings(&ctx.mock_server.uri(), projects),
            Some("test-token".into()),
            Some("test@example.com".into()),
            None,
        )
        .await;
    ctx.with_person_scope(
        &mut ingestion,
        "canonical@example.com",
        Some(ACCOUNT),
        SINCE,
        OffsetDateTime::parse(UPPER, &time::format_description::well_known::Rfc3339).unwrap(),
    )
    .await;
    ingestion
}

async fn timezone(ctx: &SourceTestContext) {
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"timeZone":"America/Los_Angeles"})),
        )
        .mount(&ctx.mock_server)
        .await;
}

async fn cursor(ingestion: &IngestionContext) -> String {
    let source = jira_source();
    let plan = source.plan(ingestion).await.unwrap();
    source.initial_cursor(ingestion, &plan)
}

#[tokio::test]
async fn frozen_assignee_project_and_exact_updated_window_preserve_ticket_details() {
    let ctx = SourceTestContext::new().await;
    let ingestion = scoped_context(&ctx, &["PROJ", "EMPTY"]).await;
    // A global incremental watermark must not replace the requested backfill.
    ctx.repos
        .activity
        .upsert_watermark("jira", "2026-01-01T00:00:00Z", 42)
        .await
        .unwrap();
    timezone(&ctx).await;
    let mut eligible = jira_issue_node(
        "PROJ-1",
        "Ancient ticket recently updated",
        "done",
        ACCOUNT,
        "2020-01-01T00:00:00Z",
        UPPER,
    );
    eligible["fields"]["customfield_10016"] = serde_json::json!(5);
    eligible["changelog"] = serde_json::json!({"histories":[
        {"created":"2025-03-01T00:00:00Z","items":[{"field":"status","toString":"In Progress"}]},
        {"created":"2025-03-02T00:00:00Z","items":[{"field":"status","toString":"Done"}]}
    ]});
    let mut reporter = jira_issue_node(
        "PROJ-2",
        "Reporter only",
        "new",
        "someone-else",
        SINCE,
        UPPER,
    );
    reporter["fields"]["reporter"] = serde_json::json!({"accountId":ACCOUNT});
    let mut former = jira_issue_node(
        "PROJ-3",
        "Formerly assigned",
        "new",
        "someone-else",
        SINCE,
        UPPER,
    );
    former["changelog"] = serde_json::json!({"histories":[{"items":[{"field":"assignee","from":ACCOUNT,"to":"someone-else"}]}]});
    let mut unassigned = eligible.clone();
    unassigned["key"] = "PROJ-4".into();
    unassigned["fields"]["assignee"] = serde_json::Value::Null;
    let before = jira_issue_node(
        "PROJ-5",
        "Before lower",
        "new",
        ACCOUNT,
        "2020-01-01T00:00:00Z",
        "2025-02-28T23:59:59Z",
    );
    let after = jira_issue_node(
        "PROJ-6",
        "After upper",
        "new",
        ACCOUNT,
        "2020-01-01T00:00:00Z",
        "2025-03-15T12:00:00.001Z",
    );
    let other_project = jira_issue_node(
        "OUTSIDE-1",
        "Outside source projects",
        "new",
        ACCOUNT,
        "2020-01-01T00:00:00Z",
        UPPER,
    );
    let jql = format!(
        "project = \"PROJ\" AND assignee = \"{ACCOUNT}\" AND updated >= \"2025-02-27 16:00\" ORDER BY updated ASC, key ASC"
    );
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("jql", jql))
        .and(query_param("fields", "summary,description,status,issuetype,priority,labels,assignee,reporter,created,updated,resolutiondate,parent,customfield_10016"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jira_search_response(
                &[
                    eligible,
                    reporter,
                    former,
                    unassigned,
                    before,
                    after,
                    other_project,
                ],
                true,
                None,
            )),
        )
        .expect(1)
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("GET")).and(path("/rest/api/3/search/jql"))
        .and(query_param("jql", format!("project = \"EMPTY\" AND assignee = \"{ACCOUNT}\" AND updated >= \"2025-02-27 16:00\" ORDER BY updated ASC, key ASC")))
        .respond_with(ResponseTemplate::new(200).set_body_json(jira_search_response(&[], true, None)))
        .expect(1).mount(&ctx.mock_server).await;
    let source = jira_source();
    let mut initial: serde_json::Value = serde_json::from_str(&cursor(&ingestion).await).unwrap();
    initial["story_points_field"] = "customfield_10016".into();
    let result = source
        .fetch_batch(&ingestion, &initial.to_string())
        .await
        .unwrap();
    assert_eq!(result.items.len(), 1);
    let ticket = &result.items[0];
    assert_eq!(ticket.state, Some(ContributionState::Closed));
    assert_eq!(
        ticket.url.as_deref(),
        Some(format!("{}/browse/PROJ-1", ctx.mock_server.uri()).as_str())
    );
    assert_eq!(ticket.metrics["story_points"], 5.0);
    assert_eq!(ticket.metrics["cycle_time_hours"], 24.0);
    assert_eq!(
        ticket
            .state_history
            .as_ref()
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        source.store_batch(&ingestion, &result.items).await.unwrap(),
        1
    );
    assert_eq!(
        source.store_batch(&ingestion, &result.items).await.unwrap(),
        1
    );
    source
        .advance_watermark(&ingestion, UPPER, 1)
        .await
        .unwrap();
    assert_eq!(
        ctx.repos
            .activity
            .get_watermark("jira")
            .await
            .unwrap()
            .as_deref(),
        Some("2026-01-01T00:00:00Z")
    );
    let second = source
        .fetch_batch(&ingestion, result.next_cursor.as_deref().unwrap())
        .await
        .unwrap();
    assert!(second.items.is_empty());
    assert!(second.next_cursor.is_none());
    let requests = ctx.mock_server.received_requests().await.unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.url.path() == "/rest/api/3/myself")
            .count(),
        1,
        "timezone is frozen across pages/projects"
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn punctuation_is_encoded_and_empty_projects_remain_assignee_scoped() {
    let ctx = SourceTestContext::new().await;
    let mut ingestion = scoped_context(&ctx, &[]).await;
    let account = r#"557:id\with"quote"#;
    ingestion
        .request
        .as_mut()
        .unwrap()
        .source
        .identity
        .as_mut()
        .unwrap()
        .platform_user_id = Some(account.into());
    timezone(&ctx).await;
    Mock::given(method("GET")).and(path("/rest/api/3/search/jql"))
        .and(query_param("jql", r#"assignee = "557:id\\with\"quote" AND updated >= "2025-02-27 16:00" ORDER BY updated ASC, key ASC"#))
        .respond_with(ResponseTemplate::new(200).set_body_json(jira_search_response(&[], true, None)))
        .expect(1).mount(&ctx.mock_server).await;
    let result = jira_source()
        .fetch_batch(&ingestion, &cursor(&ingestion).await)
        .await
        .unwrap();
    assert!(result.next_cursor.is_none());
    ctx.teardown().await;
}

#[tokio::test]
async fn pagination_resume_rechecks_reassignment_and_freezes_scope() {
    let ctx = SourceTestContext::new().await;
    let ingestion = scoped_context(&ctx, &["PROJ"]).await;
    timezone(&ctx).await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jira_search_response(
                &[],
                false,
                Some("page-2"),
            )),
        )
        .expect(1)
        .with_priority(2)
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("nextPageToken", "page-2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jira_search_response(
                &[jira_issue_node(
                    "PROJ-1",
                    "Reassigned during paging",
                    "new",
                    "other",
                    "2020-01-01T00:00:00Z",
                    UPPER,
                )],
                true,
                None,
            )),
        )
        .expect(1)
        .with_priority(1)
        .mount(&ctx.mock_server)
        .await;
    let source = jira_source();
    let page = source
        .fetch_batch(&ingestion, &cursor(&ingestion).await)
        .await
        .unwrap();
    let resumed = source
        .fetch_batch(&ingestion, page.next_cursor.as_deref().unwrap())
        .await
        .unwrap();
    assert!(resumed.items.is_empty());
    assert!(resumed.next_cursor.is_none());
    let mut changed = ingestion.clone();
    changed.request.as_mut().unwrap().run_started_at += time::Duration::seconds(1);
    assert!(
        source
            .fetch_batch(&changed, page.next_cursor.as_deref().unwrap())
            .await
            .is_err()
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn missing_empty_repeated_and_cyclic_tokens_fail_explicitly() {
    let ctx = SourceTestContext::new().await;
    let ingestion = scoped_context(&ctx, &["PROJ"]).await;
    timezone(&ctx).await;
    let initial = cursor(&ingestion).await;
    for token in [None, Some(""), Some(" "), Some("repeat"), Some("prior")] {
        let mock = Mock::given(method("GET"))
            .and(path("/rest/api/3/search/jql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(jira_search_response(&[], false, token)),
            )
            .expect(1)
            .mount_as_scoped(&ctx.mock_server)
            .await;
        let mut state: serde_json::Value = serde_json::from_str(&initial).unwrap();
        state["next_page_token"] = "repeat".into();
        state["token_cycle"] = serde_json::json!({"anchor":"prior","power":2,"distance":1});
        assert!(
            jira_source()
                .fetch_batch(&ingestion, &state.to_string())
                .await
                .is_err()
        );
        drop(mock);
    }
    ctx.teardown().await;
}

#[tokio::test]
async fn changed_endpoint_unsupported_server_and_malformed_projects_fail_before_http() {
    let ctx = SourceTestContext::new().await;
    let mut ingestion = scoped_context(&ctx, &["PROJ"]).await;
    let initial = cursor(&ingestion).await;
    let alternate = wiremock::MockServer::start().await;
    ingestion.source_config.settings["base_url"] = alternate.uri().into();
    assert!(
        jira_source()
            .fetch_batch(&ingestion, &initial)
            .await
            .unwrap_err()
            .to_string()
            .contains("endpoint changed")
    );
    assert!(alternate.received_requests().await.unwrap().is_empty());
    ingestion.source_config.settings["base_url"] = ctx.mock_server.uri().into();
    ingestion.source_config.settings["api_mode"] = "server".into();
    assert!(
        jira_source()
            .plan(&ingestion)
            .await
            .unwrap_err()
            .to_string()
            .contains("Server/Data Center")
    );
    ingestion.source_config.settings["api_mode"] = "cloud".into();
    ingestion.source_config.settings["projects"] = serde_json::json!(["PROJ", 1]);
    assert!(jira_source().plan(&ingestion).await.is_err());
    ingestion.source_config.settings["projects"] = serde_json::json!(["PROJ"]);
    for account in [None, Some("bad\nOR assignee != selected")] {
        ingestion
            .request
            .as_mut()
            .unwrap()
            .source
            .identity
            .as_mut()
            .unwrap()
            .platform_user_id = account.map(str::to_owned);
        assert!(jira_source().plan(&ingestion).await.is_err());
    }
    assert!(
        ctx.mock_server
            .received_requests()
            .await
            .unwrap()
            .is_empty()
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn rate_limit_keeps_same_project_and_cursor_for_resume() {
    let ctx = SourceTestContext::new().await;
    let ingestion = scoped_context(&ctx, &["PROJ", "NEXT"]).await;
    timezone(&ctx).await;
    let initial = cursor(&ingestion).await;
    let rate_limit = Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "19"))
        .expect(1)
        .mount_as_scoped(&ctx.mock_server)
        .await;
    assert!(matches!(
        jira_source().fetch_batch(&ingestion, &initial).await,
        Err(ps_core::Error::RateLimit {
            retry_after_secs: 19
        })
    ));
    drop(rate_limit);
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jira_search_response(&[], true, None)),
        )
        .expect(1)
        .mount(&ctx.mock_server)
        .await;
    let result = jira_source()
        .fetch_batch(&ingestion, &initial)
        .await
        .unwrap();
    let resumed: serde_json::Value =
        serde_json::from_str(result.next_cursor.as_deref().unwrap()).unwrap();
    assert_eq!(resumed["project_index"], 1);
    assert!(resumed["failed_items"].as_array().unwrap().is_empty());
    ctx.teardown().await;
}

#[tokio::test]
async fn exhausted_server_errors_preserve_partial_project_failure() {
    let ctx = SourceTestContext::new().await;
    let ingestion = scoped_context(&ctx, &["BAD", "GOOD"]).await;
    timezone(&ctx).await;
    let failing = Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(ResponseTemplate::new(503).set_body_string("private provider response body"))
        .expect(6)
        .mount_as_scoped(&ctx.mock_server)
        .await;
    let initial = cursor(&ingestion).await;
    let source = jira_source();
    tokio::time::pause();
    let fetch = source.fetch_batch(&ingestion, &initial);
    tokio::pin!(fetch);
    let result = loop {
        tokio::select! {
            result = &mut fetch => break result.unwrap(),
            () = tokio::task::yield_now() => {
                // Keep the runtime awake so real socket I/O gets polled before
                // advancing each virtual second of exponential backoff.
                for _ in 0..1000 { tokio::task::yield_now().await; }
                tokio::time::advance(std::time::Duration::from_secs(1)).await;
            }
        }
    };
    tokio::time::resume();
    drop(failing);
    let next: serde_json::Value =
        serde_json::from_str(result.next_cursor.as_deref().unwrap()).unwrap();
    assert_eq!(next["project_index"], 1);
    assert_eq!(next["failed_items"][0]["key"], "BAD");
    assert_eq!(
        next["failed_items"][0]["error"],
        "Jira project fetch failed with HTTP 503"
    );
    assert!(!next.to_string().contains("private provider response body"));
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jira_search_response(&[], true, None)),
        )
        .expect(1)
        .mount(&ctx.mock_server)
        .await;
    let completed = jira_source()
        .fetch_batch(&ingestion, &next.to_string())
        .await
        .unwrap();
    assert!(completed.next_cursor.is_none());
    let final_state: serde_json::Value =
        serde_json::from_str(completed.etag.as_deref().unwrap()).unwrap();
    assert_eq!(final_state["failed_items"][0]["key"], "BAD");
    assert!(
        !final_state
            .to_string()
            .contains("private provider response body")
    );
    ctx.teardown().await;
}
