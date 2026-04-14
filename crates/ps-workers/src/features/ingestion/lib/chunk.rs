//! Restate service for processing ingestion chunks.
//!
//! Each chunk is a separate Restate invocation with its own small journal.
//! The handler (coordinator) dispatches chunks sequentially via `.call()`,
//! keeping its own journal minimal (~1 entry per chunk).

use ps_core::ingestion::{ContributionInput, IngestionContext};
use ps_core::models::{Platform, SourceConfig};
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infra::run_lifecycle::{journaled, journaled_value, terminal_err};
use crate::infra::{
    SharedState, decrypt_optional_secret, decrypt_required_secret, load_source_config,
};

use super::finalise::{diff_rate_limit_sleep_duration, extract_watermark};
use super::orchestration::{build_ingestion_context, fetch_batch};
use super::progress::{
    BatchAction, IngestionSpec, ProgressTracker, SerFetchResult, SkippedDiffAction,
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRequest {
    /// Source platform type used to load config + create source adapter.
    pub source_type: Platform,
    /// Opaque cursor JSON — continues from previous chunk.
    pub cursor: String,
    /// Coordinator's run ID for progress updates.
    pub run_id: Uuid,
    /// Maximum batches to process before returning.
    pub max_batches: usize,
    /// Items already stored by previous chunks. Added to this chunk's count
    /// so progress display shows the global total.
    pub items_offset: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkResult {
    /// Items stored in this chunk.
    pub items_stored: i32,
    /// Cursor position at end of chunk.
    pub cursor: String,
    /// `true` if the fetch-store loop reached end-of-data.
    pub is_complete: bool,
}

// ---------------------------------------------------------------------------
// Service definition
// ---------------------------------------------------------------------------

#[restate_sdk::service]
pub trait IngestionChunkService {
    async fn process_chunk(request: Json<ChunkRequest>)
    -> Result<Json<ChunkResult>, TerminalError>;
}

pub struct IngestionChunkServiceImpl {
    pub state: SharedState,
}

impl IngestionChunkService for IngestionChunkServiceImpl {
    async fn process_chunk(
        &self,
        ctx: Context<'_>,
        Json(req): Json<ChunkRequest>,
    ) -> Result<Json<ChunkResult>, TerminalError> {
        let source_type_key = req.source_type.to_string();
        let span = tracing::info_span!("chunk", source = %source_type_key, run_id = %req.run_id);
        let _guard = span.enter();

        // 1. Load source config (journaled).
        let config = load_chunk_source_config(&ctx, &self.state.repos, &source_type_key).await?;
        let spec = spec_for_source_type(&config.source_type);

        // 2. Decrypt secrets (outside ctx.run()).
        let token = match (spec.token_key, spec.token_required) {
            (Some(key), true) => Some(decrypt_required_secret(&self.state, config.id, key).await?),
            (Some(key), false) => decrypt_optional_secret(&self.state, config.id, key).await?,
            (None, _) => None,
        };
        let email = match spec.email_key {
            Some(key) => decrypt_optional_secret(&self.state, config.id, key).await?,
            None => None,
        };
        let api_username = match spec.api_username_key {
            Some(key) => decrypt_optional_secret(&self.state, config.id, key).await?,
            None => None,
        };

        // 3. Build context + source.
        let ing_ctx = build_ingestion_context(&self.state, &config, token, email, api_username);
        let source =
            crate::infra::registry::create_source(&config.source_type).ok_or_else(|| {
                TerminalError::new(format!("unsupported source type: {}", config.source_type))
            })?;
        let watermark_field = source.watermark_field();

        // 4. Run the batch-limited fetch-store loop.
        let mut tracker = create_progress_tracker(&config.source_type);
        let (items_stored, cursor, is_complete) = chunk_fetch_store_loop(
            &ctx,
            &ing_ctx,
            req.run_id,
            &req.cursor,
            watermark_field,
            req.max_batches,
            req.items_offset,
            tracker.as_mut(),
        )
        .await?;

        tracing::info!(items_stored, is_complete, "chunk complete");

        Ok(Json(ChunkResult {
            items_stored,
            cursor,
            is_complete,
        }))
    }
}

// ---------------------------------------------------------------------------
// Batch-limited fetch-store loop (Context<'_> version)
// ---------------------------------------------------------------------------

/// Best-effort progress update (not journaled).
macro_rules! chunk_update_progress {
    ($ing_ctx:expr, $run_id:expr, $global_items:expr, $tracker:expr, $cursor:expr, $batch:expr) => {{
        let progress = $tracker.build_progress($cursor, $batch.rate_limit.as_ref());
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
async fn chunk_fetch_store_loop(
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
                chunk_update_progress!(ing_ctx, run_id, global, tracker, &cursor, &batch);
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

                if last_progress_log.elapsed() >= std::time::Duration::from_secs(60) {
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

/// Load source config inside a journaled `ctx.run()` (service context variant).
async fn load_chunk_source_config(
    ctx: &Context<'_>,
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

/// Store a batch inside a journaled `ctx.run()` (service context variant).
async fn chunk_store_batch(
    ctx: &Context<'_>,
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

/// Advance the watermark inside a journaled `ctx.run()` (service context variant).
async fn chunk_advance_watermark(
    ctx: &Context<'_>,
    ing_ctx: &IngestionContext,
    cursor: &str,
    total_items: i32,
    watermark_field: ps_core::models::WatermarkField,
) -> Result<(), TerminalError> {
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
async fn chunk_retry_skipped_diffs(
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

    for sd in skipped {
        match crate::features::ingestion::github::source::fetch::fetch_single_pr_diff(
            &client,
            &sd.owner,
            &sd.repo,
            sd.pr_number,
        )
        .await
        {
            crate::features::ingestion::github::source::fetch::DiffFetchResult::Ok(diff_text) => {
                if let Some(item) = original_items.get(sd.item_index)
                    && let Some(ref enrichment) = item.enrichment_content
                {
                    let mut content = enrichment.clone();
                    if let Some(obj) = content.as_object_mut() {
                        obj.insert("diff".to_string(), serde_json::Value::String(diff_text));
                    }
                    updated_items.push((item.platform_id.to_string(), content));
                }
            }
            crate::features::ingestion::github::source::fetch::DiffFetchResult::RateLimited(_) => {
                tracing::warn!(
                    remaining = skipped.len() - updated_items.len(),
                    "diff retry also hit rate limit, skipping remaining"
                );
                break;
            }
            crate::features::ingestion::github::source::fetch::DiffFetchResult::Failed => {}
        }
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
fn compute_batch_action(
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
            etag_cursor,
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

/// Look up the `IngestionSpec` for a source type.
///
/// Mirrors the `const *_SPEC` definitions in each handler module.
fn spec_for_source_type(source_type: &ps_core::models::Platform) -> IngestionSpec {
    use ps_core::models::{Platform, SecretKey};

    match source_type {
        Platform::Github => IngestionSpec {
            handler_name: "GithubIngestionHandler",
            token_key: Some(SecretKey::ApiToken),
            token_required: true,
            email_key: None,
            api_username_key: None,
            item_noun: "repo",
        },
        Platform::Jira => IngestionSpec {
            handler_name: "JiraIngestionHandler",
            token_key: Some(SecretKey::ApiToken),
            token_required: true,
            email_key: Some(SecretKey::Email),
            api_username_key: None,
            item_noun: "project",
        },
        Platform::Discourse(_) => IngestionSpec {
            handler_name: "DiscourseIngestionHandler",
            token_key: Some(SecretKey::ApiKey),
            token_required: false,
            email_key: None,
            api_username_key: Some(SecretKey::ApiUsername),
            item_noun: "category",
        },
        _ => IngestionSpec {
            handler_name: "UnknownHandler",
            token_key: None,
            token_required: false,
            email_key: None,
            api_username_key: None,
            item_noun: "item",
        },
    }
}

/// Create a platform-specific progress tracker.
fn create_progress_tracker(
    source_type: &ps_core::models::Platform,
) -> Box<dyn ProgressTracker + Send> {
    use ps_core::models::Platform;

    match source_type {
        Platform::Github => {
            Box::new(crate::features::ingestion::github::handler::GithubProgressTracker::default())
        }
        Platform::Jira => {
            Box::new(crate::features::ingestion::jira::handler::JiraProgressTracker::default())
        }
        Platform::Discourse(_) => Box::new(
            crate::features::ingestion::discourse::handler::DiscourseProgressTracker::default(),
        ),
        _ => Box::new(GenericProgressTracker::default()),
    }
}

/// Fallback progress tracker for unknown source types.
#[derive(Default)]
struct GenericProgressTracker {
    items: u32,
}

impl ProgressTracker for GenericProgressTracker {
    fn count_batch(&mut self, items: &[ContributionInput], _stored: i32) {
        self.items += items.len() as u32;
    }

    fn build_progress(
        &self,
        _cursor: &str,
        _rate_limit: Option<&ps_core::models::RateLimitInfo>,
    ) -> serde_json::Value {
        serde_json::json!({
            "phase": "processing",
            "items_fetched": self.items,
            "status_message": format!("Processing ({} items fetched)", self.items),
        })
    }

    fn build_final_progress(&self) -> serde_json::Value {
        serde_json::json!({
            "phase": "complete",
            "items_fetched": self.items,
        })
    }
}
