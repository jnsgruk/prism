//! Exercise the real worker service and durable Restate journal, rather than
//! invoking adapters directly. Provider responses are local Wiremock fixtures.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use ps_core::ingestion::{IngestionContext, Source};
use ps_core::models::{Platform, SecretKey};
use ps_workers::features::ingestion::jira::source::JiraSource;
use ps_workers::features::ingestion::lib::chunk::ChunkRequest;
use time::OffsetDateTime;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, Request, ResponseTemplate};

use crate::common::restate::{RestateTestContext, TEST_SECRET_KEY};
use crate::common::wiremock_helpers::{SourceTestContext, jira_issue_node, jira_search_response};

pub(super) const ACCOUNT: &str = "chunk-selected-account";
const PRIVATE_TOKEN: &str = "fake-private-token-never-in-journal";
pub(super) const UPPER: &str = "2025-03-15T12:00:00Z";

pub(super) async fn scoped_jira(ctx: &SourceTestContext) -> IngestionContext {
    let mut ingestion = ctx.build_ingestion_ctx("selected Jira", Platform::Jira,
        serde_json::json!({"base_url":ctx.mock_server.uri(),"api_mode":"cloud","projects":["PROJ"]}),
        Some(PRIVATE_TOKEN.into()), Some("test@example.com".into()), None).await;
    ctx.with_person_scope(
        &mut ingestion,
        "selected@example.com",
        Some(ACCOUNT),
        "2025-03-01",
        OffsetDateTime::parse(UPPER, &time::format_description::well_known::Rfc3339).unwrap(),
    )
    .await;
    for (key, value) in [
        (SecretKey::ApiToken, PRIVATE_TOKEN),
        (SecretKey::Email, "test@example.com"),
    ] {
        ctx.repos
            .config
            .upsert_secret(
                Uuid::now_v7(),
                ingestion.source_config.id.into_inner(),
                key.as_str(),
                &ps_core::crypto::encrypt(&TEST_SECRET_KEY, value.as_bytes()).unwrap(),
            )
            .await
            .unwrap();
    }
    ctx.repos
        .activity
        .upsert_watermark("selected Jira", "2026-01-01T00:00:00Z", 73)
        .await
        .unwrap();
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"timeZone":"UTC"})),
        )
        .mount(&ctx.mock_server)
        .await;
    ingestion
}

async fn chunk_request(ingestion: &IngestionContext, max_batches: usize) -> ChunkRequest {
    let plan = JiraSource.plan(ingestion).await.unwrap();
    ChunkRequest {
        source_type: Platform::Jira,
        cursor: JiraSource.initial_cursor(ingestion, &plan),
        run_id: ingestion.run_id.unwrap(),
        max_batches,
        items_offset: 0,
        request: ingestion.request.clone(),
    }
}

pub(super) fn issue(key: &str, account: &str) -> serde_json::Value {
    jira_issue_node(
        key,
        "Scoped worker fixture",
        "done",
        account,
        "2020-01-01T00:00:00Z",
        UPPER,
    )
}

#[tokio::test]
async fn empty_pages_are_bounded_and_resume_exact_person_source_without_global_watermark() {
    let ctx = SourceTestContext::new().await;
    let ingestion = scoped_jira(&ctx).await;
    let unrelated = wiremock::MockServer::start().await;
    ctx.repos.config.create_source(Uuid::now_v7(), &Platform::Jira.to_string(), "unrelated Jira",
        &serde_json::json!({"base_url":unrelated.uri(),"api_mode":"cloud","projects":["OTHER"]}), None).await.unwrap();
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(|request: &Request| {
            let page = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "nextPageToken")
                .map_or(0, |(_, value)| value.parse::<usize>().unwrap());
            let body = if page == 4 {
                jira_search_response(
                    &[issue("PROJ-1", ACCOUNT), issue("PROJ-2", "outsider")],
                    true,
                    None,
                )
            } else {
                jira_search_response(&[], false, Some(&(page + 1).to_string()))
            };
            ResponseTemplate::new(200).set_body_json(body)
        })
        .expect(5)
        .mount(&ctx.mock_server)
        .await;
    let runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let mut request = chunk_request(&ingestion, 2).await;
    for expected_page in ["2", "4"] {
        let invocation = runtime.send_chunk(&request).await;
        let result = runtime.result(&invocation).await;
        assert!(!result.is_complete);
        assert_eq!(result.items_stored, 0);
        let saved: serde_json::Value = serde_json::from_str(&result.cursor).unwrap();
        assert_eq!(saved["next_page_token"], expected_page);
        assert_eq!(
            saved["request"],
            serde_json::to_value(ingestion.request.as_ref().unwrap()).unwrap()
        );
        assert!(
            ctx.repos
                .activity
                .list_pipeline_invocation_ids(ingestion.request.as_ref().unwrap().pipeline_id)
                .await
                .unwrap()
                .contains(&invocation)
        );
        request.cursor = result.cursor;
    }
    let invocation = runtime.send_chunk(&request).await;
    let final_chunk = runtime.result(&invocation).await;
    assert!(final_chunk.is_complete);
    assert_eq!(final_chunk.items_stored, 1);
    let saved = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids(
            &Platform::Jira.to_string(),
            &["PROJ-1".into(), "PROJ-2".into()],
        )
        .await
        .unwrap();
    assert_eq!(saved.len(), 1, "outsider response must not be stored");
    assert!(saved.iter().any(|(_, key)| key == "PROJ-1"));
    assert_eq!(
        ctx.repos
            .activity
            .get_watermark("selected Jira")
            .await
            .unwrap()
            .as_deref(),
        Some("2026-01-01T00:00:00Z")
    );
    assert!(unrelated.received_requests().await.unwrap().is_empty());
    drop(runtime);
    ctx.teardown().await;
}

