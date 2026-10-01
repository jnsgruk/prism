//! Acceptance of the actual scoped workflow across storage and downstream work.

use std::time::Duration;

use ps_core::ingestion::{IngestionContext, PipelineRequest};
use ps_core::models::Platform;
use uuid::Uuid;
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};

use super::scoped_chunks::{ACCOUNT, issue, scoped_jira};
use crate::common::restate::RestateTestContext;
use crate::common::wiremock_helpers::{SourceTestContext, jira_search_response};

async fn fixture(ctx: &SourceTestContext) -> IngestionContext {
    let ingestion = scoped_jira(ctx).await;
    ctx.repos
        .org
        .create_team(
            "Release snapshot team",
            "Fixture org",
            ps_core::models::TeamType::Team,
            None,
            None,
        )
        .await
        .unwrap();
    // The adapter fixture reserves an owned run; the real workflow will create
    // its own coordinator and processing runs.
    ctx.repos
        .activity
        .complete_run(ingestion.run_id.unwrap(), 0)
        .await
        .unwrap();
    let fixture_pipeline = ingestion.request.as_ref().unwrap().pipeline_id;
    sqlx::query!("DELETE FROM activity.pipeline_invocations WHERE pipeline_id = $1 AND invocation_id = 'test-invocation'", fixture_pipeline).execute(&ctx.pool).await.unwrap();
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jira_search_response(
                &[issue("PROJ-1", ACCOUNT), issue("OTHER-1", "outsider")],
                true,
                None,
            )),
        )
        .mount(&ctx.mock_server)
        .await;
    ingestion
}

fn snapshot(ingestion: &IngestionContext) -> PipelineRequest {
    let request = ingestion.request.as_ref().unwrap();
    PipelineRequest {
        scope: request.scope.clone(),
        sources: vec![request.source.clone()],
        since_date: request.since_date.clone(),
        run_started_at: request.run_started_at,
        processing: request.processing.clone(),
    }
}

