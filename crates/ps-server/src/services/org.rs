use ps_core::repo::Repos;
use ps_core::repo::org::{IdentityRow, PersonRow};
use ps_proto::prism::v1::org_service_server::OrgService;
use ps_proto::prism::v1::{
    GetTeamRequest, GetTeamResponse, ImportDirectoryRequest, ImportDirectoryResponse,
    ListPeopleRequest, ListPeopleResponse, ListTeamsRequest, ListTeamsResponse, Person,
    PlatformIdentity, Team,
};
use serde::Deserialize;
use tonic::{Request, Response, Status};
use tracing::info;
use uuid::Uuid;

use crate::directory::parse_directory_html;

use super::common::{db_err, require_auth};

/// Build `Person` proto messages from person rows + their platform identities.
fn build_people(people: Vec<PersonRow>, identities: &[IdentityRow]) -> Vec<Person> {
    people
        .into_iter()
        .map(|p| {
            let person_identities: Vec<PlatformIdentity> = identities
                .iter()
                .filter(|i| i.person_id == p.id)
                .map(|i| PlatformIdentity {
                    platform: i.platform.clone(),
                    username: i.platform_username.clone(),
                })
                .collect();

            Person {
                id: p.id.to_string(),
                name: p.name,
                email: p.email,
                level: p.level,
                identities: person_identities,
            }
        })
        .collect()
}

pub struct OrgServiceImpl {
    repos: Repos,
}

impl OrgServiceImpl {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }
}

#[tonic::async_trait]
#[allow(clippy::too_many_lines)] // sqlx query macros need inline usage for offline type inference
impl OrgService for OrgServiceImpl {
    async fn list_teams(
        &self,
        request: Request<ListTeamsRequest>,
    ) -> Result<Response<ListTeamsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let parent_filter: Option<Uuid> = req
            .parent_team_id
            .map(|id| id.parse::<Uuid>())
            .transpose()
            .map_err(|_| Status::invalid_argument("invalid parent_team_id"))?;

        let teams = self
            .repos
            .org
            .list_teams(parent_filter)
            .await
            .map_err(db_err)?;

        let teams = teams
            .into_iter()
            .map(|t| Team {
                id: t.id.to_string(),
                name: t.name,
                org_name: t.org_name,
                parent_team_id: t.parent_team_id.map(|id| id.to_string()),
                lead_id: t.lead_id.map(|id| id.to_string()),
                github_team_slug: t.github_team_slug,
                member_count: t.member_count,
            })
            .collect();

        Ok(Response::new(ListTeamsResponse { teams }))
    }

    async fn get_team(
        &self,
        request: Request<GetTeamRequest>,
    ) -> Result<Response<GetTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_id: Uuid = req
            .team_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;

        let team = self
            .repos
            .org
            .get_team(team_id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("team not found"))?;

        let member_people = self
            .repos
            .org
            .get_team_members(team_id)
            .await
            .map_err(db_err)?;

        let person_ids: Vec<Uuid> = member_people.iter().map(|r| r.id).collect();

        let identities = self
            .repos
            .org
            .get_identities_for_people(&person_ids)
            .await
            .map_err(db_err)?;

        let members = build_people(member_people, &identities);

        let team_proto = Team {
            id: team.id.to_string(),
            name: team.name,
            org_name: team.org_name,
            parent_team_id: team.parent_team_id.map(|id| id.to_string()),
            lead_id: team.lead_id.map(|id| id.to_string()),
            github_team_slug: team.github_team_slug,
            member_count: team.member_count,
        };

        Ok(Response::new(GetTeamResponse {
            team: Some(team_proto),
            members,
        }))
    }

    async fn list_people(
        &self,
        request: Request<ListPeopleRequest>,
    ) -> Result<Response<ListPeopleResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let people_rows = self.repos.org.list_people().await.map_err(db_err)?;

        let person_ids: Vec<Uuid> = people_rows.iter().map(|r| r.id).collect();

        let identities = self
            .repos
            .org
            .get_identities_for_people(&person_ids)
            .await
            .map_err(db_err)?;

        let people = build_people(people_rows, &identities);

        Ok(Response::new(ListPeopleResponse { people }))
    }

    async fn import_directory(
        &self,
        request: Request<ImportDirectoryRequest>,
    ) -> Result<Response<ImportDirectoryResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let content = String::from_utf8(req.file_content)
            .map_err(|_| Status::invalid_argument("file content is not valid UTF-8"))?;

        let records = parse_file_content(&content)?;

        let import_records: Vec<ps_core::repo::org::ImportRecord> = records
            .into_iter()
            .map(|r| ps_core::repo::org::ImportRecord {
                name: r.name,
                email: r.email,
                level: r.level,
                directory_id: r.directory_id,
                team: r.team,
                org: r.org,
                identities: r
                    .identities
                    .into_iter()
                    .map(|i| ps_core::repo::org::ImportIdentity {
                        platform: i.platform,
                        username: i.username,
                    })
                    .collect(),
            })
            .collect();

        let result = self
            .repos
            .org
            .import_records(&import_records)
            .await
            .map_err(db_err)?;

        info!(
            people_imported = result.people_imported,
            teams_created = result.teams_created,
            identities_mapped = result.identities_mapped,
            warnings = result.warnings.len(),
            "directory import complete"
        );

        Ok(Response::new(ImportDirectoryResponse {
            people_imported: result.people_imported,
            teams_created: result.teams_created,
            identities_mapped: result.identities_mapped,
            warnings: result.warnings,
        }))
    }
}

/// Detect file format and parse into `DirectoryRecord` entries.
#[allow(clippy::result_large_err)]
fn parse_file_content(content: &str) -> Result<Vec<DirectoryRecord>, Status> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('<') || trimmed.starts_with("<!") {
        // HTML: parse Canonical staff directory format
        let people = parse_directory_html(content);
        if people.is_empty() {
            return Err(Status::invalid_argument(
                "no valid entries found in HTML directory file",
            ));
        }
        Ok(people
            .into_iter()
            .map(|p| {
                let mut identities = vec![
                    DirectoryIdentity {
                        platform: "github".to_owned(),
                        username: p.github_username,
                    },
                    DirectoryIdentity {
                        platform: "launchpad".to_owned(),
                        username: p.launchpad_username,
                    },
                ];
                if let Some(mm) = p.mattermost_username {
                    identities.push(DirectoryIdentity {
                        platform: "mattermost".to_owned(),
                        username: mm,
                    });
                }
                DirectoryRecord {
                    name: p.display_name,
                    email: Some(p.email),
                    level: p.title,
                    directory_id: None,
                    team: p.group,
                    org: Some("Canonical".to_owned()),
                    identities,
                }
            })
            .collect())
    } else {
        // JSON
        serde_json::from_str(content)
            .map_err(|e| Status::invalid_argument(format!("invalid JSON: {e}")))
    }
}

/// A single record in a directory import file.
#[derive(Deserialize)]
struct DirectoryRecord {
    name: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    directory_id: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    org: Option<String>,
    #[serde(default)]
    identities: Vec<DirectoryIdentity>,
}

#[derive(Deserialize)]
struct DirectoryIdentity {
    platform: String,
    username: String,
}
