use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub id: Uuid,
    pub source_type: String,
    pub name: String,
    pub enabled: bool,
    pub settings: serde_json::Value,
    pub schedule_cron: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSetting {
    pub key: String,
    pub value: serde_json::Value,
    pub updated_at: OffsetDateTime,
}
