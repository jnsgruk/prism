use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// A pipeline orchestration record tracking a full data pipeline run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: Uuid,
    pub status: String,
    pub current_stage: Option<String>,
    pub started_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
    pub stages: serde_json::Value,
    pub current_invocation_id: Option<String>,
    pub error: Option<String>,
}
