use crate::common::db::RepoTestContext;
use ps_core::ingestion::ContributionInput;
use ps_core::models::{
    ContributionState, ContributionType, HandlerMethod, HandlerName, IngestionStatus, Platform,
    SourceName,
};
use time::OffsetDateTime;
use uuid::Uuid;

/// Helper to create a run with typed newtypes from string literals.
async fn test_create_run(
    activity: &ps_core::repo::ActivityRepo,
    id: Uuid,
    source: &str,
    handler: &str,
    method: &str,
) {
    activity
        .create_run(
            id,
            &SourceName::new(source),
            &HandlerName::new(handler),
            &HandlerMethod::new(method),
        )
        .await
        .unwrap();
}

/// Build a minimal ContributionInput for testing.
fn make_contribution(
    platform: Platform,
    contribution_type: ContributionType,
    platform_id: &str,
) -> ContributionInput {
    ContributionInput {
        platform,
        contribution_type,
        platform_id: platform_id.into(),
        platform_username: "testuser".into(),
        title: Some(format!("Test {platform_id}")),
        url: Some(format!("https://example.com/{platform_id}")),
        state: Some(ContributionState::Open),
        created_at: OffsetDateTime::now_utc(),
        updated_at: None,
        closed_at: None,
        metrics: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: Some("Test content".into()),
        state_history: None,
        enrichment_content: None,
    }
}

// ---------------------------------------------------------------------------
// Contributions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn upsert_contribution_and_retrieve() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let item = make_contribution(Platform::Github, ContributionType::PullRequest, "gh-pr-1");
    let id = Uuid::now_v7();
    repos
        .activity
        .upsert_contribution(id, None, &item)
        .await
        .unwrap();

    // Verify via raw SQL that the row exists
    let row: (Uuid,) =
        sqlx::query_as("SELECT id FROM activity.contributions WHERE platform_id = 'gh-pr-1'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(row.0, id);

    ctx.teardown().await;
}

#[tokio::test]
async fn upsert_contribution_idempotent() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let item = make_contribution(Platform::Github, ContributionType::PullRequest, "gh-pr-2");
    let id1 = Uuid::now_v7();
    repos
        .activity
        .upsert_contribution(id1, None, &item)
        .await
        .unwrap();

    // Upsert same platform_id again with different UUID — should update, not create new
    let mut item2 = item.clone();
    item2.title = Some("Updated title".into());
    let id2 = Uuid::now_v7();
    repos
        .activity
        .upsert_contribution(id2, None, &item2)
        .await
        .unwrap();

    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM activity.contributions WHERE platform = 'github' AND platform_id = 'gh-pr-2'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(count.0, 1);

    ctx.teardown().await;
}

#[tokio::test]
async fn bulk_upsert_contributions() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let items: Vec<ContributionInput> = (0..3)
        .map(|i| {
            make_contribution(
                Platform::Github,
                ContributionType::PullRequest,
                &format!("bulk-{i}"),
            )
        })
        .collect();
    let ids: Vec<Uuid> = (0..3).map(|_| Uuid::now_v7()).collect();
    let person_ids: Vec<Option<Uuid>> = vec![None; 3];
    let item_refs: Vec<&ContributionInput> = items.iter().collect();

    let result = repos
        .activity
        .bulk_upsert_contributions(&ids, &person_ids, &item_refs)
        .await
        .unwrap();
    assert_eq!(result.len(), 3);

    ctx.teardown().await;
}

#[tokio::test]
async fn bulk_upsert_empty_is_noop() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let result = repos
        .activity
        .bulk_upsert_contributions(&[], &[], &[])
        .await
        .unwrap();
    assert!(result.is_empty());

    ctx.teardown().await;
}

