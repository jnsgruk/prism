//! Shared boilerplate for ingestion handlers.
//!
//! All three ingestion handlers (GitHub, Jira, Discourse) share near-identical
//! code for loading config, creating/completing/failing runs, decrypting
//! secrets, and running fetch/store/advance loops through Restate `ctx.run()`
//! closures. This module extracts that boilerplate into free functions.

use ps_core::ingestion::{ContributionInput, FailedItem, IngestionContext};
use ps_core::models::{RateLimitInfo, SourceConfig};
use ps_core::repo::Repos;
use ps_core::repo::reasoning::{EnrichmentQueueEntry, content_hash};
use restate_sdk::prelude::*;
use tracing::debug;
use uuid::Uuid;

use super::SharedState;
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
    Ok(ctx
        .run(|| {
            let repos = repos.clone();
            let name = name.clone();
            async move {
                let config = super::load_source_config(&repos, &name)
                    .await
                    .map_err(TerminalError::new)?;
                Ok(Json::from(config))
            }
        })
        .name("load_config")
        .await?
        .into_inner())
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
        .map_err(|e| TerminalError::new(format!("db error: {e}")))?
        .ok_or_else(|| TerminalError::new(format!("source has no {key} configured")))?;

    let decrypted = ps_core::crypto::decrypt(&state.secret_key, &encrypted)
        .map_err(|e| TerminalError::new(format!("decrypt error: {e}")))?;

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
        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;

    match encrypted {
        Some(enc) => {
            let decrypted = ps_core::crypto::decrypt(&state.secret_key, &enc)
                .map_err(|e| TerminalError::new(format!("decrypt error: {e}")))?;
            let s = String::from_utf8(decrypted)
                .map_err(|e| TerminalError::new(format!("invalid encoding: {e}")))?;
            Ok(Some(s))
        }
        None => Ok(None),
    }
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
        .map_err(|e| TerminalError::new(format!("fetch failed: {e}")))?;

    Ok(SerFetchResult {
        items: result.items,
        next_cursor: result.next_cursor,
        rate_limit: result.rate_limit,
        etag: result.etag,
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

    Ok(ctx
        .run(|| {
            let ic = ic.clone();
            let items = items.clone();
            async move {
                let src = registry::create_source(&ic.source_config.source_type)
                    .ok_or_else(|| TerminalError::new("source unavailable"))?;
                let count = src
                    .store_batch(&ic, &items)
                    .await
                    .map_err(|e| TerminalError::new(format!("store failed: {e}")))?;
                #[allow(clippy::cast_possible_wrap)]
                Ok(Json::from(count as i32))
            }
        })
        .name("store_batch")
        .await?
        .into_inner())
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

    ctx.run(|| {
        let ic = ic.clone();
        let wm = wm.clone();
        let field = field.clone();
        async move {
            let src = registry::create_source(&ic.source_config.source_type)
                .ok_or_else(|| TerminalError::new("source unavailable"))?;
            let watermark = extract_watermark(&wm, &field).unwrap_or_default();
            src.advance_watermark(&ic, &watermark, total_items)
                .await
                .map_err(|e| TerminalError::new(format!("advance failed: {e}")))?;
            Ok(Json::from(()))
        }
    })
    .name("advance_watermark")
    .await?;

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
