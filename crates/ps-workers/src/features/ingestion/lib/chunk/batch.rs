use ps_core::ingestion::{ContributionInput, IngestionContext};
use restate_sdk::prelude::*;

use super::super::finalise::{diff_rate_limit_sleep_duration, extract_watermark};
use super::super::progress::{BatchAction, SerFetchResult, SkippedDiffAction};
use crate::infra::run_lifecycle::{journaled, journaled_value, terminal_err};

/// Store a batch inside a journaled `ctx.run()` (service context variant).
pub(super) async fn chunk_store_batch(
    ctx: &Context<'_>,
    ing_ctx: &IngestionContext,
    items: &[ContributionInput],
    fingerprint_error: Option<String>,
) -> Result<i32, TerminalError> {
    let ic = ing_ctx.clone();
    let items = items.to_vec();

    #[allow(clippy::cast_possible_wrap)]
    Ok(journaled_value!(
        ctx,
        "store_batch",
        [ic, items, fingerprint_error],
        {
            // A completed store replays its recorded result. Validate a refetched
            // page only when this write has not already committed in the journal.
            if let Some(error) = fingerprint_error {
                return Err(TerminalError::new(error).into());
            }
            let src = crate::infra::registry::create_source(&ic.source_config.source_type)
                .ok_or_else(|| TerminalError::new("source unavailable"))?;
            src.store_batch(&ic, &items)
                .await
                .map_err(|error| store_error(&ic, error))? as i32
        }
    ))
}

fn store_error(ic: &IngestionContext, error: ps_core::Error) -> HandlerError {
    tracing::warn!(error = %error, source = ic.source_config.name, "batch storage failed");
    match error {
        ps_core::Error::Database(_) if !ic.advances_global_watermark() => HandlerError::from(
            std::io::Error::other("scoped storage database operation failed; retrying"),
        ),
        ps_core::Error::Database(_) => TerminalError::new("store failed: internal error").into(),
        error => terminal_err("store failed")(error).into(),
    }
}

/// Advance the watermark inside a journaled `ctx.run()` (service context variant).
pub(super) async fn chunk_advance_watermark(
    ctx: &Context<'_>,
    ing_ctx: &IngestionContext,
    cursor: &str,
    total_items: i32,
    watermark_field: ps_core::models::WatermarkField,
) -> Result<(), TerminalError> {
    if !ing_ctx.advances_global_watermark() {
        return Ok(());
    }
    let ic = ing_ctx.clone();
    let wm = cursor.to_string();

    journaled!(ctx, "advance_watermark", [ic, wm], {
        let src = crate::infra::registry::create_source(&ic.source_config.source_type)
            .ok_or_else(|| TerminalError::new("source unavailable"))?;
        let watermark = extract_watermark(&wm, watermark_field).unwrap_or_default();
        src.advance_watermark(&ic, &watermark, total_items)
            .await
            .map_err(terminal_err("advance failed"))?;
    });

    Ok(())
}