#[tokio::test]
async fn rate_limit_sleeps_durably_and_worker_restart_preserves_frozen_cursor_and_journal() {
    let ctx = SourceTestContext::new().await;
    let ingestion = scoped_jira(&ctx).await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jira_search_response(
                &[issue("PROJ-1", ACCOUNT)],
                false,
                Some("page-two"),
            )),
        )
        .expect(1..)
        .with_priority(2)
        .mount(&ctx.mock_server)
        .await;
    let ready = Arc::new(AtomicBool::new(false));
    let provider_ready = ready.clone();
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("nextPageToken", "page-two"))
        .respond_with(move |_: &Request| {
            if provider_ready.load(Ordering::SeqCst) {
                ResponseTemplate::new(200).set_body_json(jira_search_response(
                    &[issue("PROJ-2", ACCOUNT)],
                    true,
                    None,
                ))
            } else {
                ResponseTemplate::new(429).insert_header("retry-after", "6")
            }
        })
        .expect(2..)
        .with_priority(1)
        .mount(&ctx.mock_server)
        .await;
    let mut runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let request = chunk_request(&ingestion, 2).await;
    let invocation = runtime.send_chunk(&request).await;
    runtime.wait_status(&invocation, "suspended").await;
    let journal_query = format!(
        "SELECT index, entry_type, name, entry_lite_json FROM sys_journal WHERE id = '{invocation}' ORDER BY index"
    );
    let before = runtime.query(&journal_query).await;
    assert!(
        before.iter().any(
            |row| row.get("entry_type").and_then(serde_json::Value::as_str)
                == Some("Command: Sleep")
        ),
        "must use a durable timer: {before:?}"
    );
    let saved = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids(&Platform::Jira.to_string(), &["PROJ-1".into()])
        .await
        .unwrap();
    assert_eq!(saved.len(), 1, "first page committed before durable sleep");
    ready.store(true, Ordering::SeqCst);
    runtime.restart_worker().await;
    let result = runtime.result(&invocation).await;
    assert!(result.is_complete);
    assert_eq!(result.items_stored, 2);
    let after = runtime.query(&journal_query).await;
    assert_eq!(
        after.get(..before.len()).unwrap(),
        before.as_slice(),
        "restart preserves the original journal prefix"
    );
    let requests = ctx.mock_server.received_requests().await.unwrap();
    for request in requests
        .iter()
        .filter(|request| request.url.path() == "/rest/api/3/search/jql")
    {
        let query = request
            .url
            .query_pairs()
            .find(|(key, _)| key == "jql")
            .unwrap()
            .1;
        assert!(query.contains(&format!("assignee = \"{ACCOUNT}\"")));
        assert!(query.contains("project = \"PROJ\""));
    }
    let journal = runtime.journal_bytes(&invocation).await;
    let contains = |value: &str| {
        journal
            .windows(value.len())
            .any(|window| window == value.as_bytes())
    };
    assert!(
        contains(ACCOUNT),
        "raw journal must retain the selected input snapshot"
    );
    assert!(
        !contains(PRIVATE_TOKEN),
        "decrypted credentials must stay outside the journal"
    );
    assert!(
        !contains("Scoped worker fixture"),
        "raw issue content must stay outside the journal"
    );
    assert!(
        contains("checkpoint_person_fetch"),
        "durable fetch decisions must remain inspectable"
    );
    assert_eq!(
        ctx.repos
            .activity
            .get_watermark("selected Jira")
            .await
            .unwrap()
            .as_deref(),
        Some("2026-01-01T00:00:00Z")
    );
    drop(runtime);
    ctx.teardown().await;
}

