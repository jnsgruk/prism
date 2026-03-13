pub mod github_ingestion;
pub mod github_team_sync;
pub mod metrics_compute;

use ps_core::repo::Repos;

/// Shared state available to all Restate handlers.
///
/// Constructed once in `main.rs`, cloned into each handler impl.
#[derive(Clone)]
pub struct SharedState {
    pub repos: Repos,
    pub secret_key: [u8; 32],
    pub http_client: reqwest::Client,
}
