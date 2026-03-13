use std::fmt;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// The hierarchical type of a team entry: org → group → team → squad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "org.team_type", rename_all = "lowercase")]
pub enum TeamType {
    Org,
    Group,
    Team,
    Squad,
}

impl fmt::Display for TeamType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Org => write!(f, "org"),
            Self::Group => write!(f, "group"),
            Self::Team => write!(f, "team"),
            Self::Squad => write!(f, "squad"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: Uuid,
    pub name: String,
    pub org_name: String,
    pub parent_team_id: Option<Uuid>,
    pub lead_id: Option<Uuid>,
    pub github_team_slug: Option<String>,
    pub team_type: TeamType,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub id: Uuid,
    pub github_org: String,
    pub github_repo: String,
    pub default_branch: Option<String>,
    pub primary_language: Option<String>,
    pub team_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
}
