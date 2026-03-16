use ps_core::ingestion::{ContributionInput, IngestionContext};
use ps_core::models::Platform;
use tracing::{debug, info};
use uuid::Uuid;

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
        .batch_resolve_person_ids(Platform::Github, &usernames)
        .await?;

    let mut stored = 0usize;
    let mut skipped = 0usize;
    for item in items {
        let Some(person_id) = person_map.get(&item.platform_username).copied() else {
            skipped += 1;
            continue;
        };
        let id = Uuid::now_v7();

        ctx.repos
            .activity
            .upsert_contribution(id, Some(person_id), item)
            .await?;

        stored += 1;
    }

    if skipped > 0 {
        info!(
            source = ctx.source_config.name,
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

    info!(
        source = ctx.source_config.name,
        old_watermark = ?old_watermark,
        new_watermark = new_watermark,
        items_collected,
        "advanced watermark"
    );
    Ok(())
}