#[tokio::test]
async fn get_contribution_ids_by_platform_ids() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let item = make_contribution(Platform::Github, ContributionType::PullRequest, "lookup-1");
    let id = Uuid::now_v7();
    repos
        .activity
        .upsert_contribution(id, None, &item)
        .await
        .unwrap();

    let result = repos
        .activity
        .get_contribution_ids_by_platform_ids("github", &["lookup-1".into()])
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, id);
    assert_eq!(result[0].1, "lookup-1");

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_run_and_complete() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let run_id = Uuid::now_v7();
    test_create_run(
        &repos.activity,
        run_id,
        "github",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;

    let run = repos.activity.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.status, IngestionStatus::Running);
    assert!(run.completed_at.is_none());

    repos.activity.complete_run(run_id, 42).await.unwrap();

    let run = repos.activity.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.status, IngestionStatus::Completed);
    assert!(run.completed_at.is_some());
    assert_eq!(run.items_collected, Some(42));

    ctx.teardown().await;
}

#[tokio::test]
async fn fail_run_records_error() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let run_id = Uuid::now_v7();
    test_create_run(
        &repos.activity,
        run_id,
        "jira",
        "JiraIngestionHandler",
        "run_ingestion",
    )
    .await;

    repos
        .activity
        .fail_run(run_id, "Connection refused")
        .await
        .unwrap();

    let run = repos.activity.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.status, IngestionStatus::Failed);
    assert_eq!(run.error_message.as_deref(), Some("Connection refused"));

    ctx.teardown().await;
}

#[tokio::test]
async fn complete_run_with_warnings() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let run_id = Uuid::now_v7();
    test_create_run(
        &repos.activity,
        run_id,
        "github",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;

    let metadata = serde_json::json!({"failed_repos": ["canonical/broken"]});
    repos
        .activity
        .complete_run_with_warnings(run_id, 10, "1 repo(s) failed", metadata)
        .await
        .unwrap();

    let run = repos.activity.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.status, IngestionStatus::CompletedWithWarnings);
    assert_eq!(run.items_collected, Some(10));
    assert!(run.error_message.is_some());

    ctx.teardown().await;
}

#[tokio::test]
async fn list_runs_by_source() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let id1 = Uuid::now_v7();
    let id2 = Uuid::now_v7();
    let id3 = Uuid::now_v7();
    test_create_run(
        &repos.activity,
        id1,
        "github",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;
    test_create_run(
        &repos.activity,
        id2,
        "github",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;
    test_create_run(
        &repos.activity,
        id3,
        "jira",
        "JiraIngestionHandler",
        "run_ingestion",
    )
    .await;

    let gh_runs = repos
        .activity
        .list_runs(Some("github"), None, false)
        .await
        .unwrap();
    assert_eq!(gh_runs.len(), 2);

    let jira_runs = repos
        .activity
        .list_runs(Some("jira"), None, false)
        .await
        .unwrap();
    assert_eq!(jira_runs.len(), 1);

    let all_runs = repos.activity.list_runs(None, None, false).await.unwrap();
    assert_eq!(all_runs.len(), 3);

    ctx.teardown().await;
}

#[tokio::test]
async fn list_runs_ingestion_only() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    test_create_run(
        &repos.activity,
        Uuid::now_v7(),
        "github",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;
    test_create_run(
        &repos.activity,
        Uuid::now_v7(),
        "github",
        "GithubTeamSyncHandler",
        "sync_teams",
    )
    .await;
    test_create_run(
        &repos.activity,
        Uuid::now_v7(),
        "metrics",
        "MetricsComputeHandler",
        "compute_current_periods",
    )
    .await;

    let ingestion = repos.activity.list_runs(None, None, true).await.unwrap();
    assert_eq!(ingestion.len(), 1);

    let all = repos.activity.list_runs(None, None, false).await.unwrap();
    assert_eq!(all.len(), 3);

    ctx.teardown().await;
}

#[tokio::test]
async fn cancel_run_by_id() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let run_id = Uuid::now_v7();
    test_create_run(
        &repos.activity,
        run_id,
        "github",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;

    repos.activity.cancel_run_by_id(run_id).await.unwrap();

    let run = repos.activity.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.status, IngestionStatus::Cancelled);

    ctx.teardown().await;
}

