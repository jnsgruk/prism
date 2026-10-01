use ps_core::{
    ingestion::{ContributionInput, IngestionContext},
    models::{ContributionType, Platform},
};
use ps_workers::infra::registry::create_source;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::common::{fixtures::create_person_with_identity, wiremock_helpers::SourceTestContext};

pub(super) fn date(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).unwrap()
}

pub(super) fn item(
    platform: Platform,
    kind: ContributionType,
    key: &str,
    actor: &str,
    created: &str,
) -> ContributionInput {
    ContributionInput {
        platform,
        contribution_type: kind,
        platform_id: key.into(),
        platform_username: actor.into(),
        title: Some(key.into()),
        url: Some(format!("https://example.com/{key}")),
        state: None,
        created_at: date(created),
        updated_at: Some(date(created)),
        closed_at: None,
        metrics: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: Some("source text".into()),
        state_history: None,
        enrichment_content: Some(serde_json::json!({"body":"source text"})),
    }
}

pub(super) async fn context(ctx: &SourceTestContext, platform: Platform) -> IngestionContext {
    let mut source = ctx.build_ingestion_ctx("selected source", platform.clone(), serde_json::json!({
        "base_url":ctx.mock_server.uri(), "orgs":["example"], "projects":["PROJ"], "fetch_likes":true,
    }), Some("token".into()), None, None).await;
    ctx.with_person_scope(
        &mut source,
        "selected",
        (platform == Platform::Jira).then_some("Cloud:Selected"),
        "2026-01-01",
        date("2026-09-30T12:00:00Z"),
    )
    .await;
    source
}

