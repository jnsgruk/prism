pub mod github_ingestion;
pub mod github_team_sync;
pub mod metrics_compute;

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

/// Load an enabled source config by name.
///
/// Called from inside a `ctx.run()` closure by handlers that need source
/// config. Kept as a free function so it can be shared across handlers
/// regardless of Restate context type.
pub async fn load_source_config(repos: &Repos, source_name: &str) -> Result<SourceConfig, String> {
    repos
        .config
        .get_enabled_source_by_name(source_name)
        .await
        .map_err(|e| format!("db error: {e}"))?
        .ok_or_else(|| format!("source '{source_name}' not found or disabled"))
}
