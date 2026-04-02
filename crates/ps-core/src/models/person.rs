use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use super::{PersonId, Platform, PlatformUsername, TeamId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    pub id: PersonId,
    pub name: String,
    pub email: Option<String>,
    pub level: Option<String>,
    pub directory_id: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformIdentity {
    pub id: Uuid,
    pub person_id: PersonId,
    pub platform: Platform,
    pub platform_username: PlatformUsername,
    pub platform_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMembership {
    pub id: Uuid,
    pub person_id: PersonId,
    pub team_id: TeamId,
    pub start_date: Date,
    pub end_date: Option<Date>,
}