#[tokio::test]
async fn mixed_batches_write_only_selected_person_deduplicate_and_keep_global_coverage() {
    for (platform, kind, actor) in [
        (Platform::Github, ContributionType::PrReview, "selected"),
        (
            Platform::Jira,
            ContributionType::JiraTicket,
            "Cloud:Selected",
        ),
        (
            Platform::Discourse("ubuntu".into()),
            ContributionType::DiscoursePost,
            "selected",
        ),
    ] {
        let ctx = SourceTestContext::new().await;
        let source_ctx = context(&ctx, platform.clone()).await;
        let source = create_source(&platform).unwrap();
        let other = create_person_with_identity(&ctx.pool, "Other", &platform, "other").await;
        let existing = item(
            platform.clone(),
            kind,
            "other-existing",
            "other",
            "2026-06-01T00:00:00Z",
        );
        ctx.repos
            .activity
            .upsert_contribution(Uuid::now_v7(), Some(other), &existing)
            .await
            .unwrap();
        ctx.repos
            .activity
            .upsert_watermark("selected source", "global-checkpoint", 93)
            .await
            .unwrap();
        let before = sqlx::query!("SELECT watermark_value, last_successful_run, items_collected_last_run FROM activity.ingestion_watermarks WHERE source_name = 'selected source'").fetch_one(&ctx.pool).await.unwrap();
        let selected = item(
            platform.clone(),
            kind,
            "selected-key",
            actor,
            "2026-06-01T00:00:00Z",
        );
        let mut updated = selected.clone();
        updated.metrics = serde_json::json!({"score":4});
        let batch = vec![
            selected,
            existing,
            item(
                platform.clone(),
                kind,
                "unresolved",
                "outsider",
                "2026-06-01T00:00:00Z",
            ),
            item(
                platform.clone(),
                kind,
                "too-old",
                actor,
                "2025-12-31T23:59:59Z",
            ),
            item(
                platform.clone(),
                kind,
                "too-new",
                actor,
                "2026-10-01T00:00:00Z",
            ),
            item(
                Platform::Discourse("another".into()),
                ContributionType::DiscoursePost,
                "other-instance",
                actor,
                "2026-06-01T00:00:00Z",
            ),
            updated,
        ];
        assert_eq!(source.store_batch(&source_ctx, &batch).await.unwrap(), 1);
        let first = sqlx::query!("SELECT id, person_id, metrics FROM activity.contributions WHERE platform_id = 'selected-key'").fetch_one(&ctx.pool).await.unwrap();
        assert_eq!(
            first.person_id,
            source_ctx
                .request
                .as_ref()
                .unwrap()
                .scope
                .person_id()
                .map(|id| id.into_inner())
        );
        assert_eq!(first.metrics["score"], 4);
        assert_eq!(source.store_batch(&source_ctx, &batch).await.unwrap(), 1);
        source
            .advance_watermark(&source_ctx, "bad-scope-checkpoint", 500)
            .await
            .unwrap();
        let after = sqlx::query!("SELECT watermark_value, last_successful_run, items_collected_last_run FROM activity.ingestion_watermarks WHERE source_name = 'selected source'").fetch_one(&ctx.pool).await.unwrap();
        assert_eq!(before.watermark_value, after.watermark_value);
        assert_eq!(before.last_successful_run, after.last_successful_run);
        assert_eq!(
            before.items_collected_last_run,
            after.items_collected_last_run
        );
        let counts = sqlx::query!(r#"SELECT (SELECT count(*) FROM activity.contributions) AS "rows!", (SELECT count(*) FROM activity.contribution_changes) AS "changes!", (SELECT count(*) FROM reasoning.enrichment_queue) AS "queue!""#).fetch_one(&ctx.pool).await.unwrap();
        assert_eq!((counts.rows, counts.changes, counts.queue), (2, 1, 1));
        let second_id = sqlx::query_scalar!(
            "SELECT id FROM activity.contributions WHERE platform_id = 'selected-key'"
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
        assert_eq!(first.id, second_id);
        ctx.teardown().await;
    }
}

#[tokio::test]
async fn identity_removal_and_cancellation_fail_before_writes() {
    for cancel in [false, true] {
        let ctx = SourceTestContext::new().await;
        let source_ctx = context(&ctx, Platform::Github).await;
        let request = source_ctx.request.as_ref().unwrap();
        let source = create_source(&Platform::Github).unwrap();
        assert_eq!(source.store_batch(&source_ctx, &[]).await.unwrap(), 0);

        if cancel {
            ctx.repos
                .activity
                .request_pipeline_cancel(request.pipeline_id)
                .await
                .unwrap();
        } else {
            ctx.repos
                .org
                .remove_person_identity(
                    request.scope.person_id().unwrap(),
                    request.source.identity.as_ref().unwrap().identity_id,
                )
                .await
                .unwrap();
        }
        assert!(source.store_batch(&source_ctx, &[]).await.is_err());
        let filtered = item(
            Platform::Github,
            ContributionType::PrReview,
            "other-review",
            "outsider",
            "2026-06-01T00:00:00Z",
        );
        assert!(source.store_batch(&source_ctx, &[filtered]).await.is_err());
        assert!(
            create_source(&Platform::Github)
                .unwrap()
                .store_batch(
                    &source_ctx,
                    &[item(
                        Platform::Github,
                        ContributionType::PrReview,
                        "review",
                        "selected",
                        "2026-06-01T00:00:00Z"
                    )]
                )
                .await
                .is_err()
        );
        let rows = sqlx::query_scalar!("SELECT count(*) FROM activity.contributions")
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
        assert_eq!(rows, Some(0));
        ctx.teardown().await;
    }
}

#[tokio::test]
async fn conflicting_existing_attribution_never_reassigns_or_enqueues() {
    let ctx = SourceTestContext::new().await;
    let source_ctx = context(&ctx, Platform::Github).await;
    let other = create_person_with_identity(&ctx.pool, "Other", &Platform::Github, "other").await;
    let contribution = item(
        Platform::Github,
        ContributionType::PrReview,
        "review",
        "selected",
        "2026-06-01T00:00:00Z",
    );
    let id = Uuid::now_v7();
    ctx.repos
        .activity
        .upsert_contribution(id, Some(other), &contribution)
        .await
        .unwrap();
    assert!(
        create_source(&Platform::Github)
            .unwrap()
            .store_batch(&source_ctx, &[contribution])
            .await
            .is_err()
    );
    let saved = sqlx::query!("SELECT id, person_id FROM activity.contributions")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!((saved.id, saved.person_id), (id, Some(other)));
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM reasoning.enrichment_queue")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(0)
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn enqueue_failure_rolls_back_and_retry_recovers() {
    let ctx = SourceTestContext::new().await;
    let source_ctx = context(&ctx, Platform::Github).await;
    sqlx::query!(
        "ALTER TABLE reasoning.enrichment_queue ADD CONSTRAINT reject_test_queue CHECK (false)"
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    let source = create_source(&Platform::Github).unwrap();
    let items = vec![item(
        Platform::Github,
        ContributionType::PrReview,
        "review",
        "selected",
        "2026-06-01T00:00:00Z",
    )];
    assert!(source.store_batch(&source_ctx, &items).await.is_err());
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.contributions")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(0)
    );
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.contribution_changes")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(0)
    );
    sqlx::query!("ALTER TABLE reasoning.enrichment_queue DROP CONSTRAINT reject_test_queue")
        .execute(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(source.store_batch(&source_ctx, &items).await.unwrap(), 1);
    ctx.teardown().await;
}

#[tokio::test]
async fn authoritative_like_correction_keeps_id_and_all_affected_periods() {
    let ctx = SourceTestContext::new().await;
    let platform = Platform::Discourse("ubuntu".into());
    let source_ctx = context(&ctx, platform.clone()).await;
    let mut legacy = item(
        platform.clone(),
        ContributionType::DiscourseLike,
        "like-99-selected",
        "selected",
        "2026-03-31T23:59:00Z",
    );
    legacy.enrichment_content = None;
    let id = Uuid::now_v7();
    let person = source_ctx
        .request
        .as_ref()
        .unwrap()
        .scope
        .person_id()
        .unwrap()
        .into_inner();
    ctx.repos
        .activity
        .upsert_contribution(id, Some(person), &legacy)
        .await
        .unwrap();
    let mut action = legacy.clone();
    action.created_at = date("2026-04-08T00:00:00Z");
    action.updated_at = Some(action.created_at);
    action.metadata = serde_json::json!({"post_id":99,"topic_id":55,"username":"selected",
        "event_time_source":"discourse_user_action", "event_created_at":"2026-04-08T00:00:00Z",
        "user_action_type":1,"user_action_key":"1:55:99:selected:2026-04-08T00:00:00Z"});
    let source = create_source(&platform).unwrap();
    source
        .store_batch(&source_ctx, &[action.clone(), action.clone()])
        .await
        .unwrap();
    source
        .store_batch(&source_ctx, &[action.clone()])
        .await
        .unwrap();
    ctx.repos
        .activity
        .upsert_contribution(Uuid::now_v7(), Some(person), &legacy)
        .await
        .unwrap();
    let saved = sqlx::query!("SELECT id, created_at, metadata FROM activity.contributions")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(saved.id, id);
    assert_eq!(saved.created_at, action.created_at);
    assert_eq!(saved.metadata["event_time_source"], "discourse_user_action");
    let manifest = sqlx::query!("SELECT previous_created_at, current_created_at, affected_periods, contribution_id, source_id, run_id FROM activity.contribution_changes").fetch_one(&ctx.pool).await.unwrap();
    assert_eq!(
        (manifest.previous_created_at, manifest.current_created_at),
        (Some(legacy.created_at), action.created_at)
    );
    assert_eq!(
        (
            manifest.contribution_id,
            manifest.source_id,
            manifest.run_id
        ),
        (
            id,
            source_ctx.source_config.id.into_inner(),
            source_ctx.run_id
        )
    );
    let periods = manifest.affected_periods.as_array().unwrap();
    for month in ["2026-03-01", "2026-04-01"] {
        assert!(periods.contains(&serde_json::json!({"period_type":"month","period_start":month})));
    }
    assert_eq!(periods.len(), 6);
    // An older re-like page cannot reverse authoritative newer activity.
    let mut older = action.clone();
    older.created_at = date("2026-04-01T00:00:00Z");
    older.metadata["event_created_at"] = "2026-04-01T00:00:00Z".into();
    older.metadata["user_action_key"] = "1:55:99:selected:2026-04-01T00:00:00Z".into();
    assert_eq!(source.store_batch(&source_ctx, &[older]).await.unwrap(), 0);
    assert_eq!(
        sqlx::query_scalar!("SELECT created_at FROM activity.contributions")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        action.created_at
    );
    let evidence = sqlx::query!("SELECT metadata FROM activity.contributions")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(
        evidence.metadata["event_created_at"],
        "2026-04-08T00:00:00Z"
    );
    assert_eq!(
        evidence.metadata["user_action_key"],
        "1:55:99:selected:2026-04-08T00:00:00Z"
    );
    assert_eq!(evidence.metadata["post_id"], 99);
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.contribution_changes")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(1)
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn write_waits_for_identity_update_and_rechecks_after_lock() {
    let ctx = SourceTestContext::new().await;
    let source_ctx = context(&ctx, Platform::Github).await;
    let identity = source_ctx
        .request
        .as_ref()
        .unwrap()
        .source
        .identity
        .as_ref()
        .unwrap();
    let mut tx = ctx.pool.begin().await.unwrap();
    let person = identity.person_id.into_inner();
    sqlx::query!("SELECT id FROM org.people WHERE id = $1 FOR UPDATE", person)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query!(
        "UPDATE org.platform_identities SET platform_username = 'renamed' WHERE id = $1",
        identity.identity_id
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    let clone = source_ctx.clone();
    let task = tokio::spawn(async move {
        create_source(&Platform::Github)
            .unwrap()
            .store_batch(
                &clone,
                &[item(
                    Platform::Github,
                    ContributionType::PrReview,
                    "review",
                    "selected",
                    "2026-06-01T00:00:00Z",
                )],
            )
            .await
    });
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    tx.commit().await.unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.contributions")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(0)
    );
    ctx.teardown().await;
}
