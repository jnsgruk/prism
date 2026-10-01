//! Restate service for processing ingestion chunks.
//!
//! Each chunk is a separate Restate invocation with its own small journal.
//! The handler (coordinator) dispatches chunks sequentially via `.call()`,
//! keeping its own journal minimal (~1 entry per chunk).

use ps_core::ingestion::ContributionInput;
use ps_core::models::{Platform, SourceConfig};
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infra::run_lifecycle::{
    ensure_owned_active, journaled_value, register_owned_self, terminal_err,
};
use crate::infra::{
    SharedState, decrypt_optional_secret, decrypt_required_secret, load_source_config,
};

use super::orchestration::build_ingestion_context;
use super::progress::{IngestionSpec, ProgressTracker};

mod batch;
mod fetch_loop;

use fetch_loop::chunk_fetch_store_loop;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<ps_core::ingestion::SourceRunContext>,
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

    async fn process_scoped_chunk(
        request: Json<ChunkRequest>,
    ) -> Result<Json<ChunkResult>, TerminalError>;
}

pub struct IngestionChunkServiceImpl {
    pub state: SharedState,
}

impl IngestionChunkService for IngestionChunkServiceImpl {
    async fn process_scoped_chunk(
        &self,
        ctx: Context<'_>,
        Json(req): Json<ChunkRequest>,
    ) -> Result<Json<ChunkResult>, TerminalError> {
        let request = req
            .request
            .as_ref()
            .ok_or_else(|| TerminalError::new("scoped chunk requires a snapshot"))?;
        request
            .validate()
            .map_err(terminal_err("invalid scoped chunk"))?;
        super::scope::reject_unavailable_scope(request)?;
        if req.source_type != request.source.platform {
            return Err(TerminalError::new("chunk source does not match snapshot"));
        }
        register_owned_self!(
            ctx,
            self.state.repos,
            request.pipeline_id,
            "chunk",
            Some(req.run_id)
        );
        ensure_owned_active!(ctx, self.state.repos, request.pipeline_id)?;
        let config = load_exact_chunk_config(&ctx, &self.state.repos, request).await?;
        self.process_chunk_with_config(&ctx, req, config).await
    }

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
        self.process_chunk_with_config(&ctx, req, config).await
    }
}

impl IngestionChunkServiceImpl {
    async fn process_chunk_with_config(
        &self,
        ctx: &Context<'_>,
        req: ChunkRequest,
        config: SourceConfig,
    ) -> Result<Json<ChunkResult>, TerminalError> {
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
        let mut ing_ctx = build_ingestion_context(&self.state, &config, token, email, api_username);
        ing_ctx.request = req.request;
        let source =
            crate::infra::registry::create_source(&config.source_type).ok_or_else(|| {
                TerminalError::new(format!("unsupported source type: {}", config.source_type))
            })?;
        let watermark_field = source.watermark_field();

        // 4. Run the batch-limited fetch-store loop.
        let mut tracker = create_progress_tracker(&config.source_type);
        let (items_stored, cursor, is_complete) = chunk_fetch_store_loop(
            ctx,
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

async fn load_exact_chunk_config(
    ctx: &Context<'_>,
    repos: &ps_core::repo::Repos,
    request: &ps_core::ingestion::SourceRunContext,
) -> Result<SourceConfig, TerminalError> {
    let repos = repos.clone();
    let selected = request.source.clone();
    Ok(journaled_value!(
        ctx,
        "load_exact_config",
        [repos, selected],
        {
            let config = repos
                .config
                .get_source(selected.source_id.into_inner())
                .await
                .map_err(terminal_err("failed to load source"))?
                .ok_or_else(|| TerminalError::new("selected source no longer exists"))?;
            if config.source_type != selected.platform || config.name != selected.source_name {
                return Err(TerminalError::new("selected source identity changed").into());
            }
            config
        }
    ))
}
