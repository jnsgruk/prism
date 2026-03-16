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

    // Collect unique Jira account IDs for batch identity resolution.
    // Jira uses accountId (stored in platform_user_id) rather than username.
    let account_ids: Vec<String> = items
        .iter()
        .filter(|i| !i.platform_username.is_empty())
        .map(|i| i.platform_username.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Resolve by platform_user_id (Jira accountId) rather than platform_username.
    let person_map = ctx
        .repos
        .org
        .batch_resolve_by_user_id(&Platform::Jira, &account_ids)
        .await?;

    let mut ids = Vec::with_capacity(items.len());
    let mut person_ids = Vec::with_capacity(items.len());
    let mut resolved_items: Vec<&ContributionInput> = Vec::with_capacity(items.len());
    let mut unresolved_count = 0usize;

    for item in items {
        let person_id = if item.platform_username.is_empty() {
            None
        } else {
            person_map.get(&item.platform_username).copied()
        };

        ids.push(Uuid::now_v7());
        person_ids.push(person_id);
        resolved_items.push(item);

        if person_id.is_none() && !item.platform_username.is_empty() {
            unresolved_count += 1;
        }
    }

    let stored = resolved_items.len();
    if stored > 0 {
        ctx.repos
            .activity
            .bulk_upsert_contributions(&ids, &person_ids, &resolved_items)
            .await?;
    }

    if unresolved_count > 0 {
        info!(
            source = ctx.source_config.name,
            stored,
            unresolved_identities = unresolved_count,
            "stored batch — some Jira identities unresolved (upload Jira user CSV to map)"
        );
    } else {
        debug!(stored, "stored Jira batch");
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
        "advanced Jira watermark"
    );
    Ok(())
}
