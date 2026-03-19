use ps_core::ingestion::{ContributionInput, IngestionContext};
use ps_core::models::Platform;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::handlers::ingestion_common;

pub(super) async fn store_batch_impl(
    ctx: &IngestionContext,
    items: &[ContributionInput],
) -> Result<usize, ps_core::Error> {
    if items.is_empty() {
        return Ok(0);
    }

    // Collect unique usernames for batch identity resolution.
    let usernames: Vec<String> = items
        .iter()
        .map(|i| i.platform_username.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let person_map = ctx
        .repos
        .org
        .batch_resolve_person_ids(&Platform::Github, &usernames)
        .await?;

    // Filter to items with resolved identities, collect for bulk upsert.
    let mut ids = Vec::with_capacity(items.len());
    let mut person_ids = Vec::with_capacity(items.len());
    let mut resolved_items: Vec<&ContributionInput> = Vec::with_capacity(items.len());
    let mut skipped = 0usize;

    for item in items {
        let Some(person_id) = person_map.get(&item.platform_username).copied() else {
            skipped += 1;
            continue;
        };
        ids.push(Uuid::now_v7());
        person_ids.push(Some(person_id));
        resolved_items.push(item);
    }

    let stored = resolved_items.len();
    if stored > 0 {
        let upserted = ctx
            .repos
            .activity
            .bulk_upsert_contributions(&ids, &person_ids, &resolved_items)
            .await?;

        // Enqueue enrichment content for AI processing.
        if let Err(e) =
            ingestion_common::enqueue_enrichments(&ctx.repos, &resolved_items, &upserted).await
        {
            warn!(source = ctx.source_config.name, error = %e, "failed to enqueue enrichments");
        }
    }

    if skipped > 0 {
        debug!(
            stored,
            skipped_identities = skipped,
            "stored batch with unresolved identities"
        );
    } else {
        debug!(stored, "stored batch");
    }

    Ok(stored)
}

pub(super) async fn advance_watermark_impl(
    ctx: &IngestionContext,
    new_watermark: &str,
    items_collected: i32,
) -> Result<(), ps_core::Error> {
    let old_watermark = ctx
        .repos
        .activity
        .get_watermark(&ctx.source_config.name)
        .await?;

    ctx.repos
        .activity
        .upsert_watermark(&ctx.source_config.name, new_watermark, items_collected)
        .await?;

    debug!(
        old_watermark = ?old_watermark,
        new_watermark = new_watermark,
        items_collected,
        "advanced watermark"
    );
    Ok(())
}
