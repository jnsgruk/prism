use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    pub id: Uuid,
    pub person_id: Option<Uuid>,
    pub platform: String,
    pub contribution_type: String,
    pub platform_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub state: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: Option<OffsetDateTime>,
    pub closed_at: Option<OffsetDateTime>,
    pub metrics: serde_json::Value,
    pub metadata: serde_json::Value,
    pub content: Option<String>,
    pub state_history: Option<serde_json::Value>,
    pub ingested_at: OffsetDateTime,
}
