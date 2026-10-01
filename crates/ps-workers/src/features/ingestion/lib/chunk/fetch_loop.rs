use ps_core::ingestion::IngestionContext;
use restate_sdk::prelude::*;
use uuid::Uuid;

use super::super::orchestration::fetch_batch;
use super::super::progress::{BatchAction, ProgressTracker, SerFetchResult, SkippedDiffAction};
use super::batch::{
    chunk_advance_watermark, chunk_retry_skipped_diffs, chunk_store_batch, compute_batch_action,
};
use crate::infra::run_lifecycle::{ensure_owned_active, journaled_value, terminal_err};

/// Best-effort progress update (not journaled).
macro_rules! chunk_update_progress {
    ($ing_ctx:expr, $run_id:expr, $global_items:expr, $tracker:expr, $cursor:expr, $batch:expr) => {{
        let rl = $batch.display_rate_limit.as_ref().or($batch.rate_limit.as_ref());
        let progress = $tracker.build_progress($cursor, rl);
        if let Err(e) = $ing_ctx
            .repos
            .activity
            .update_run_progress_detail($run_id, $global_items, &progress)
            .await
        {
            tracing::debug!(error = %e, "failed to update run progress");
        }
    }};
}

/// Best-effort progress update with rate-limit pause info (not journaled).
///
/// Like `chunk_update_progress!` but injects a `rate_limit_reset_at` ISO 8601
/// timestamp so the UI can show "Paused — resumes in Xm" while sleeping.
macro_rules! chunk_update_progress_with_pause {
    ($ing_ctx:expr, $run_id:expr, $global_items:expr, $tracker:expr, $cursor:expr, $batch:expr) => {{
        let rl = $batch.display_rate_limit.as_ref().or($batch.rate_limit.as_ref());
        let mut progress = $tracker.build_progress($cursor, rl);
        if let Some(ref rl) = $batch.rate_limit {
            if let Some(obj) = progress.as_object_mut() {
                let ts = rl
                    .reset_at
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                obj.insert("rate_limit_reset_at".into(), serde_json::json!(ts));
            }
        }
        if let Err(e) = $ing_ctx
            .repos
            .activity
            .update_run_progress_detail($run_id, $global_items, &progress)
            .await
        {
            tracing::debug!(error = %e, "failed to update run progress");
        }
    }};
}

/// Fetch-store loop with a batch limit and `Context<'_>` (service context).
///
/// Returns `(items_stored, final_cursor, is_complete)`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn chunk_fetch_store_loop(
    ctx: &Context<'_>,
    ing_ctx: &IngestionContext,
    run_id: Uuid,
    initial_cursor: &str,
    watermark_field: ps_core::models::WatermarkField,
    max_batches: usize,
    items_offset: i32,
    tracker: &mut (dyn ProgressTracker + Send),
) -> Result<(i32, String, bool), TerminalError> {
    let mut cursor = initial_cursor.to_string();
    let mut total_items = 0i32;
    let mut batches = 0u32;
    let mut last_progress_log = std::time::Instant::now();

    loop {
        if let Some(ref request) = ing_ctx.request {
            ensure_owned_active!(ctx, ing_ctx.repos, request.pipeline_id)?;
        }
        // Step 1: Fetch batch (journaled).
        let batch: SerFetchResult = {
            let ic = ing_ctx.clone();
            let cur = cursor.clone();
            journaled_value!(ctx, "fetch_batch", [ic, cur], {
                fetch_batch(&ic, &cur).await?
            })
        };

        // Best-effort rate limit warning.
        if let Some(ref rl) = batch.rate_limit
            && rl.remaining < 100
        {
            tracing::warn!(
                remaining = rl.remaining,
                limit = rl.limit,
                "rate limit pressure"
            );
        }

        // Step 2: Compute branching decision (pure function).
        let action = compute_batch_action(&batch, &cursor, watermark_field);

        // Step 3: Execute.
        match action {
            BatchAction::SleepForRateLimit {
                wait_secs,
                etag_cursor,
            } => {
                if let Some(ref latest) = etag_cursor {
                    cursor = latest.clone();
                }
                tracing::info!(
                    wait_secs,
                    "rate limit exhausted, sleeping durably before retry"
                );
                let global = items_offset + total_items;
                chunk_update_progress_with_pause!(
                    ing_ctx, run_id, global, tracker, &cursor, &batch
                );
                ctx.sleep(std::time::Duration::from_secs(wait_secs)).await?;
            }
            BatchAction::Process {
                item_count,
                has_watermark,
                next_cursor,
                etag_cursor,
                skipped_diffs,
            } => {
                if let Some(ref latest) = etag_cursor {
                    cursor = latest.clone();
                }

                if item_count > 0 {
                    let stored = chunk_store_batch(ctx, ing_ctx, &batch.items).await?;
                    total_items += stored;
                    tracker.count_batch(&batch.items, stored);
                    batches += 1;

                    if has_watermark {
                        chunk_advance_watermark(
                            ctx,
                            ing_ctx,
                            &cursor,
                            items_offset + total_items,
                            watermark_field,
                        )
                        .await?;
                    }

                    tracing::debug!(batch_stored = stored, total_items, "stored batch");
                }

                // Handle skipped diffs (GitHub REST rate limiting).
                match skipped_diffs {
                    SkippedDiffAction::None => {}
                    SkippedDiffAction::SleepThenRetry { wait_secs } => {
                        tracing::info!(
                            wait_secs,
                            skipped = batch.skipped_diffs.len(),
                            "sleeping for REST rate limit reset before retrying diffs"
                        );
                        let global = items_offset + total_items;
                        chunk_update_progress_with_pause!(
                            ing_ctx, run_id, global, tracker, &cursor, &batch
                        );
                        ctx.sleep(std::time::Duration::from_secs(wait_secs)).await?;
                        chunk_retry_skipped_diffs(ctx, ing_ctx, &batch.items, &batch.skipped_diffs)
                            .await?;
                    }
                    SkippedDiffAction::RetryOnly => {
                        chunk_retry_skipped_diffs(ctx, ing_ctx, &batch.items, &batch.skipped_diffs)
                            .await?;
                    }
                }

                let global = items_offset + total_items;
                chunk_update_progress!(ing_ctx, run_id, global, tracker, &cursor, &batch);

                if last_progress_log.elapsed() >= std::time::Duration::from_mins(1) {
                    tracing::info!(total_items, batches, "progress");
                    last_progress_log = std::time::Instant::now();
                }

                let Some(nc) = next_cursor else {
                    // End of data — return complete.
                    return Ok((total_items, cursor, true));
                };
                cursor = nc;

                // Check batch limit.
                if batches >= max_batches as u32 {
                    tracing::info!(batches, total_items, "chunk batch limit reached");
                    return Ok((total_items, cursor, false));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Context<'_> wrapper functions
// ---------------------------------------------------------------------------