#[tokio::test]
async fn get_active_handler_runs() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let id1 = Uuid::now_v7();
    let id2 = Uuid::now_v7();
    test_create_run(
        &repos.activity,
        id1,
        "github",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;
    test_create_run(
        &repos.activity,
        id2,
        "jira",
        "JiraIngestionHandler",
        "run_ingestion",
    )
    .await;
    repos.activity.complete_run(id2, 5).await.unwrap();

    let active = repos.activity.get_active_handler_runs().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, id1);

    ctx.teardown().await;
}

#[tokio::test]
async fn update_run_progress() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let run_id = Uuid::now_v7();
    test_create_run(
        &repos.activity,
        run_id,
        "github",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;

    repos
        .activity
        .update_run_progress(run_id, 25)
        .await
        .unwrap();

    let run = repos.activity.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.items_collected, Some(25));

    ctx.teardown().await;
}

#[tokio::test]
async fn update_run_progress_detail() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let run_id = Uuid::now_v7();
    test_create_run(
        &repos.activity,
        run_id,
        "github",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;

    let progress = serde_json::json!({"prs": 10, "reviews": 5});
    repos
        .activity
        .update_run_progress_detail(run_id, 15, &progress)
        .await
        .unwrap();

    let row: (Option<serde_json::Value>,) =
        sqlx::query_as("SELECT progress FROM activity.ingestion_runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(row.0.unwrap()["prs"], 10);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Watermarks
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_watermark_and_advance() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    // Initially none
    let wm = repos.activity.get_watermark("github").await.unwrap();
    assert!(wm.is_none());

    repos
        .activity
        .upsert_watermark("github", "2025-01-01T00:00:00Z", 100)
        .await
        .unwrap();

    let wm = repos.activity.get_watermark("github").await.unwrap();
    assert_eq!(wm.as_deref(), Some("2025-01-01T00:00:00Z"));

    // Advance
    repos
        .activity
        .upsert_watermark("github", "2025-02-01T00:00:00Z", 50)
        .await
        .unwrap();

    let wm = repos.activity.get_watermark("github").await.unwrap();
    assert_eq!(wm.as_deref(), Some("2025-02-01T00:00:00Z"));

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// ETag cache
// ---------------------------------------------------------------------------

#[tokio::test]
async fn store_and_check_etag() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let etag = repos
        .activity
        .get_cached_etag("github", "https://api.github.com/repos/foo/bar")
        .await
        .unwrap();
    assert!(etag.is_none());

    repos
        .activity
        .set_cached_etag(
            "github",
            "https://api.github.com/repos/foo/bar",
            "W/\"abc123\"",
        )
        .await
        .unwrap();

    let etag = repos
        .activity
        .get_cached_etag("github", "https://api.github.com/repos/foo/bar")
        .await
        .unwrap();
    assert_eq!(etag.as_deref(), Some("W/\"abc123\""));

    // Overwrite
    repos
        .activity
        .set_cached_etag(
            "github",
            "https://api.github.com/repos/foo/bar",
            "W/\"def456\"",
        )
        .await
        .unwrap();

    let etag = repos
        .activity
        .get_cached_etag("github", "https://api.github.com/repos/foo/bar")
        .await
        .unwrap();
    assert_eq!(etag.as_deref(), Some("W/\"def456\""));

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Invocation tracking
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invocation_tracking() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    // set_current_invocation_id upserts the watermark row if needed
    repos
        .activity
        .set_current_invocation_id("github", "inv-abc-123")
        .await
        .unwrap();

    let inv_id = repos
        .activity
        .get_current_invocation_id("github")
        .await
        .unwrap();
    assert_eq!(inv_id.as_deref(), Some("inv-abc-123"));

    // Clear
    repos
        .activity
        .clear_current_invocation_id("github")
        .await
        .unwrap();

    let inv_id = repos
        .activity
        .get_current_invocation_id("github")
        .await
        .unwrap();
    assert!(inv_id.is_none());

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Source status (cross-schema join)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_source_statuses_cross_schema_join() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    // Create an enabled source
    let settings = serde_json::json!({});
    repos
        .config
        .create_source(Uuid::now_v7(), "github", "GH", &settings, None)
        .await
        .unwrap();

    // Add watermark
    repos
        .activity
        .upsert_watermark("GH", "2025-01-01", 50)
        .await
        .unwrap();

    // Create a completed run
    let run_id = Uuid::now_v7();
    test_create_run(
        &repos.activity,
        run_id,
        "GH",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;
    repos.activity.complete_run(run_id, 50).await.unwrap();

    let statuses = repos.activity.get_source_statuses().await.unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].name, "GH");
    assert_eq!(statuses[0].watermark_value.as_deref(), Some("2025-01-01"));
    assert!(!statuses[0].has_active_run);
    assert_eq!(statuses[0].items_collected_last_run, Some(50));

    ctx.teardown().await;
}

