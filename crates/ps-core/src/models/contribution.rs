use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{ContributionState, ContributionType, Platform, PlatformId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    pub id: Uuid,
    pub person_id: Option<Uuid>,
    pub platform: Platform,
    pub contribution_type: ContributionType,
    pub platform_id: PlatformId,
    pub title: Option<String>,
    pub url: Option<String>,
    pub state: Option<ContributionState>,
    pub created_at: OffsetDateTime,
    pub updated_at: Option<OffsetDateTime>,
    pub closed_at: Option<OffsetDateTime>,
    pub metrics: serde_json::Value,
    pub metadata: serde_json::Value,
    pub content: Option<String>,
    pub state_history: Option<serde_json::Value>,
    pub ingested_at: OffsetDateTime,
}