/// Retry skipped PR diffs (service context variant).
///
/// Re-fetches diffs that were skipped due to REST rate limiting, then
/// re-enqueues affected contributions for enrichment with updated content.
pub(super) async fn chunk_retry_skipped_diffs(
    ctx: &Context<'_>,
    ing_ctx: &IngestionContext,
    original_items: &[ContributionInput],
    skipped: &[ps_core::ingestion::SkippedDiff],
) -> Result<(), TerminalError> {
    let token = ing_ctx.token.as_deref().unwrap_or("");
    if token.is_empty() {
        return Ok(());
    }

    let api_base = ing_ctx
        .source_config
        .settings
        .get("base_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.github.com");

    let client = crate::features::ingestion::github::client::GitHubClient::new(
        ing_ctx.http_client.clone(),
        api_base,
        token,
    );

    let mut updated_items: Vec<(String, serde_json::Value)> = Vec::new();
    let mut scoped_items = Vec::new();

    for sd in skipped {
        let Some(original) = original_items.get(sd.item_index) else {
            continue;
        };
        if let Some(request) = ing_ctx
            .person_request()
            .map_err(terminal_err("invalid person snapshot"))?
            && !request
                .eligible_contribution(original)
                .map_err(terminal_err("invalid diff candidate"))?
        {
            continue;
        }
        match crate::features::ingestion::github::source::fetch::fetch_single_pr_diff(
            &client,
            &sd.owner,
            &sd.repo,
            sd.pr_number,
        )
        .await
        {
            crate::features::ingestion::github::source::fetch::DiffFetchResult::Ok(diff_text) => {
                if let Some(enrichment) = &original.enrichment_content {
                    let mut content = enrichment.clone();
                    if let Some(obj) = content.as_object_mut() {
                        obj.insert("diff".to_string(), serde_json::Value::String(diff_text));
                    }
                    if ing_ctx.advances_global_watermark() {
                        updated_items.push((original.platform_id.to_string(), content));
                    } else {
                        let mut updated = original.clone();
                        updated.enrichment_content = Some(content);
                        scoped_items.push(updated);
                    }
                }
            }
            crate::features::ingestion::github::source::fetch::DiffFetchResult::RateLimited(_) => {
                tracing::warn!(
                    remaining = skipped.len() - updated_items.len() - scoped_items.len(),
                    "diff retry also hit rate limit, skipping remaining"
                );
                break;
            }
            crate::features::ingestion::github::source::fetch::DiffFetchResult::Failed => {}
        }
    }

    if !ing_ctx.advances_global_watermark() {
        chunk_store_batch(ctx, ing_ctx, &scoped_items, None).await?;
        return Ok(());
    }

    if updated_items.is_empty() {
        return Ok(());
    }

    let repos = ing_ctx.repos.clone();
    let items_for_closure = updated_items.clone();

    let result = ctx
        .run(|| {
            let repos = repos.clone();
            let items = items_for_closure.clone();
            async move {
                let platform_ids: Vec<String> = items.iter().map(|(pid, _)| pid.clone()).collect();
                let id_pairs = repos
                    .activity
                    .get_contribution_ids_by_platform_ids("github", &platform_ids)
                    .await
                    .map_err(terminal_err("db error"))?;

                let content_by_pid: std::collections::HashMap<&str, &serde_json::Value> = items
                    .iter()
                    .map(|(pid, content)| (pid.as_str(), content))
                    .collect();

                let entries: Vec<ps_core::repo::reasoning::EnrichmentQueueEntry> = id_pairs
                    .iter()
                    .filter_map(|(contribution_id, platform_id)| {
                        let content = content_by_pid.get(platform_id.as_str())?;
                        Some(ps_core::repo::reasoning::EnrichmentQueueEntry {
                            contribution_id: *contribution_id,
                            content: (*content).clone(),
                            content_hash: ps_core::repo::reasoning::content_hash(content),
                        })
                    })
                    .collect();

                if !entries.is_empty() {
                    repos
                        .reasoning
                        .bulk_enqueue_enrichments(&entries)
                        .await
                        .map_err(terminal_err("enqueue error"))?;
                }

                #[allow(clippy::cast_possible_wrap)]
                Ok(Json::from(entries.len() as i32))
            }
        })
        .name("retry_diff_enqueue")
        .await;

    match result {
        Ok(count) => {
            let re_enqueued = count.into_inner();
            tracing::info!(
                fetched = updated_items.len(),
                re_enqueued,
                "retried skipped diffs"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to re-enqueue retried diffs");
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute the branching decision from a fetch result (pure function).
///
/// Duplicated from `orchestration::compute_batch_action` because that function
/// is private. The logic must stay in sync.
pub(super) fn compute_batch_action(
    batch: &SerFetchResult,
    cursor: &str,
    watermark_field: ps_core::models::WatermarkField,
) -> BatchAction {
    let etag_cursor = batch.etag.clone();

    if batch.items.is_empty()
        && batch.next_cursor.is_some()
        && let Some(ref rl) = batch.rate_limit
        && rl.remaining == 0
    {
        let wait = diff_rate_limit_sleep_duration(rl);
        return BatchAction::SleepForRateLimit {
            wait_secs: wait.as_secs(),
            etag_cursor: etag_cursor.or_else(|| batch.next_cursor.clone()),
        };
    }

    let effective_cursor = etag_cursor.as_deref().unwrap_or(cursor);
    let has_watermark =
        extract_watermark(effective_cursor, watermark_field).is_some_and(|wm| !wm.is_empty());

    let skipped_diffs = if batch.skipped_diffs.is_empty() {
        SkippedDiffAction::None
    } else if let Some(ref rl) = batch.rate_limit
        && rl.remaining == 0
    {
        let wait = diff_rate_limit_sleep_duration(rl);
        SkippedDiffAction::SleepThenRetry {
            wait_secs: wait.as_secs(),
        }
    } else {
        SkippedDiffAction::RetryOnly
    };

    BatchAction::Process {
        item_count: batch.items.len(),
        has_watermark,
        next_cursor: batch.next_cursor.clone(),
        etag_cursor,
        skipped_diffs,
    }
}
