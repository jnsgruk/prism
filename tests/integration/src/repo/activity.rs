use crate::define_repo_test;
use ps_core::ingestion::ContributionInput;
use ps_core::models::{ContributionState, ContributionType, IngestionStatus, Platform};
use time::OffsetDateTime;
use uuid::Uuid;

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

define_repo_test!(upsert_contribution_and_retrieve, |repos, pool| async move {
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
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, id);
});

define_repo_test!(upsert_contribution_idempotent, |repos, pool| async move {
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
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count.0, 1);
});

define_repo_test!(bulk_upsert_contributions, |repos, _pool| async move {
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
});

define_repo_test!(bulk_upsert_empty_is_noop, |repos, _pool| async move {
    let result = repos
        .activity
        .bulk_upsert_contributions(&[], &[], &[])
        .await
        .unwrap();
    assert!(result.is_empty());
});

define_repo_test!(
    get_contribution_ids_by_platform_ids,
    |repos, _pool| async move {
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
    }
);

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

define_repo_test!(create_run_and_complete, |repos, _pool| async move {
    let run_id = Uuid::now_v7();
    repos
        .activity
        .create_run(run_id, "github", "GithubIngestionHandler", "run_ingestion")
        .await
        .unwrap();

    let run = repos.activity.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.status, IngestionStatus::Running);
    assert!(run.completed_at.is_none());

    repos.activity.complete_run(run_id, 42).await.unwrap();

    let run = repos.activity.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.status, IngestionStatus::Completed);
    assert!(run.completed_at.is_some());
    assert_eq!(run.items_collected, Some(42));
});

define_repo_test!(fail_run_records_error, |repos, _pool| async move {
    let run_id = Uuid::now_v7();
    repos
        .activity
        .create_run(run_id, "jira", "JiraIngestionHandler", "run_ingestion")
        .await
        .unwrap();

    repos
        .activity
        .fail_run(run_id, "Connection refused")
        .await
        .unwrap();

    let run = repos.activity.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.status, IngestionStatus::Failed);
    assert_eq!(run.error_message.as_deref(), Some("Connection refused"));
});

define_repo_test!(complete_run_with_warnings, |repos, _pool| async move {
    let run_id = Uuid::now_v7();
    repos
        .activity
        .create_run(run_id, "github", "GithubIngestionHandler", "run_ingestion")
        .await
        .unwrap();

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
});

define_repo_test!(list_runs_by_source, |repos, _pool| async move {
    let id1 = Uuid::now_v7();
    let id2 = Uuid::now_v7();
    let id3 = Uuid::now_v7();
    repos
        .activity
        .create_run(id1, "github", "GithubIngestionHandler", "run_ingestion")
        .await
        .unwrap();
    repos
        .activity
        .create_run(id2, "github", "GithubIngestionHandler", "run_ingestion")
        .await
        .unwrap();
    repos
        .activity
        .create_run(id3, "jira", "JiraIngestionHandler", "run_ingestion")
        .await
        .unwrap();

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
});

define_repo_test!(list_runs_ingestion_only, |repos, _pool| async move {
    repos
        .activity
        .create_run(
            Uuid::now_v7(),
            "github",
            "GithubIngestionHandler",
            "run_ingestion",
        )
        .await
        .unwrap();
    repos
        .activity
        .create_run(
            Uuid::now_v7(),
            "github",
            "GithubTeamSyncHandler",
            "sync_teams",
        )
        .await
        .unwrap();
    repos
        .activity
        .create_run(
            Uuid::now_v7(),
            "metrics",
            "MetricsComputeHandler",
            "compute_current_periods",
        )
        .await
        .unwrap();

    let ingestion = repos.activity.list_runs(None, None, true).await.unwrap();
    assert_eq!(ingestion.len(), 1);

    let all = repos.activity.list_runs(None, None, false).await.unwrap();
    assert_eq!(all.len(), 3);
});

define_repo_test!(cancel_run_by_id, |repos, _pool| async move {
    let run_id = Uuid::now_v7();
    repos
        .activity
        .create_run(run_id, "github", "GithubIngestionHandler", "run_ingestion")
        .await
        .unwrap();

    repos.activity.cancel_run_by_id(run_id).await.unwrap();

    let run = repos.activity.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.status, IngestionStatus::Cancelled);
});