#[tokio::test]
async fn get_source_statuses_with_active_run() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let settings = serde_json::json!({});
    repos
        .config
        .create_source(Uuid::now_v7(), "github", "GH", &settings, None)
        .await
        .unwrap();

    let run_id = Uuid::now_v7();
    test_create_run(
        &repos.activity,
        run_id,
        "GH",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;
    repos
        .activity
        .update_run_progress(run_id, 10)
        .await
        .unwrap();

    let statuses = repos.activity.get_source_statuses().await.unwrap();
    assert_eq!(statuses.len(), 1);
    assert!(statuses[0].has_active_run);
    assert_eq!(statuses[0].active_run_items, Some(10));

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Cancel & reset
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cancel_active_runs() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let id1 = Uuid::now_v7();
    let id2 = Uuid::now_v7();
    test_create_run(
        &repos.activity,
        id1,
        "github",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;
    test_create_run(
        &repos.activity,
        id2,
        "github",
        "GithubIngestionHandler",
        "backfill",
    )
    .await;

    repos.activity.cancel_active_runs("github").await.unwrap();

    let r1 = repos.activity.get_run(id1).await.unwrap().unwrap();
    let r2 = repos.activity.get_run(id2).await.unwrap().unwrap();
    assert_eq!(r1.status, IngestionStatus::Cancelled);
    assert_eq!(r2.status, IngestionStatus::Cancelled);

    ctx.teardown().await;
}

#[tokio::test]
async fn reset_all_clears_everything() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    // Seed some data
    let item = make_contribution(Platform::Github, ContributionType::PullRequest, "reset-1");
    repos
        .activity
        .upsert_contribution(Uuid::now_v7(), None, &item)
        .await
        .unwrap();
    repos
        .activity
        .upsert_watermark("github", "2025-01-01", 1)
        .await
        .unwrap();
    test_create_run(
        &repos.activity,
        Uuid::now_v7(),
        "github",
        "GithubIngestionHandler",
        "run_ingestion",
    )
    .await;
    repos
        .activity
        .set_cached_etag("github", "url", "etag")
        .await
        .unwrap();

    let deleted = repos.activity.reset_all().await.unwrap();
    assert_eq!(deleted, 1);

    // Verify all cleared
    assert!(
        repos
            .activity
            .get_watermark("github")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        repos
            .activity
            .list_runs(None, None, false)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        repos
            .activity
            .get_cached_etag("github", "url")
            .await
            .unwrap()
            .is_none()
    );

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Pipelines
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_pipeline_and_retrieve() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let id = Uuid::now_v7();
    let pipeline = repos
        .activity
        .create_pipeline(id, Some("inv-123"))
        .await
        .unwrap();

    assert_eq!(pipeline.id, id);
    assert_eq!(pipeline.status, "running");
    assert!(pipeline.current_stage.is_none());
    assert!(pipeline.completed_at.is_none());
    assert_eq!(pipeline.current_invocation_id.as_deref(), Some("inv-123"));
    assert_eq!(pipeline.stages, serde_json::json!({}));

    let latest = repos.activity.get_latest_pipeline().await.unwrap().unwrap();
    assert_eq!(latest.id, id);
    assert_eq!(latest.status, "running");

    ctx.teardown().await;
}

