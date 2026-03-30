//! Shared boilerplate for ingestion handlers.
//!
//! All three ingestion handlers (GitHub, Jira, Discourse) share near-identical
//! code for loading config, creating/completing/failing runs, decrypting
//! secrets, and running fetch/store/advance loops through Restate `ctx.run()`
//! closures. This module extracts that boilerplate into free functions.

use ps_core::ingestion::{ContributionInput, FailedItem, IngestionContext, SkippedDiff};
use ps_core::models::{RateLimitInfo, SourceConfig};
use ps_core::repo::Repos;
use ps_core::repo::reasoning::{EnrichmentQueueEntry, content_hash};
use restate_sdk::prelude::*;
use tracing::debug;
use uuid::Uuid;

use super::SharedState;
use super::run_lifecycle::{journaled, journaled_value, terminal_err};
use crate::registry;

/// Serialisable fetch result for Restate journaling.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct SerFetchResult {
    pub items: Vec<ContributionInput>,
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub rate_limit: Option<RateLimitInfo>,
    /// Carries the latest cursor state for watermark extraction, even when
    /// `next_cursor` is `None` (final batch). Used by Discourse ingestion.
    #[serde(default)]
    pub etag: Option<String>,
    /// PR diffs skipped due to REST rate limiting (GitHub only).
    #[serde(default)]
    pub skipped_diffs: Vec<SkippedDiff>,
}

/// Extract a named field from a serialised cursor JSON string.
///
/// GitHub and Jira use `"max_updated_at"`, Discourse uses `"max_bumped_at"`.
pub(super) fn extract_watermark(cursor_json: &str, field: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(cursor_json)
        .ok()?
        .get(field)?
        .as_str()
        .map(String::from)
}

/// Load a source config inside a Restate `ctx.run()` closure.
pub(super) async fn load_source_config(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    source_name: &str,
) -> Result<SourceConfig, TerminalError> {
    let repos = repos.clone();
    let name = source_name.to_string();
    Ok(journaled_value!(ctx, "load_config", [repos, name], {
        super::load_source_config(&repos, &name)
            .await
            .map_err(TerminalError::new)?
    }))
}

/// Create an ingestion run record inside a Restate `ctx.run()` closure.
pub(super) async fn create_ingestion_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    source_name: &str,
    handler_name: &str,
    method: &str,
) -> Result<Uuid, TerminalError> {
    super::run_lifecycle::create_run!(ctx, repos, source_name, handler_name, method)
}

/// Mark a run as complete inside a Restate `ctx.run()` closure.
pub(super) async fn complete_ingestion_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run_id: Uuid,
    source_name: &str,
    items_collected: i32,
) {
    super::run_lifecycle::complete_run!(ctx, repos, run_id, source_name, items_collected);
}

