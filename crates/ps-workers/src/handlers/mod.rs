pub mod discourse_ingestion;
pub mod embedding;
pub mod enrichment;
pub mod github_ingestion;
pub mod github_team_sync;
pub mod identity_resolution;
pub mod ingestion_common;
pub mod insights;
pub mod jira_ingestion;
pub mod metrics_compute;
pub mod model_catalogue;
mod run_lifecycle;

use ps_core::models::SourceConfig;
use ps_core::repo::Repos;
use zeroize::Zeroizing;

/// Shared state available to all Restate handlers.
///
/// Constructed once in `main.rs`, cloned into each handler impl.
#[derive(Clone)]
pub struct SharedState {
    pub repos: Repos,
    pub secret_key: Zeroizing<[u8; 32]>,
    pub http_client: reqwest::Client,
}

/// Load an enabled source config by source type (the Restate virtual object key).
///
/// Called from inside a `ctx.run()` closure by handlers that need source
/// config. Kept as a free function so it can be shared across handlers
/// regardless of Restate context type.
pub async fn load_source_config(
    repos: &Repos,
    source_type_key: &str,
) -> Result<SourceConfig, String> {
    repos
        .config
        .get_enabled_source_by_type(source_type_key)
        .await
        .map_err(|e| format!("db error: {e}"))?
        .ok_or_else(|| format!("source type '{source_type_key}' not found or disabled"))
}
