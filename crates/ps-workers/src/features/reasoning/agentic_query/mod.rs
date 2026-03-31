mod artifact;
mod event_loop;
mod handler;
mod query_core;
pub mod step_registry;
mod trace;

use crate::infra::SharedState;

pub use handler::{AgenticQueryHandler, AgenticQueryHandlerImpl};
pub use query_core::{QueryResult, run_agentic_query_core};

/// Request payload for `run_query`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticQueryRequest {
    pub conversation_id: String,
    pub user_id: String,
    pub question: String,
    pub model: String,
    pub small_model: String,
    pub provider_keys: Vec<(String, String)>,
    /// When set, the agent should use this model for image generation.
    #[serde(default)]
    pub image_model: Option<String>,
}
