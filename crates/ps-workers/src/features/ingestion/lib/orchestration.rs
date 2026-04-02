use ps_core::ingestion::{ContributionInput, IngestionContext};
use ps_core::models::SourceConfig;
use restate_sdk::prelude::*;

use crate::infra::run_lifecycle::{
    complete_run, complete_run_with_warnings, create_run, fail_run, journaled, journaled_value,
    terminal_err,
};
use crate::infra::{
    SharedState, decrypt_optional_secret, decrypt_required_secret, load_source_config,
};

use super::finalise::{
    diff_rate_limit_sleep_duration, extract_failed_items, extract_watermark, finalise_run,
    retry_skipped_diffs,
};
use super::progress::{IngestionSpec, ProgressTracker, SerFetchResult};

/// Best-effort progress update (not journaled).
macro_rules! update_progress {
    ($ing_ctx:expr, $run_id:expr, $total_items:expr, $tracker:expr, $cursor:expr, $batch:expr) => {{
        let progress = $tracker.build_progress($cursor, $batch.rate_limit.as_ref());
        if let Err(e) = $ing_ctx
            .repos
            .activity
            .update_run_progress_detail($run_id, $total_items, &progress)
            .await
        {
            tracing::debug!(error = %e, "failed to update run progress");
        }
    }};
}

/// Load a source config inside a Restate `ctx.run()` closure.
pub async fn load_ingestion_source_config(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    source_name: &str,
) -> Result<SourceConfig, TerminalError> {
    let repos = repos.clone();
    let name = source_name.to_string();
    Ok(journaled_value!(ctx, "load_config", [repos, name], {
        load_source_config(&repos, &name)
            .await
            .map_err(TerminalError::new)?
    }))
}

/// Shared ingestion orchestration used by all three ingestion handlers.
///
/// Handles: source creation, run creation, secret decryption, planning,
/// watermark override, fetch/store loop, run finalisation.
///
/// The caller provides a `ProgressTracker` for source-specific progress
/// reporting, and a closure to fire downstream triggers after completion.
#[allow(clippy::too_many_arguments)]
pub async fn execute_ingestion(
    ctx: &ObjectContext<'_>,
    state: &SharedState,
    spec: &IngestionSpec,
    source_name: &str,
    config: &SourceConfig,
    override_watermark: Option<String>,
    tracker: &mut (dyn ProgressTracker + Send),
    trigger_downstream: impl FnOnce(&ObjectContext<'_>),
) -> Result<(), TerminalError> {
    let start = std::time::Instant::now();

    let source = crate::infra::registry::create_source(&config.source_type).ok_or_else(|| {
        TerminalError::new(format!("unsupported source type: {}", config.source_type))
    })?;

    let method = if override_watermark.is_some() {
        "backfill"
    } else {
        "run_ingestion"
    };
    let run_id =
        create_ingestion_run(ctx, &state.repos, source_name, spec.handler_name, method).await?;

    // Store the Restate invocation ID so reconcile_stale_runs can verify
    // this invocation is still alive instead of cancelling it as orphaned.
    let invocation_id = ctx.invocation_id().to_string();
    let repos = state.repos.clone();
    let sn = source_name.to_string();
    journaled!(ctx, "set_invocation_id", [repos, sn, invocation_id], {
        repos
            .activity
            .set_current_invocation_id(&sn, &invocation_id)
            .await
            .map_err(terminal_err("failed to set invocation ID"))?;
    });

    let span = tracing::info_span!(
        "handler",
        handler = spec.handler_name,
        source = source_name,
        run_id = %run_id,
    );
    let _guard = span.enter();

    tracing::info!("starting ingestion");

    // Decrypt secrets outside ctx.run() to avoid journaling plaintext
    let token = match (spec.token_key, spec.token_required) {
        (Some(key), true) => Some(decrypt_required_secret(state, config.id, key).await?),
        (Some(key), false) => decrypt_optional_secret(state, config.id, key).await?,
        (None, _) => None,
    };
    let email = match spec.email_key {
        Some(key) => decrypt_optional_secret(state, config.id, key).await?,
        None => None,
    };
    let api_username = match spec.api_username_key {
        Some(key) => decrypt_optional_secret(state, config.id, key).await?,
        None => None,
    };

    let ing_ctx = build_ingestion_context(state, config, token, email, api_username);

    let mut plan = match source.plan(&ing_ctx).await {
        Ok(p) => p,
        Err(e) => {
            fail_ingestion_run(ctx, &state.repos, run_id, source_name, &e.to_string()).await;
            return Err(TerminalError::new(format!("plan failed: {e}")));
        }
    };

    if let Some(ref wm) = override_watermark {
        plan.watermark = Some(wm.clone());
    }

    tracing::debug!(watermark = ?plan.watermark, "ingestion plan ready");

    let initial_cursor = source.initial_cursor(&ing_ctx, &plan);
    let watermark_field = source.watermark_field();

    let result = fetch_store_loop(
        ctx,
        &ing_ctx,
        run_id,
        source_name,
        &initial_cursor,
        watermark_field,
        tracker,
    )
    .await;

    let (total_items, final_cursor) = match result {
        Ok(v) => v,
        Err(e) => {
            let msg = e.to_string();
            fail_ingestion_run(ctx, &state.repos, run_id, source_name, &msg).await;
            return Err(TerminalError::new(format!("ingestion failed: {msg}")));
        }
    };

    let failed_items = extract_failed_items(&final_cursor);
    finalise_run(
        ctx,
        &state.repos,
        &ing_ctx,
        run_id,
        source_name,
        total_items,
        &failed_items,
        spec.item_noun,
        &final_cursor,
        source.watermark_field(),
    )
    .await?;

    if total_items > 0 {
        tracing::debug!("triggering downstream handlers");
        trigger_downstream(ctx);
    }

    tracing::info!(
        total_items,
        duration_secs = start.elapsed().as_secs(),
        "complete"
    );
    Ok(())
}

/// Construct an `IngestionContext` from shared state and config.
pub fn build_ingestion_context(
    state: &SharedState,
    config: &SourceConfig,
    token: Option<String>,
    email: Option<String>,
    api_username: Option<String>,
) -> IngestionContext {
    IngestionContext {
        repos: state.repos.clone(),
        source_config: config.clone(),
        http_client: state.http_client.clone(),
        token,
        email,
        api_username,
    }
}

/// Create an ingestion run record inside a Restate `ctx.run()` closure.
pub(super) async fn create_ingestion_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    source_name: &str,
    handler_name: &str,
    method: &str,
) -> Result<uuid::Uuid, TerminalError> {
    create_run!(ctx, repos, source_name, handler_name, method)
}

