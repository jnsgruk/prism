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
