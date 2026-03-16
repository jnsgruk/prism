mod export;
pub mod github_teams;
mod identities;
mod import;
mod memberships;
mod people;
mod teams;

pub use people::ListPeopleParams;

use crate::models::TeamType;
use sqlx::PgPool;
use uuid::Uuid;

/// Repository for the `org` schema: people, teams, platform identities,
/// team memberships, and repositories.
#[derive(Clone)]
pub struct OrgRepo {
    pool: PgPool,
}

/// A team row with active member count.
pub struct TeamWithCount {
    pub id: Uuid,
    pub name: String,
    pub org_name: String,
    pub parent_team_id: Option<Uuid>,
    pub lead_id: Option<Uuid>,
    pub lead_name: Option<String>,
    pub team_type: TeamType,
    pub member_count: i32,
}

/// A person row with optional current team info.
pub struct PersonRow {
    pub id: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub level: Option<String>,
    pub active: bool,
    pub team_id: Option<Uuid>,
    pub team_name: Option<String>,
}

/// A platform identity row.
pub struct IdentityRow {
    pub person_id: Uuid,
    pub platform: String,
    pub platform_username: String,
}

/// Input for a directory import record.
///
/// Supports both JSON deserialization (for JSON file imports) and direct
/// construction (for HTML directory parsing via `ps_core::directory`).
#[derive(serde::Deserialize)]
pub struct ImportRecord {
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub directory_id: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub team_type: Option<TeamType>,
    #[serde(default)]
    pub org: Option<String>,
    #[serde(default)]
    pub identities: Vec<ImportIdentity>,
    /// Manager name (from directory HTML --manager field).
    #[serde(default)]
    pub manager_name: Option<String>,
    /// Nesting depth in the directory HTML (1 = VP, 2 = director/manager, etc.).
    #[serde(default)]
    pub depth: Option<u32>,
    /// Whether this person has direct reports in the directory tree.
    #[serde(default)]
    pub has_reports: bool,
    /// Group name from directory (e.g. "Ubuntu Engineering"), used for parent wiring.
    #[serde(default)]
    pub group: Option<String>,
}

/// A platform identity within an import record.
#[derive(serde::Deserialize)]
pub struct ImportIdentity {
    pub platform: String,
    pub username: String,
}

/// Result of a directory import operation.
pub struct ImportResult {
    pub people_imported: i32,
    pub people_updated: i32,
    pub teams_created: i32,
    pub identities_mapped: i32,
    pub warnings: Vec<String>,
    pub stale_people_count: i32,
    pub unassigned_count: i32,
}

impl OrgRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
