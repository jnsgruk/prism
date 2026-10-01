use ps_core::{
    models::{ContributionType, EnrichmentType, Platform},
    repo::reasoning::UpsertEnrichmentParams,
};
use ps_workers::infra::registry::create_source;

use super::scoped_storage::{context, item};
use crate::common::wiremock_helpers::SourceTestContext;

#[tokio::test]
async fn scoped_changed_enrichment_retires_stale_ai_and_repeated_inputs_get_new_dirty_generations()
{
    let ctx = SourceTestContext::new().await;
    let source_ctx = context(&ctx, Platform::Github).await;
    let source = create_source(&Platform::Github).unwrap();
    let mut row = item(
        Platform::Github,
        ContributionType::PrReview,
        "review",
        "selected",
        "2026-02-12T12:00:00Z",
    );
    source
        .store_batch(&source_ctx, &[row.clone()])
        .await
        .unwrap();
    let id =
        sqlx::query_scalar!("SELECT id FROM activity.contributions WHERE platform_id='review'")
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    let pipeline_id = source_ctx.request.as_ref().unwrap().pipeline_id;
    for kind in [EnrichmentType::ReviewDepth, EnrichmentType::Sentiment] {
        ctx.repos
            .reasoning
            .upsert_enrichment(&UpsertEnrichmentParams {
                contribution_id: id,
                enrichment_type: kind,
                value: &serde_json::json!({"score":4,"sentiment":"constructive"}),
                model_name: "fixture",
                confidence: Some(0.9),
                input_hash: None,
                input_preview: None,
                source_content_hash: row
                    .enrichment_content
                    .as_ref()
                    .map(ps_core::repo::reasoning::content_hash)
                    .as_deref(),
            })
            .await
            .unwrap();
    }
    ctx.repos
        .reasoning
        .delete_fully_enriched_entries()
        .await
        .unwrap();
    // A metric-only change should preserve AI whose input text has not changed.
    row.metrics = serde_json::json!({"score":3});
    source
        .store_batch(&source_ctx, &[row.clone()])
        .await
        .unwrap();
    assert_eq!(
        ctx.repos
            .reasoning
            .get_enrichments_for_contribution(id)
            .await
            .unwrap()
            .len(),
        2
    );
    ctx.repos
        .reasoning
        .delete_fully_enriched_entries()
        .await
        .unwrap();
    let work = ctx
        .repos
        .activity
        .pending_snapshot_invalidations(Some(pipeline_id), false, 8)
        .await
        .unwrap();
    let old_ids: Vec<_> = work
        .iter()
        .flat_map(|period| period.invalidation_ids.iter().copied())
        .collect();
    ctx.repos
        .activity
        .acknowledge_snapshot_invalidations(&old_ids, false)
        .await
        .unwrap();
    ctx.repos
        .activity
        .acknowledge_snapshot_invalidations(&old_ids, true)
        .await
        .unwrap();
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(pipeline_id)
            .await
            .unwrap(),
        (0, 0)
    );

    let old_content = row.enrichment_content.clone();
    row.enrichment_content = Some(serde_json::json!({"body":"corrected source text"}));
    source
        .store_batch(&source_ctx, &[row.clone()])
        .await
        .unwrap();
    assert!(
        ctx.repos
            .reasoning
            .get_enrichments_for_contribution(id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ctx.repos
            .reasoning
            .find_queued_for_enrichment(EnrichmentType::ReviewDepth, 10)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(pipeline_id)
            .await
            .unwrap(),
        (3, 3)
    );

    // A -> B -> A is a real correction, rather than replaying the first A.
    row.enrichment_content = old_content;
    source
        .store_batch(&source_ctx, &[row.clone()])
        .await
        .unwrap();
    let work = ctx
        .repos
        .activity
        .pending_snapshot_invalidations(Some(pipeline_id), false, 8)
        .await
        .unwrap();
    assert!(
        work.iter()
            .flat_map(|period| period.invalidation_ids.iter())
            .all(|id| !old_ids.contains(id))
    );
    ctx.repos
        .activity
        .acknowledge_snapshot_invalidations(&old_ids, false)
        .await
        .unwrap();
    ctx.repos
        .activity
        .acknowledge_snapshot_invalidations(&old_ids, true)
        .await
        .unwrap();
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(pipeline_id)
            .await
            .unwrap(),
        (6, 6)
    );
    // Replaying unchanged A neither retires results nor adds invalidations.
    source.store_batch(&source_ctx, &[row]).await.unwrap();
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(pipeline_id)
            .await
            .unwrap(),
        (6, 6)
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn scoped_input_change_rejects_in_flight_ai_and_preserves_replacement_work() {
    use ps_reasoning::{
        features::enrichment::process_queued_enrichment_batch, routing::TaskRouter,
    };
    use std::{sync::Arc, time::Duration};
    use wiremock::{Mock, ResponseTemplate, matchers::method};

    let ctx = SourceTestContext::new().await;
    let source_ctx = context(&ctx, Platform::Github).await;
    let source = create_source(&Platform::Github).unwrap();
    let mut row = item(
        Platform::Github,
        ContributionType::PrReview,
        "racing-review",
        "selected",
        "2026-02-12T12:00:00Z",
    );
    source
        .store_batch(&source_ctx, &[row.clone()])
        .await
        .unwrap();
    let captured = ctx
        .repos
        .reasoning
        .find_queued_for_enrichment(EnrichmentType::ReviewDepth, 10)
        .await
        .unwrap();
    let id = captured[0].contribution_id;
    let old_hash = captured[0].content_hash.clone();
    let mut router = TaskRouter::new(ps_reasoning::types::AiConfig::default());
    let key = "fixture-only";
    let client = ps_reasoning::rig::providers::gemini::Client::builder()
        .api_key(key)
        .base_url(ctx.mock_server.uri())
        .build()
        .unwrap();
    router.set_google_client(client, key);
    let router = Arc::new(router);
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)
        .set_delay(Duration::from_secs(2))
        .set_body_json(serde_json::json!({
            "candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"submit","args":{"score":4,"sentiment":"constructive","rationale":"fixture","confidence":0.9}}}]},"finishReason":"STOP"}],
            "usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":5,"totalTokenCount":15}
        }))).mount(&ctx.mock_server).await;
    let repos = ctx.repos.clone();
    let worker_router = router.clone();
    let old_batch = tokio::spawn(async move {
        process_queued_enrichment_batch(
            &worker_router,
            &repos.reasoning,
            EnrichmentType::ReviewDepth,
            &captured,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !ctx
                .mock_server
                .received_requests()
                .await
                .unwrap()
                .is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // The old AI request has started but its response is still delayed.
    row.enrichment_content =
        Some(serde_json::json!({"body":"corrected source input after processing started"}));
    source
        .store_batch(&source_ctx, &[row.clone()])
        .await
        .unwrap();
    let new_hash = ps_core::repo::reasoning::content_hash(row.enrichment_content.as_ref().unwrap());
    assert_ne!(old_hash, new_hash);
    let stale = old_batch.await.unwrap();
    assert_eq!(stale.errors, 0, "the provider returned a valid response");
    assert!(stale.total_usage.input_tokens > 0);
    assert_eq!(
        stale.processed, 0,
        "stale AI results must not count as committed"
    );
    assert!(stale.successful_contribution_ids.is_empty());
    assert!(
        ctx.repos
            .reasoning
            .get_enrichments_for_contribution(id)
            .await
            .unwrap()
            .is_empty()
    );
    // Unknown provenance from a legacy writer cannot acknowledge new input.
    ctx.repos
        .reasoning
        .upsert_enrichment(&UpsertEnrichmentParams {
            contribution_id: id,
            enrichment_type: EnrichmentType::ReviewDepth,
            value: &serde_json::json!({"score":1}),
            model_name: "legacy-worker",
            confidence: Some(0.5),
            input_hash: Some(&old_hash),
            input_preview: Some("old input"),
            source_content_hash: None,
        })
        .await
        .unwrap();
    assert_eq!(
        ctx.repos
            .reasoning
            .delete_fully_enriched_entries()
            .await
            .unwrap(),
        0
    );
    let pending = ctx
        .repos
        .reasoning
        .find_queued_for_enrichment(EnrichmentType::ReviewDepth, 10)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].content_hash, new_hash);
    let pipeline_id = source_ctx.request.as_ref().unwrap().pipeline_id;
    let work = ctx
        .repos
        .activity
        .pending_snapshot_invalidations(Some(pipeline_id), false, 8)
        .await
        .unwrap();
    let generations: Vec<_> = work
        .iter()
        .flat_map(|period| period.invalidation_ids.iter().copied())
        .collect();
    ctx.repos
        .activity
        .acknowledge_snapshot_invalidations(&generations, false)
        .await
        .unwrap();
    assert!(
        ctx.repos
            .activity
            .pending_snapshot_invalidations(Some(pipeline_id), true, 8)
            .await
            .unwrap()
            .is_empty(),
        "stale completion cannot unblock historical insights"
    );

    for kind in [EnrichmentType::ReviewDepth, EnrichmentType::Sentiment] {
        let pending = ctx
            .repos
            .reasoning
            .find_queued_for_enrichment(kind, 10)
            .await
            .unwrap();
        let fresh =
            process_queued_enrichment_batch(&router, &ctx.repos.reasoning, kind, &pending).await;
        assert_eq!((fresh.processed, fresh.errors), (1, 0));
    }
    assert_eq!(
        ctx.repos
            .reasoning
            .delete_fully_enriched_entries()
            .await
            .unwrap(),
        1
    );
    assert!(
        !ctx.repos
            .activity
            .pending_snapshot_invalidations(Some(pipeline_id), true, 8)
            .await
            .unwrap()
            .is_empty()
    );
    let hashes = sqlx::query_scalar!(
        "SELECT source_content_hash FROM reasoning.enrichments WHERE contribution_id=$1",
        id
    )
    .fetch_all(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(hashes, vec![Some(new_hash.clone()), Some(new_hash)]);
    ctx.teardown().await;
}

#[tokio::test]
async fn scoped_cleanup_waits_for_pr_eligibility_change_without_deleting_replacement() {
    use std::time::Duration;
    let ctx = SourceTestContext::new().await;
    let source_ctx = context(&ctx, Platform::Github).await;
    let source = create_source(&Platform::Github).unwrap();
    let mut row = item(
        Platform::Github,
        ContributionType::PullRequest,
        "growing-pr",
        "selected",
        "2026-02-12T12:00:00Z",
    );
    row.metrics = serde_json::json!({"additions":1,"deletions":0});
    source.store_batch(&source_ctx, &[row]).await.unwrap();
    let queued = ctx
        .repos
        .reasoning
        .find_queued_for_enrichment(EnrichmentType::Significance, 10)
        .await
        .unwrap();
    let id = queued[0].contribution_id;
    let content = serde_json::json!({"title":"growing PR","additions":100,"deletions":0});
    let hash = ps_core::repo::reasoning::content_hash(&content);
    // Hold the committed old snapshot visible while a scoped-style transaction
    // replaces its contribution metrics and queue, then let cleanup encounter it.
    let mut update = ctx.pool.begin().await.unwrap();
    sqlx::query!(
        "UPDATE activity.contributions SET metrics=$2 WHERE id=$1",
        id,
        serde_json::json!({"additions":100,"deletions":0})
    )
    .execute(&mut *update)
    .await
    .unwrap();
    sqlx::query!(
        "UPDATE reasoning.enrichment_queue SET content=$2,content_hash=$3 WHERE contribution_id=$1",
        id,
        content,
        hash
    )
    .execute(&mut *update)
    .await
    .unwrap();
    let repos = ctx.repos.clone();
    let cleanup = tokio::spawn(async move {
        repos
            .reasoning
            .delete_fully_enriched_entries()
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting = sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!" FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND state='active' AND pid<>pg_backend_pid()"#).fetch_one(&ctx.pool).await.unwrap();
            if waiting > 0 { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    update.commit().await.unwrap();
    assert_eq!(
        cleanup.await.unwrap(),
        0,
        "old size eligibility cannot delete new enrichable work"
    );
    let pending = ctx
        .repos
        .reasoning
        .find_queued_for_enrichment(EnrichmentType::Significance, 10)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].content_hash, hash);
    assert_eq!(pending[0].content, content);
    ctx.teardown().await;
}
