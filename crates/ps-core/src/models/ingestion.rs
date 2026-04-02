use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::{IngestionStatus, RunId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watermark {
    pub source_name: String,
    pub watermark_value: String,
    pub last_successful_run: Option<OffsetDateTime>,
    pub last_attempt: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    pub items_collected_last_run: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionRun {
    pub id: RunId,
    pub source_name: String,
    pub started_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
    pub status: IngestionStatus,
    pub items_collected: Option<i32>,
    pub error_message: Option<String>,
    pub rate_limit_waits_seconds: Option<i32>,
    pub metadata: Option<serde_json::Value>,
    pub handler_name: String,
    pub handler_method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    pub remaining: i32,
    pub limit: i32,
    pub reset_at: OffsetDateTime,
}
