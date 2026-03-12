use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: Uuid,
    pub name: String,
    pub org_name: String,
    pub parent_team_id: Option<Uuid>,
    pub lead_id: Option<Uuid>,
    pub github_team_slug: Option<String>,
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
