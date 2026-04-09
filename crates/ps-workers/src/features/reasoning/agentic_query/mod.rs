mod handler;

use crate::infra::SharedState;

pub use handler::{AgenticQueryHandler, AgenticQueryHandlerImpl};

/// Request payload for `prepare_query`.
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

/// Response from `prepare_query` — the pod is ready and reachable at this IP.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrepareQueryResponse {
    pub pod_ip: String,
    pub pod_name: String,
}