/// Mark a run as complete inside a Restate `ctx.run()` closure.
pub(super) async fn complete_ingestion_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run_id: uuid::Uuid,
    source_name: &str,
    items_collected: i32,
) {
    complete_run!(ctx, repos, run_id, source_name, items_collected);
}

/// Mark a run as completed with warnings inside a Restate `ctx.run()` closure.
pub(super) async fn complete_ingestion_run_with_warnings(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run_id: uuid::Uuid,
    source_name: &str,
    items_collected: i32,
    error_summary: &str,
    metadata: serde_json::Value,
) {
    complete_run_with_warnings!(
        ctx,
        repos,
        run_id,
        source_name,
        items_collected,
        error_summary,
        metadata
    );
}

/// Mark a run as failed inside a Restate `ctx.run()` closure.
pub(super) async fn fail_ingestion_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run_id: uuid::Uuid,
    source_name: &str,
    error_msg: &str,
) {
    fail_run!(ctx, repos, run_id, source_name, error_msg);
}

/// Fetch a batch with panic isolation. Panics in source `fetch_batch()`
/// implementations are caught and converted to `TerminalError`.
async fn fetch_batch_catch_panic(
    ing_ctx: &IngestionContext,
    cursor: &str,
) -> Result<Result<SerFetchResult, TerminalError>, TerminalError> {
    use futures::FutureExt as _;
    use std::panic::AssertUnwindSafe;

    AssertUnwindSafe(fetch_batch(ing_ctx, cursor))
        .catch_unwind()
        .await
        .map_err(|panic| {
            let msg = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            TerminalError::new(format!("fetch panicked: {msg}"))
        })
}

/// Fetch a batch — NOT journaled (external API call, large response, idempotent on replay).
pub async fn fetch_batch(
    ing_ctx: &IngestionContext,
    cursor: &str,
) -> Result<SerFetchResult, TerminalError> {
    let src = crate::infra::registry::create_source(&ing_ctx.source_config.source_type)
        .ok_or_else(|| TerminalError::new("source unavailable"))?;
    let result = src
        .fetch_batch(ing_ctx, cursor)
        .await
        .map_err(terminal_err("fetch failed"))?;

    Ok(SerFetchResult {
        items: result.items,
        next_cursor: result.next_cursor,
        rate_limit: result.rate_limit,
        etag: result.etag,
        skipped_diffs: result.skipped_diffs,
    })
}

/// Store a batch inside a Restate `ctx.run()` closure.
pub async fn store_batch(
    ctx: &ObjectContext<'_>,
    ing_ctx: &IngestionContext,
    items: &[ContributionInput],
) -> Result<i32, TerminalError> {
    let ic = ing_ctx.clone();
    let items = items.to_vec();

    #[allow(clippy::cast_possible_wrap)]
    Ok(journaled_value!(ctx, "store_batch", [ic, items], {
        let src = crate::infra::registry::create_source(&ic.source_config.source_type)
            .ok_or_else(|| TerminalError::new("source unavailable"))?;
        src.store_batch(&ic, &items)
            .await
            .map_err(terminal_err("store failed"))? as i32
    }))
}