#[tokio::test]
async fn cancelling_sleeping_owned_chunk_does_not_cancel_unrelated_scheduled_work() {
    let ctx = SourceTestContext::new().await;
    let ingestion = scoped_jira(&ctx).await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "30"))
        .expect(1)
        .mount(&ctx.mock_server)
        .await;
    let runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let request = chunk_request(&ingestion, 2).await;
    let mut ordinary = request.clone();
    ordinary.request = None;
    ordinary.run_id = Uuid::now_v7();
    let unrelated = runtime.send("process_chunk", &ordinary, Some("1h")).await;
    let invocation = runtime.send_chunk(&request).await;
    runtime.wait_status(&invocation, "suspended").await;
    let pipeline = ingestion.request.as_ref().unwrap().pipeline_id;
    let owned = ctx
        .repos
        .activity
        .list_pipeline_invocation_ids(pipeline)
        .await
        .unwrap();
    assert!(owned.contains(&invocation));
    assert!(!owned.contains(&unrelated));
    ctx.repos
        .activity
        .request_pipeline_cancel(pipeline)
        .await
        .unwrap();
    runtime.kill(&invocation).await;
    let cancelled = runtime.attach(&invocation).await;
    assert!(
        !cancelled.status().is_success(),
        "cancelled chunk must not complete"
    );
    runtime.wait_status(&unrelated, "scheduled").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        ctx.repos
            .activity
            .get_contribution_ids_by_platform_ids(&Platform::Jira.to_string(), &["PROJ-1".into()])
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ctx.repos
            .activity
            .get_watermark("selected Jira")
            .await
            .unwrap()
            .as_deref(),
        Some("2026-01-01T00:00:00Z")
    );
    drop(runtime);
    ctx.teardown().await;
}

#[tokio::test]
async fn ordinary_chunk_keeps_global_watermark_and_legacy_journal_behavior() {
    use ps_core::models::{HandlerMethod, HandlerName, SourceName};

    let ctx = SourceTestContext::new().await;
    let mut ingestion = scoped_jira(&ctx).await;
    let independent_pipeline = ingestion.request.as_ref().unwrap().pipeline_id;
    ingestion.request = None;
    let run_id = Uuid::now_v7();
    ctx.repos
        .activity
        .create_run(
            run_id,
            &SourceName::new("selected Jira"),
            &HandlerName::new("JiraIngestionHandler"),
            &HandlerMethod::new("run_ingestion"),
        )
        .await
        .unwrap();
    ingestion.run_id = Some(run_id);
    // The fixture's older source watermark intentionally predates the issue.
    ctx.repos
        .activity
        .upsert_watermark("selected Jira", "2025-03-01T00:00:00Z", 0)
        .await
        .unwrap();
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param(
            "jql",
            "project = \"PROJ\" AND updated >= \"2025-03-01 00:00\" ORDER BY updated ASC",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jira_search_response(
                &[issue("PROJ-1", ACCOUNT)],
                true,
                None,
            )),
        )
        .expect(1)
        .mount(&ctx.mock_server)
        .await;
    let runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let request = chunk_request(&ingestion, 2).await;
    let invocation = runtime.send("process_chunk", &request, None).await;
    let result = runtime.result(&invocation).await;
    assert!(result.is_complete);
    assert_eq!(result.items_stored, 1);
    assert_eq!(
        ctx.repos
            .activity
            .get_watermark("selected Jira")
            .await
            .unwrap()
            .as_deref(),
        Some(UPPER)
    );
    let journal = runtime
        .query(&format!(
            "SELECT name FROM sys_journal WHERE id = '{invocation}'"
        ))
        .await;
    assert!(
        journal.iter().any(
            |entry| entry.get("name").and_then(serde_json::Value::as_str) == Some("fetch_batch")
        )
    );
    assert!(!journal.iter().any(
        |entry| entry.get("name").and_then(serde_json::Value::as_str)
            == Some("checkpoint_person_fetch")
    ));
    let owned = ctx
        .repos
        .activity
        .list_pipeline_invocation_ids(independent_pipeline)
        .await
        .unwrap();
    assert!(
        !owned.contains(&invocation),
        "ordinary runs have no pipeline ownership"
    );
    drop(runtime);
    ctx.teardown().await;
}