async fn wait_stage(ctx: &SourceTestContext, pipeline_id: Uuid, expected: &str) {
    for _ in 0..200 {
        let pipeline = ctx
            .repos
            .activity
            .get_pipeline(pipeline_id)
            .await
            .unwrap()
            .unwrap();
        if pipeline.current_stage.as_deref() == Some(expected) {
            return;
        }
        assert!(
            !matches!(pipeline.status.as_str(), "failed" | "cancelled"),
            "workflow terminated early: {pipeline:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("workflow did not enter {expected}");
}

async fn stored_ids(ctx: &SourceTestContext) -> Vec<(Uuid, String)> {
    ctx.repos
        .activity
        .get_contribution_ids_by_platform_ids(
            &Platform::Jira.to_string(),
            &["PROJ-1".into(), "OTHER-1".into()],
        )
        .await
        .unwrap()
}

async fn wait_historical_write(
    runtime: &RestateTestContext,
    ctx: &SourceTestContext,
    pipeline_id: Uuid,
) {
    for _ in 0..200 {
        let rows = runtime.query("SELECT id FROM sys_invocation WHERE target_service_name = 'HistoricalSnapshotService'").await;
        for row in rows {
            let id = row["id"].as_str().unwrap();
            let journal = runtime
                .query(&format!("SELECT name FROM sys_journal WHERE id = '{id}'"))
                .await;
            if journal.iter().any(|entry| {
                entry["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("refresh_metrics_"))
            }) {
                let owned = ctx
                    .repos
                    .activity
                    .list_pipeline_invocation_ids(pipeline_id)
                    .await
                    .unwrap();
                assert!(
                    owned.iter().any(|invocation| invocation == id),
                    "historical batch must be owned before writes"
                );
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("historical snapshot write did not start");
}

#[tokio::test]
async fn restart_during_historical_processing_preserves_storage_and_pending_work_then_repeat_is_stable()
 {
    let ctx = SourceTestContext::new().await;
    let ingestion = fixture(&ctx).await;
    let pipeline_id = ingestion.request.as_ref().unwrap().pipeline_id;
    let request = snapshot(&ingestion);
    let mut blocker = ctx.pool.begin().await.unwrap();
    sqlx::query!("LOCK TABLE metrics.team_snapshots IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let mut runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let invocation = runtime.send_pipeline(pipeline_id, &request).await;
    wait_stage(&ctx, pipeline_id, "metrics").await;
    wait_historical_write(&runtime, &ctx, pipeline_id).await;
    let before = stored_ids(&ctx).await;
    assert_eq!(
        before.len(),
        1,
        "mixed actor response stores only the selected account"
    );
    let pending = ctx
        .repos
        .activity
        .count_pending_snapshot_invalidations(pipeline_id)
        .await
        .unwrap();
    assert!(
        pending.0 > 0 && pending.1 > 0,
        "storage must commit durable downstream work"
    );
    let journal = runtime.query(&format!("SELECT index,entry_type,name,entry_lite_json FROM sys_journal WHERE id = '{invocation}' ORDER BY index")).await;
    assert!(!journal.is_empty());
    runtime.restart_worker().await;
    assert_eq!(stored_ids(&ctx).await, before);
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(pipeline_id)
            .await
            .unwrap(),
        pending
    );
    blocker.commit().await.unwrap();
    let response = runtime.attach(&invocation).await;
    let result: serde_json::Value = response.json().await.unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(pipeline_id)
            .await
            .unwrap(),
        (0, 0)
    );
    assert_eq!(stored_ids(&ctx).await, before);
    assert_eq!(
        runtime.provider.received_requests().await.unwrap().len(),
        1,
        "embedding work uses the local provider fixture"
    );
    let after = runtime.query(&format!("SELECT index,entry_type,name,entry_lite_json FROM sys_journal WHERE id = '{invocation}' ORDER BY index")).await;
    assert_eq!(
        after.get(..journal.len()).unwrap(),
        journal.as_slice(),
        "workflow journal prefix survives interruption"
    );
    let repeat = Uuid::now_v7();
    ctx.repos
        .activity
        .reserve_pipeline(
            repeat,
            &serde_json::to_value(&request).unwrap(),
            Uuid::now_v7(),
            "test-admin",
        )
        .await
        .unwrap();
    let repeated = runtime.send_pipeline(repeat, &request).await;
    let response = runtime.attach(&repeated).await;
    let result: serde_json::Value = response.json().await.unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    assert_eq!(stored_ids(&ctx).await, before);
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
async fn cancellation_after_storage_retains_pending_history_and_preserves_unrelated_invocation() {
    let ctx = SourceTestContext::new().await;
    let ingestion = fixture(&ctx).await;
    let pipeline_id = ingestion.request.as_ref().unwrap().pipeline_id;
    let request = snapshot(&ingestion);
    let mut blocker = ctx.pool.begin().await.unwrap();
    sqlx::query!("LOCK TABLE metrics.team_snapshots IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let unrelated = runtime
        .send(
            "process_chunk",
            &ps_workers::features::ingestion::lib::chunk::ChunkRequest {
                source_type: Platform::Jira,
                cursor: "unused-scheduled-fixture".into(),
                run_id: Uuid::now_v7(),
                max_batches: 1,
                items_offset: 0,
                request: None,
            },
            Some("1h"),
        )
        .await;
    let invocation = runtime.send_pipeline(pipeline_id, &request).await;
    wait_stage(&ctx, pipeline_id, "metrics").await;
    wait_historical_write(&runtime, &ctx, pipeline_id).await;
    let saved = stored_ids(&ctx).await;
    assert_eq!(saved.len(), 1);
    let pending = ctx
        .repos
        .activity
        .count_pending_snapshot_invalidations(pipeline_id)
        .await
        .unwrap();
    assert!(pending.0 > 0);
    runtime.cancel_pipeline(pipeline_id).await;
    assert!(
        ctx.repos
            .activity
            .get_pipeline(pipeline_id)
            .await
            .unwrap()
            .unwrap()
            .cancellation_requested
    );
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(pipeline_id)
            .await
            .unwrap(),
        pending
    );
    // Cancellation drains in-flight side effects before finalizing. Let the
    // held database statement finish so the post-write guard can reject it.
    blocker.commit().await.unwrap();
    let response = runtime.attach(&invocation).await;
    let result: serde_json::Value = response.json().await.unwrap();
    assert_eq!(result["status"], "cancelled", "{result}");
    assert_eq!(
        stored_ids(&ctx).await,
        saved,
        "committed work remains traceable after cancellation"
    );
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(pipeline_id)
            .await
            .unwrap(),
        pending
    );
    runtime.wait_status(&unrelated, "scheduled").await;
    runtime.recover_snapshots().await;
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(pipeline_id)
            .await
            .unwrap(),
        (0, 0),
        "terminal owner recovery drains committed work after cancellation"
    );
    assert_eq!(stored_ids(&ctx).await, saved);
    runtime.wait_status(&unrelated, "scheduled").await;
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
async fn partial_selected_source_failure_cannot_report_complete_pipeline_coverage() {
    let ctx = SourceTestContext::new().await;
    let ingestion = scoped_jira(&ctx).await;
    ctx.repos
        .activity
        .complete_run(ingestion.run_id.unwrap(), 0)
        .await
        .unwrap();
    let fixture_pipeline = ingestion.request.as_ref().unwrap().pipeline_id;
    sqlx::query!("DELETE FROM activity.pipeline_invocations WHERE pipeline_id = $1 AND invocation_id = 'test-invocation'", fixture_pipeline).execute(&ctx.pool).await.unwrap();
    ctx.repos
        .config
        .update_source_settings(
            ingestion.source_config.id.into_inner(),
            &serde_json::json!({
                "base_url":ctx.mock_server.uri(), "api_mode":"cloud", "projects":["PROJ","PRIVATE"],
            }),
        )
        .await
        .unwrap();
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(|request: &wiremock::Request| {
            let query = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "jql")
                .unwrap()
                .1;
            if query.contains("project = \"PRIVATE\"") {
                ResponseTemplate::new(403).set_body_string("private upstream failure detail")
            } else {
                ResponseTemplate::new(200).set_body_json(jira_search_response(
                    &[issue("PROJ-1", ACCOUNT)],
                    true,
                    None,
                ))
            }
        })
        .expect(2)
        .mount(&ctx.mock_server)
        .await;
    let runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let id = ingestion.request.as_ref().unwrap().pipeline_id;
    let invocation = runtime.send_pipeline(id, &snapshot(&ingestion)).await;
    let response = runtime.attach(&invocation).await;
    let result: serde_json::Value = response.json().await.unwrap();
    assert_ne!(
        result["status"], "completed",
        "partial source coverage must never be reported complete"
    );
    assert_eq!(runtime.provider.received_requests().await.unwrap().len(), 1);
    let pipeline = ctx.repos.activity.get_pipeline(id).await.unwrap().unwrap();
    assert_eq!(pipeline.status, "completed_with_warnings");
    assert_eq!(
        stored_ids(&ctx).await.len(),
        1,
        "successful project committed before the failure"
    );
    let (metrics, insights) = ctx
        .repos
        .activity
        .count_pending_snapshot_invalidations(id)
        .await
        .unwrap();
    assert_eq!(
        (metrics, insights),
        (0, 0),
        "committed partial work is recalculated despite the source error"
    );
    let runs = ctx
        .repos
        .activity
        .list_runs(Some("selected Jira"), Some("JiraIngestionHandler"), false)
        .await
        .unwrap();
    let run = runs.first().unwrap();
    assert_eq!(
        run.status,
        ps_core::models::IngestionStatus::CompletedWithWarnings
    );
    let progress = run.progress.as_ref().unwrap();
    assert_eq!(progress["failed_items"].as_array().unwrap().len(), 1);
    assert!(
        !progress
            .to_string()
            .contains("private upstream failure detail")
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
