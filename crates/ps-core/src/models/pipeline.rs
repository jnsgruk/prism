use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// A pipeline orchestration record tracking a full data pipeline run.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Pipeline {
    pub id: Uuid,
    pub status: String,
    pub current_stage: Option<String>,
    pub started_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
    pub stages: serde_json::Value,
    pub current_invocation_id: Option<String>,
    pub error: Option<String>,
    pub request_snapshot: serde_json::Value,
    pub requested_by: Option<Uuid>,
    pub requested_by_username: Option<String>,
    pub cancellation_requested: bool,
    pub dispatch_acknowledged: bool,
    pub dispatch_attempts: i32,
    pub dispatch_after: OffsetDateTime,
}
