use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use super::Platform;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub id: Uuid,
    pub source_type: Platform,
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

/// A cached model entry from an AI provider's model catalogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModel {
    pub id: String,
    pub provider: super::AiProvider,
    pub display_name: String,
    pub description: Option<String>,
    pub context_length: Option<i32>,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    pub capabilities: Vec<String>,
}
