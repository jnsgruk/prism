use ps_core::ingestion::{ContributionInput, IngestionContext};
use ps_core::models::Platform;
use tracing::{debug, info};
use uuid::Uuid;

use super::extract_instance;

pub(super) async fn store_batch_impl(
    ctx: &IngestionContext,
    items: &[ContributionInput],
) -> Result<usize, ps_core::Error> {
    if items.is_empty() {
        return Ok(0);
    }

    let instance = extract_instance(&ctx.source_config.name);
    let platform = Platform::Discourse(instance);

    // Collect unique (username, display_name) pairs for identity resolution.
    // Display names are extracted from post metadata where available.
    let mut seen = std::collections::HashSet::new();
    let users: Vec<(String, Option<String>)> = items
        .iter()
        .filter(|i| !i.platform_username.is_empty() && seen.insert(i.platform_username.clone()))
        .map(|i| {
            let display_name = i
                .metadata
                .get("display_name")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            (i.platform_username.clone(), display_name)
        })
        .collect();

    // Auto-create people + identities for new usernames.
    let person_map = ctx
        .repos
        .org
        .batch_ensure_identities(&platform, &users)
        .await?;

    let new_identities = users.len()
        - users
            .iter()
            .filter(|(u, _)| person_map.contains_key(u))
            .count();

    let mut ids = Vec::with_capacity(items.len());
    let mut person_ids = Vec::with_capacity(items.len());
    let mut resolved_items: Vec<&ContributionInput> = Vec::with_capacity(items.len());

    for item in items {
        let person_id = if item.platform_username.is_empty() {
            None
        } else {
            person_map.get(&item.platform_username).copied()
        };

        ids.push(Uuid::now_v7());
        person_ids.push(person_id);
        resolved_items.push(item);
    }

    let stored = resolved_items.len();
    if stored > 0 {
        ctx.repos
            .activity
            .bulk_upsert_contributions(&ids, &person_ids, &resolved_items)
            .await?;
    }

    // Backfill person_id on older Discourse contributions whose username
    // now has a known identity mapping (via metadata->>'username').
    let platform_str = platform.to_string();
    let backfilled = ctx
        .repos
        .activity
        .backfill_discourse_person_ids(&platform_str)
        .await?;

    if backfilled > 0 {
        info!(
            source = ctx.source_config.name,
            backfilled, "backfilled person_id on older Discourse contributions"
        );
    }

    if new_identities > 0 {
        info!(
            source = ctx.source_config.name,
            stored, new_identities, "stored Discourse batch — created new identities"
        );
    } else {
        debug!(stored, "stored Discourse batch");
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
        new_watermark,
        items_collected,
        "advanced Discourse watermark"
    );
    Ok(())
}
