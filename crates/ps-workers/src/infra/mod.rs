pub mod registry;
pub mod retry;
pub mod run_lifecycle;
pub mod secrets;
pub mod state;

pub use secrets::{decrypt_optional_secret, decrypt_required_secret};
pub use state::SharedState;

use ps_core::models::SourceConfig;
use ps_core::repo::Repos;

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