#[tokio::test]
async fn update_pipeline_stage_advances() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let id = Uuid::now_v7();
    repos.activity.create_pipeline(id, None).await.unwrap();

    let stages = serde_json::json!({
        "team_sync": { "status": "completed" },
        "ingestion": { "status": "running" }
    });
    repos
        .activity
        .update_pipeline_stage(id, "ingestion", &stages)
        .await
        .unwrap();

    let pipeline = repos.activity.get_latest_pipeline().await.unwrap().unwrap();
    assert_eq!(pipeline.current_stage.as_deref(), Some("ingestion"));
    assert_eq!(pipeline.stages["team_sync"]["status"], "completed");
    assert_eq!(pipeline.stages["ingestion"]["status"], "running");

    ctx.teardown().await;
}

#[tokio::test]
async fn complete_pipeline_sets_status() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let id = Uuid::now_v7();
    repos
        .activity
        .create_pipeline(id, Some("inv-456"))
        .await
        .unwrap();

    let stages = serde_json::json!({
        "ingestion": { "status": "completed" },
        "metrics": { "status": "completed" }
    });
    repos
        .activity
        .complete_pipeline(id, "completed", &stages, None)
        .await
        .unwrap();

    let pipeline = repos.activity.get_latest_pipeline().await.unwrap().unwrap();
    assert_eq!(pipeline.status, "completed");
    assert!(pipeline.completed_at.is_some());
    assert!(pipeline.current_invocation_id.is_none());
    assert!(pipeline.error.is_none());

    // Test failed status with error
    let id2 = Uuid::now_v7();
    repos.activity.create_pipeline(id2, None).await.unwrap();

    repos
        .activity
        .complete_pipeline(
            id2,
            "failed",
            &serde_json::json!({}),
            Some("all handlers failed"),
        )
        .await
        .unwrap();

    let pipeline = repos.activity.get_latest_pipeline().await.unwrap().unwrap();
    assert_eq!(pipeline.status, "failed");
    assert_eq!(pipeline.error.as_deref(), Some("all handlers failed"));

    ctx.teardown().await;
}

#[tokio::test]
async fn list_recent_pipelines_ordered() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    // Create 3 pipelines
    for _ in 0..3 {
        let id = Uuid::now_v7();
        repos.activity.create_pipeline(id, None).await.unwrap();
        repos
            .activity
            .complete_pipeline(id, "completed", &serde_json::json!({}), None)
            .await
            .unwrap();
    }

    let recent = repos.activity.list_recent_pipelines(10).await.unwrap();
    assert_eq!(recent.len(), 3);

    // Verify ordering: most recent first
    for pair in recent.windows(2) {
        assert!(pair[0].started_at >= pair[1].started_at);
    }

    // Verify limit
    let limited = repos.activity.list_recent_pipelines(2).await.unwrap();
    assert_eq!(limited.len(), 2);

    ctx.teardown().await;
}

#[tokio::test]
async fn has_active_pipeline_check() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    assert!(!repos.activity.has_active_pipeline().await.unwrap());

    let id = Uuid::now_v7();
    repos.activity.create_pipeline(id, None).await.unwrap();

    assert!(repos.activity.has_active_pipeline().await.unwrap());

    repos
        .activity
        .complete_pipeline(id, "completed", &serde_json::json!({}), None)
        .await
        .unwrap();

    assert!(!repos.activity.has_active_pipeline().await.unwrap());

    ctx.teardown().await;
}