define_repo_test!(get_active_handler_runs, |repos, _pool| async move {
    let id1 = Uuid::now_v7();
    let id2 = Uuid::now_v7();
    repos
        .activity
        .create_run(id1, "github", "GithubIngestionHandler", "run_ingestion")
        .await
        .unwrap();
    repos
        .activity
        .create_run(id2, "jira", "JiraIngestionHandler", "run_ingestion")
        .await
        .unwrap();
    repos.activity.complete_run(id2, 5).await.unwrap();

    let active = repos.activity.get_active_handler_runs().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, id1);
});

define_repo_test!(update_run_progress, |repos, _pool| async move {
    let run_id = Uuid::now_v7();
    repos
        .activity
        .create_run(run_id, "github", "GithubIngestionHandler", "run_ingestion")
        .await
        .unwrap();

    repos
        .activity
        .update_run_progress(run_id, 25)
        .await
        .unwrap();

    let run = repos.activity.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.items_collected, Some(25));
});

define_repo_test!(update_run_progress_detail, |repos, pool| async move {
    let run_id = Uuid::now_v7();
    repos
        .activity
        .create_run(run_id, "github", "GithubIngestionHandler", "run_ingestion")
        .await
        .unwrap();

    let progress = serde_json::json!({"prs": 10, "reviews": 5});
    repos
        .activity
        .update_run_progress_detail(run_id, 15, &progress)
        .await
        .unwrap();

    let row: (Option<serde_json::Value>,) =
        sqlx::query_as("SELECT progress FROM activity.ingestion_runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0.unwrap()["prs"], 10);
});

// ---------------------------------------------------------------------------
// Watermarks
// ---------------------------------------------------------------------------

define_repo_test!(get_watermark_and_advance, |repos, _pool| async move {
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
});

// ---------------------------------------------------------------------------
// ETag cache
// ---------------------------------------------------------------------------

define_repo_test!(store_and_check_etag, |repos, _pool| async move {
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
});

// ---------------------------------------------------------------------------
// Invocation tracking
// ---------------------------------------------------------------------------

define_repo_test!(invocation_tracking, |repos, _pool| async move {
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
});

// ---------------------------------------------------------------------------
// Source status (cross-schema join)
// ---------------------------------------------------------------------------

define_repo_test!(
    get_source_statuses_cross_schema_join,
    |repos, _pool| async move {
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
        repos
            .activity
            .create_run(run_id, "GH", "GithubIngestionHandler", "run_ingestion")
            .await
            .unwrap();
        repos.activity.complete_run(run_id, 50).await.unwrap();

        let statuses = repos.activity.get_source_statuses().await.unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].name, "GH");
        assert_eq!(statuses[0].watermark_value.as_deref(), Some("2025-01-01"));
        assert!(!statuses[0].has_active_run);
        assert_eq!(statuses[0].items_collected_last_run, Some(50));
    }
);

define_repo_test!(
    get_source_statuses_with_active_run,
    |repos, _pool| async move {
        let settings = serde_json::json!({});
        repos
            .config
            .create_source(Uuid::now_v7(), "github", "GH", &settings, None)
            .await
            .unwrap();

        let run_id = Uuid::now_v7();
        repos
            .activity
            .create_run(run_id, "GH", "GithubIngestionHandler", "run_ingestion")
            .await
            .unwrap();
        repos
            .activity
            .update_run_progress(run_id, 10)
            .await
            .unwrap();

        let statuses = repos.activity.get_source_statuses().await.unwrap();
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].has_active_run);
        assert_eq!(statuses[0].active_run_items, Some(10));
    }
);

// ---------------------------------------------------------------------------
// Cancel & reset
// ---------------------------------------------------------------------------

define_repo_test!(cancel_active_runs, |repos, _pool| async move {
    let id1 = Uuid::now_v7();
    let id2 = Uuid::now_v7();
    repos
        .activity
        .create_run(id1, "github", "GithubIngestionHandler", "run_ingestion")
        .await
        .unwrap();
    repos
        .activity
        .create_run(id2, "github", "GithubIngestionHandler", "backfill")
        .await
        .unwrap();

    repos.activity.cancel_active_runs("github").await.unwrap();

    let r1 = repos.activity.get_run(id1).await.unwrap().unwrap();
    let r2 = repos.activity.get_run(id2).await.unwrap().unwrap();
    assert_eq!(r1.status, IngestionStatus::Cancelled);
    assert_eq!(r2.status, IngestionStatus::Cancelled);
});

define_repo_test!(reset_all_clears_everything, |repos, _pool| async move {
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
    repos
        .activity
        .create_run(
            Uuid::now_v7(),
            "github",
            "GithubIngestionHandler",
            "run_ingestion",
        )
        .await
        .unwrap();
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
});