/// Mark a run as completed with warnings inside a Restate `ctx.run()` closure.
pub(super) async fn complete_ingestion_run_with_warnings(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run_id: Uuid,
    source_name: &str,
    items_collected: i32,
    error_summary: &str,
    metadata: serde_json::Value,
) {
    super::run_lifecycle::complete_run_with_warnings!(
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
    run_id: Uuid,
    source_name: &str,
    error_msg: &str,
) {
    super::run_lifecycle::fail_run!(ctx, repos, run_id, source_name, error_msg);
}

/// Decrypt a required secret. Returns an error if the secret is not configured.
///
/// Called outside `ctx.run()` to avoid journaling the plaintext.
pub(super) async fn decrypt_required_secret(
    state: &SharedState,
    source_id: Uuid,
    key: &str,
) -> Result<String, TerminalError> {
    let encrypted = state
        .repos
        .config
        .get_encrypted_secret(source_id, key)
        .await
        .map_err(terminal_err("db error"))?
        .ok_or_else(|| TerminalError::new(format!("source has no {key} configured")))?;

    let decrypted = ps_core::crypto::decrypt(&state.secret_key, &encrypted)
        .map_err(terminal_err("decrypt error"))?;

    String::from_utf8(decrypted).map_err(|e| TerminalError::new(format!("invalid encoding: {e}")))
}

/// Decrypt an optional secret. Returns `Ok(None)` if the secret is not configured.
///
/// Called outside `ctx.run()` to avoid journaling the plaintext.
pub(super) async fn decrypt_optional_secret(
    state: &SharedState,
    source_id: Uuid,
    key: &str,
) -> Result<Option<String>, TerminalError> {
    let encrypted = state
        .repos
        .config
        .get_encrypted_secret(source_id, key)
        .await
        .map_err(terminal_err("db error"))?;

    match encrypted {
        Some(enc) => {
            let decrypted = ps_core::crypto::decrypt(&state.secret_key, &enc)
                .map_err(terminal_err("decrypt error"))?;
            let s = String::from_utf8(decrypted).map_err(terminal_err("invalid encoding"))?;
            Ok(Some(s))
        }
        None => Ok(None),
    }
}

/// Source-specific progress tracking for the fetch-store loop.
pub(super) trait ProgressTracker {
    /// Count items from a fetched batch (e.g. increment PR/ticket/topic counter).
    fn count_batch(&mut self, items: &[ContributionInput], stored: i32);

    /// Build a progress JSON object from current counters and cursor state.
    fn build_progress(&self, cursor: &str, rate_limit: Option<&RateLimitInfo>)
    -> serde_json::Value;

    /// Build the final "complete" progress JSON.
    fn build_final_progress(&self) -> serde_json::Value;
}

/// Unified fetch-store loop shared by all three ingestion handlers.
///
/// Fetches batches from the source, stores them via journaled `ctx.run()`,
/// updates progress, and returns `(total_items, final_cursor)`.
pub(super) async fn fetch_store_loop(
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
        let batch = {
            use futures::FutureExt as _;
            use std::panic::AssertUnwindSafe;
            AssertUnwindSafe(fetch_batch(ing_ctx, &cursor))
                .catch_unwind()
                .await
                .map_err(|panic| {
                    let msg = panic
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| panic.downcast_ref::<&str>().copied())
                        .unwrap_or("unknown panic");
                    TerminalError::new(format!("fetch panicked: {msg}"))
                })??
        };

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
            let progress = tracker.build_progress(&cursor, batch.rate_limit.as_ref());
            if let Err(e) = ing_ctx
                .repos
                .activity
                .update_run_progress_detail(run_id, total_items, &progress)
                .await
            {
                tracing::debug!(error = %e, "failed to update run progress");
            }

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

        let progress = tracker.build_progress(&cursor, batch.rate_limit.as_ref());
        if let Err(e) = ing_ctx
            .repos
            .activity
            .update_run_progress_detail(run_id, total_items, &progress)
            .await
        {
            tracing::debug!(error = %e, "failed to update run progress");
        }

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

/// Specification for an ingestion handler, describing which secrets to decrypt
/// and what to call the item type in error summaries.
pub(super) struct IngestionSpec {
    pub handler_name: &'static str,
    /// Secret key name for the API token (e.g. `"api_token"`, `"api_key"`).
    /// `None` if this source has no token.
    pub token_key: Option<&'static str>,
    /// Whether the token is required (error if missing) vs optional.
    pub token_required: bool,
    /// Secret key name for an email credential (Jira Basic auth).
    pub email_key: Option<&'static str>,
    /// Secret key name for an API username (Discourse).
    pub api_username_key: Option<&'static str>,
    /// Noun for items in error summaries (e.g. `"repo"`, `"project"`, `"category"`).
    pub item_noun: &'static str,
}

/// Shared ingestion orchestration used by all three ingestion handlers.
///
/// Handles: source creation, run creation, secret decryption, planning,
/// watermark override, fetch/store loop, run finalisation.
///
/// The caller provides a `ProgressTracker` for source-specific progress
/// reporting, and a closure to fire downstream triggers after completion.
#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_ingestion(
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

    let source = registry::create_source(&config.source_type).ok_or_else(|| {
        TerminalError::new(format!("unsupported source type: {}", config.source_type))
    })?;

    let method = if override_watermark.is_some() {
        "backfill"
    } else {
        "run_ingestion"
    };
    let run_id =
        create_ingestion_run(ctx, &state.repos, source_name, spec.handler_name, method).await?;

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
pub(super) fn build_ingestion_context(
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

/// Fetch a batch — NOT journaled (external API call, large response, idempotent on replay).
///
/// External API calls go outside `ctx.run()` because responses are large and
/// re-executing is safe (stores use idempotent upserts). Only DB writes
/// (`store_batch`, `advance_watermark`, run lifecycle) should be journaled.
pub(super) async fn fetch_batch(
    ing_ctx: &IngestionContext,
    cursor: &str,
) -> Result<SerFetchResult, TerminalError> {
    let src = registry::create_source(&ing_ctx.source_config.source_type)
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
pub(super) async fn store_batch(
    ctx: &ObjectContext<'_>,
    ing_ctx: &IngestionContext,
    items: &[ContributionInput],
) -> Result<i32, TerminalError> {
    let ic = ing_ctx.clone();
    let items = items.to_vec();

    #[allow(clippy::cast_possible_wrap)]
    Ok(journaled_value!(ctx, "store_batch", [ic, items], {
        let src = registry::create_source(&ic.source_config.source_type)
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
pub(super) async fn advance_watermark(
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
        let src = registry::create_source(&ic.source_config.source_type)
            .ok_or_else(|| TerminalError::new("source unavailable"))?;
        let watermark = extract_watermark(&wm, &field).unwrap_or_default();
        src.advance_watermark(&ic, &watermark, total_items)
            .await
            .map_err(terminal_err("advance failed"))?;
    });

    Ok(())
}

/// Extract failed items from the final cursor JSON.
pub(super) fn extract_failed_items(cursor: &str) -> Vec<FailedItem> {
    serde_json::from_str::<serde_json::Value>(cursor)
        .ok()
        .and_then(|v| v.get("failed_items").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Finalise an ingestion run based on whether there were failures.
///
/// Three outcomes:
/// - No failures → advance watermark + complete
/// - All items failed (`total_items` == 0) → fail
/// - Partial failure → complete with warnings (do NOT advance watermark)
#[allow(clippy::too_many_arguments)]
pub(super) async fn finalise_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    ing_ctx: &IngestionContext,
    run_id: Uuid,
    source_name: &str,
    total_items: i32,
    failed_items: &[FailedItem],
    item_noun: &str,
    final_cursor: &str,
    watermark_field: &str,
) -> Result<(), TerminalError> {
    if failed_items.is_empty() {
        if total_items > 0 {
            advance_watermark(ctx, ing_ctx, final_cursor, total_items, watermark_field).await?;
        }
        complete_ingestion_run(ctx, repos, run_id, source_name, total_items).await;
    } else if total_items == 0 {
        let summary = format!(
            "all {} {item_noun}(s) failed: {}",
            failed_items.len(),
            failed_items
                .iter()
                .map(|f| f.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        fail_ingestion_run(ctx, repos, run_id, source_name, &summary).await;
    } else {
        // Partial failure — do NOT advance watermark.
        let summary = format!(
            "{} {item_noun}(s) failed: {}",
            failed_items.len(),
            failed_items
                .iter()
                .map(|f| f.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let metadata = serde_json::json!({ "failed_items": failed_items });
        complete_ingestion_run_with_warnings(
            ctx,
            repos,
            run_id,
            source_name,
            total_items,
            &summary,
            metadata,
        )
        .await;
    }
    Ok(())
}

/// Compute how long to sleep until a rate limit reset, with a buffer.
fn diff_rate_limit_sleep_duration(rate_limit: &RateLimitInfo) -> std::time::Duration {
    let now = time::OffsetDateTime::now_utc();
    let delta = rate_limit.reset_at - now;
    let secs = delta.whole_seconds().max(1) + 1;
    #[allow(clippy::cast_sign_loss)]
    std::time::Duration::from_secs(secs as u64)
}

/// Retry fetching PR diffs that were skipped due to rate limiting,
/// then re-enqueue the affected contributions for enrichment with
/// updated content that includes the diff.
///
/// Called after a durable `ctx.sleep()` so the rate limit has reset.
async fn retry_skipped_diffs(
    ctx: &ObjectContext<'_>,
    ing_ctx: &IngestionContext,
    original_items: &[ContributionInput],
    skipped: &[SkippedDiff],
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

    let client =
        crate::github::client::GitHubClient::new(ing_ctx.http_client.clone(), api_base, token);

    // Fetch diffs for skipped PRs.
    let mut updated_items: Vec<(String, serde_json::Value)> = Vec::new();

    for sd in skipped {
        match crate::github::source::fetch::fetch_single_pr_diff(
            &client,
            &sd.owner,
            &sd.repo,
            sd.pr_number,
        )
        .await
        {
            crate::github::source::fetch::DiffFetchResult::Ok(diff_text) => {
                // Get the original enrichment_content and add the diff.
                if let Some(item) = original_items.get(sd.item_index)
                    && let Some(ref enrichment) = item.enrichment_content
                {
                    let mut content = enrichment.clone();
                    if let Some(obj) = content.as_object_mut() {
                        obj.insert("diff".to_string(), serde_json::Value::String(diff_text));
                    }
                    let platform_id = item.platform_id.clone();
                    updated_items.push((platform_id, content));
                }
            }
            crate::github::source::fetch::DiffFetchResult::RateLimited(_) => {
                tracing::warn!(
                    remaining = skipped.len() - updated_items.len(),
                    "diff retry also hit rate limit, skipping remaining"
                );
                break;
            }
            crate::github::source::fetch::DiffFetchResult::Failed => {}
        }
    }

    if updated_items.is_empty() {
        return Ok(());
    }

    // Re-enqueue enrichments with updated content (now including diffs).
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

#[cfg(test)]
mod tests {
    use super::*;

    // -- extract_watermark --

    #[test]
    fn extract_watermark_max_updated_at() {
        let cursor = r#"{"max_updated_at": "2025-01-15T12:00:00Z", "repo_index": 0}"#;
        assert_eq!(
            extract_watermark(cursor, "max_updated_at"),
            Some("2025-01-15T12:00:00Z".into())
        );
    }

    #[test]
    fn extract_watermark_max_bumped_at() {
        let cursor = r#"{"max_bumped_at": "2025-02-01T00:00:00Z"}"#;
        assert_eq!(
            extract_watermark(cursor, "max_bumped_at"),
            Some("2025-02-01T00:00:00Z".into())
        );
    }

    #[test]
    fn extract_watermark_missing_field() {
        let cursor = r#"{"repo_index": 0}"#;
        assert_eq!(extract_watermark(cursor, "max_updated_at"), None);
    }

    #[test]
    fn extract_watermark_null_value() {
        let cursor = r#"{"max_updated_at": null}"#;
        assert_eq!(extract_watermark(cursor, "max_updated_at"), None);
    }

    #[test]
    fn extract_watermark_invalid_json() {
        assert_eq!(extract_watermark("not json", "max_updated_at"), None);
    }

    // -- extract_failed_items --

    #[test]
    fn extract_failed_items_empty() {
        let cursor = r#"{"failed_items": []}"#;
        assert!(extract_failed_items(cursor).is_empty());
    }

    #[test]
    fn extract_failed_items_with_entries() {
        let cursor = r#"{"failed_items": [{"key": "org/repo", "error": "403 forbidden"}]}"#;
        let items = extract_failed_items(cursor);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, "org/repo");
        assert_eq!(items[0].error, "403 forbidden");
    }

    #[test]
    fn extract_failed_items_missing_field() {
        let cursor = r#"{"repo_index": 0}"#;
        assert!(extract_failed_items(cursor).is_empty());
    }

    #[test]
    fn extract_failed_items_invalid_json() {
        assert!(extract_failed_items("bad json").is_empty());
    }

    #[test]
    fn extract_failed_items_multiple() {
        let cursor = r#"{"failed_items": [
            {"key": "a", "error": "e1"},
            {"key": "b", "error": "e2"},
            {"key": "c", "error": "e3"}
        ]}"#;
        let items = extract_failed_items(cursor);
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].key, "c");
    }
}

/// Enqueue enrichment content for upserted contributions.
///
/// Maps `(id, platform_id)` pairs from `bulk_upsert_contributions` back to
/// the `enrichment_content` blobs on the original items, computes content
/// hashes, and bulk-inserts into the enrichment queue.
///
/// Called from each source's `store_batch_impl` after upsert.
pub async fn enqueue_enrichments(
    repos: &Repos,
    items: &[&ContributionInput],
    upserted: &[(Uuid, String)],
) -> Result<u64, ps_core::Error> {
    // Build a platform_id → enrichment_content lookup from items.
    let content_by_platform_id: std::collections::HashMap<&str, &serde_json::Value> = items
        .iter()
        .filter_map(|item| {
            item.enrichment_content
                .as_ref()
                .map(|c| (item.platform_id.as_str(), c))
        })
        .collect();

    let entries: Vec<EnrichmentQueueEntry> = upserted
        .iter()
        .filter_map(|(contribution_id, platform_id)| {
            let content = content_by_platform_id.get(platform_id.as_str())?;
            Some(EnrichmentQueueEntry {
                contribution_id: *contribution_id,
                content: (*content).clone(),
                content_hash: content_hash(content),
            })
        })
        .collect();

    if entries.is_empty() {
        return Ok(0);
    }

    let count = repos.reasoning.bulk_enqueue_enrichments(&entries).await?;
    debug!(
        queued = entries.len(),
        updated = count,
        "enqueued enrichments"
    );
    Ok(count)
}
