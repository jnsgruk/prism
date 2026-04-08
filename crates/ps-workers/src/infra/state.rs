use std::sync::Arc;

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
    pub container_manager: Option<Arc<ps_agent::ContainerManager>>,
}