/// Advance the watermark inside a Restate `ctx.run()` closure.
///
/// `watermark_field` is the JSON field to extract from the cursor
/// (e.g. `"max_updated_at"` or `"max_bumped_at"`).
pub async fn advance_watermark(
    ctx: &ObjectContext<'_>,
    ing_ctx: &IngestionContext,
    cursor: &str,
    total_items: i32,
    watermark_field: &str,
) -> Result<(), TerminalError> {
    let ic = ing_ctx.clone();
    let wm = cursor.to_string();
    let field = watermark_field.to_string();

    journaled!(ctx, "advance_watermark", [ic, wm, field], {
        let src = crate::infra::registry::create_source(&ic.source_config.source_type)
            .ok_or_else(|| TerminalError::new("source unavailable"))?;
        let watermark = extract_watermark(&wm, &field).unwrap_or_default();
        src.advance_watermark(&ic, &watermark, total_items)
            .await
            .map_err(terminal_err("advance failed"))?;
    });

    Ok(())
}

/// Unified fetch-store loop shared by all three ingestion handlers.
///
/// Fetches batches from the source, stores them via journaled `ctx.run()`,
/// updates progress, and returns `(total_items, final_cursor)`.
pub async fn fetch_store_loop(
    ctx: &ObjectContext<'_>,
    ing_ctx: &IngestionContext,
    run_id: uuid::Uuid,
    _source_name: &str,
    initial_cursor: &str,
    watermark_field: &str,
    tracker: &mut (dyn ProgressTracker + Send),
) -> Result<(i32, String), TerminalError> {
    let mut cursor = initial_cursor.to_string();
    let mut total_items = 0i32;
    let mut batches = 0u32;
    let mut last_progress_log = std::time::Instant::now();

    loop {
        let batch = fetch_batch_catch_panic(ing_ctx, &cursor).await??;

        // Update cursor from etag if present (Jira/Discourse pattern),
        // which carries watermark state even when next_cursor is None.
        if let Some(ref latest) = batch.etag {
            cursor = latest.clone();
        }

        // Rate limit warning
        if let Some(ref rl) = batch.rate_limit
            && rl.remaining < 100
        {
            tracing::warn!(
                remaining = rl.remaining,
                limit = rl.limit,
                "rate limit pressure"
            );
        }

        // If rate limit is exhausted and no items returned, sleep durably
        // until reset then retry the same cursor (GraphQL rate limit case).
        if batch.items.is_empty()
            && batch.next_cursor.is_some()
            && let Some(ref rl) = batch.rate_limit
            && rl.remaining == 0
        {
            let wait = diff_rate_limit_sleep_duration(rl);
            tracing::info!(
                wait_secs = wait.as_secs(),
                "rate limit exhausted, sleeping durably before retry"
            );

            // Update progress before sleeping so the UI reflects the
            // exhausted rate limit rather than showing a stale snapshot.
            update_progress!(ing_ctx, run_id, total_items, tracker, &cursor, &batch);
            ctx.sleep(wait).await?;
            // Don't advance cursor — retry the same position after sleep.
            continue;
        }

        if !batch.items.is_empty() {
            let stored = store_batch(ctx, ing_ctx, &batch.items).await?;
            total_items += stored;
            tracker.count_batch(&batch.items, stored);
            batches += 1;

            // Advance watermark incrementally after each batch so retries
            // don't re-fetch already-stored data.
            if let Some(wm) = extract_watermark(&cursor, watermark_field)
                && !wm.is_empty()
            {
                advance_watermark(ctx, ing_ctx, &cursor, total_items, watermark_field).await?;
            }

            tracing::debug!(batch_stored = stored, total_items, "stored batch");
        }

        // If diffs were skipped due to rate limiting, sleep durably then retry.
        if !batch.skipped_diffs.is_empty() {
            if let Some(ref rl) = batch.rate_limit
                && rl.remaining == 0
            {
                let wait = diff_rate_limit_sleep_duration(rl);
                tracing::info!(
                    wait_secs = wait.as_secs(),
                    skipped = batch.skipped_diffs.len(),
                    "sleeping for REST rate limit reset before retrying diffs"
                );
                ctx.sleep(wait).await?;
            }

            retry_skipped_diffs(ctx, ing_ctx, &batch.items, &batch.skipped_diffs).await?;
        }

        update_progress!(ing_ctx, run_id, total_items, tracker, &cursor, &batch);

        // Periodic progress log at info level for long-running backfills
        if last_progress_log.elapsed() >= std::time::Duration::from_secs(60) {
            tracing::info!(total_items, batches, "progress");
            last_progress_log = std::time::Instant::now();
        }

        let Some(next_cursor) = batch.next_cursor else {
            break;
        };
        // For GitHub, cursor comes from next_cursor. For Jira/Discourse,
        // cursor was already updated from etag above. In all cases, if
        // next_cursor is Some, that's the authoritative next position.
        cursor = next_cursor;
    }

    // Final progress
    let final_progress = tracker.build_final_progress();
    if let Err(e) = ing_ctx
        .repos
        .activity
        .update_run_progress_detail(run_id, total_items, &final_progress)
        .await
    {
        tracing::debug!(error = %e, "failed to update final progress");
    }

    Ok((total_items, cursor))
}
