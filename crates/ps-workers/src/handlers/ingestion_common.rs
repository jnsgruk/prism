//! Shared boilerplate for ingestion handlers.
//!
//! All three ingestion handlers (GitHub, Jira, Discourse) share near-identical
//! code for loading config, creating/completing/failing runs, decrypting
//! secrets, and running fetch/store/advance loops through Restate `ctx.run()`
//! closures. This module extracts that boilerplate into free functions.

use ps_core::ingestion::{ContributionInput, IngestionContext};
use ps_core::models::{RateLimitInfo, SourceConfig};
use ps_core::repo::Repos;
use ps_core::repo::reasoning::{EnrichmentQueueEntry, content_hash};
use restate_sdk::prelude::*;
use tracing::{debug, error};
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
    let repos = repos.clone();
    let sn = source_name.to_string();
    let handler = handler_name.to_string();
    let method_owned = method.to_string();
    ctx.run(|| {
        let repos = repos.clone();
        let sn = sn.clone();
        let handler = handler.clone();
        let method_owned = method_owned.clone();
        async move {
            let id = Uuid::now_v7();
            repos
                .activity
                .create_run(id, &sn, &handler, &method_owned)
                .await
                .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
            Ok(Json::from(id.to_string()))
        }
    })
    .name("create_run")
    .await?
    .into_inner()
    .parse()
    .map_err(|e| TerminalError::new(format!("invalid run_id: {e}")))
}

/// Mark a run as complete inside a Restate `ctx.run()` closure.
pub(super) async fn complete_ingestion_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run_id: Uuid,
    source_name: &str,
    items_collected: i32,
) {
    let repos = repos.clone();
    let sn = source_name.to_string();
    let result = ctx
        .run(|| {
            let repos = repos.clone();
            let sn = sn.clone();
            async move {
                repos
                    .activity
                    .complete_run(run_id, items_collected)
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                repos
                    .activity
                    .clear_current_invocation_id(&sn)
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                Ok(Json::from(()))
            }
        })
        .name("complete_run")
        .await;

    if let Err(e) = result {
        error!(source = source_name, "failed to update run status: {e}");
    }
}

/// Mark a run as failed inside a Restate `ctx.run()` closure.
pub(super) async fn fail_ingestion_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run_id: Uuid,
    source_name: &str,
    error_msg: &str,
) {
    let repos = repos.clone();
    let err = error_msg.to_string();
    let sn = source_name.to_string();
    let result = ctx
        .run(|| {
            let repos = repos.clone();
            let err = err.clone();
            let sn = sn.clone();
            async move {
                repos
                    .activity
                    .fail_run(run_id, &err)
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                repos
                    .activity
                    .clear_current_invocation_id(&sn)
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                Ok(Json::from(()))
            }
        })
        .name("fail_run")
        .await;

    if let Err(e) = result {
        error!(source = source_name, "failed to update run status: {e}");
    }
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

/// Fetch a batch inside a Restate `ctx.run()` closure.
pub(super) async fn fetch_batch(
    ctx: &ObjectContext<'_>,
    state: &SharedState,
    config: &SourceConfig,
    cursor: &str,
    token: Option<&str>,
) -> Result<SerFetchResult, TerminalError> {
    let repos = state.repos.clone();
    let http = state.http_client.clone();
    let cfg = config.clone();
    let tok = token.map(String::from);
    let cur = cursor.to_string();
    let source_type = config.source_type.clone();

    Ok(ctx
        .run(|| {
            let repos = repos.clone();
            let http = http.clone();
            let cfg = cfg.clone();
            let cur = cur.clone();
            let source_type = source_type.clone();
            async move {
                let src = registry::create_source(&source_type)
                    .ok_or_else(|| TerminalError::new("source unavailable"))?;
                let ic = IngestionContext {
                    repos,
                    source_config: cfg,
                    http_client: http,
                    token: tok,
                    email: None,
                    api_username: None,
                };
                let result = src
                    .fetch_batch(&ic, &cur)
                    .await
                    .map_err(|e| TerminalError::new(format!("fetch failed: {e}")))?;

                Ok(Json::from(SerFetchResult {
                    items: result.items,
                    next_cursor: result.next_cursor,
                    rate_limit: result.rate_limit,
                }))
            }
        })
        .name("fetch_batch")
        .await?
        .into_inner())
}

/// Store a batch inside a Restate `ctx.run()` closure.
pub(super) async fn store_batch(
    ctx: &ObjectContext<'_>,
    state: &SharedState,
    config: &SourceConfig,
    items: &[ContributionInput],
    token: Option<&str>,
) -> Result<i32, TerminalError> {
    let repos = state.repos.clone();
    let http = state.http_client.clone();
    let cfg = config.clone();
    let tok = token.map(String::from);
    let items = items.to_vec();
    let source_type = config.source_type.clone();

    Ok(ctx
        .run(|| {
            let repos = repos.clone();
            let http = http.clone();
            let cfg = cfg.clone();
            let items = items.clone();
            let source_type = source_type.clone();
            async move {
                let src = registry::create_source(&source_type)
                    .ok_or_else(|| TerminalError::new("source unavailable"))?;
                let ic = IngestionContext {
                    repos,
                    source_config: cfg,
                    http_client: http,
                    token: tok,
                    email: None,
                    api_username: None,
                };
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
    state: &SharedState,
    config: &SourceConfig,
    cursor: &str,
    total_items: i32,
    token: Option<&str>,
    watermark_field: &str,
) -> Result<(), TerminalError> {
    let repos = state.repos.clone();
    let http = state.http_client.clone();
    let cfg = config.clone();
    let tok = token.map(String::from);
    let wm = cursor.to_string();
    let source_type = config.source_type.clone();
    let field = watermark_field.to_string();

    ctx.run(|| {
        let repos = repos.clone();
        let http = http.clone();
        let cfg = cfg.clone();
        let wm = wm.clone();
        let source_type = source_type.clone();
        let field = field.clone();
        async move {
            let src = registry::create_source(&source_type)
                .ok_or_else(|| TerminalError::new("source unavailable"))?;
            let ic = IngestionContext {
                repos,
                source_config: cfg,
                http_client: http,
                token: tok,
                email: None,
                api_username: None,
            };
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
