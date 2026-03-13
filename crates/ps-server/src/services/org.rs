use std::collections::{HashMap, HashSet};

use ps_core::models::TeamType;
use ps_core::repo::org::{
    IdentityRow, ImportIdentity, ImportRecord, ListPeopleParams, PersonRow, TeamWithCount,
};
use ps_core::repo::{PageRequest, Repos, SortParams};
use ps_proto::prism::v1::org_service_server::OrgService;
use ps_proto::prism::v1::{
    AssignPersonToTeamRequest, AssignPersonToTeamResponse, CreateTeamRequest, CreateTeamResponse,
    DeactivatePersonRequest, DeactivatePersonResponse, DeleteTeamRequest, DeleteTeamResponse,
    GetTeamRequest, GetTeamResponse, GetTeamTreeRequest, GetTeamTreeResponse,
    ImportDirectoryRequest, ImportDirectoryResponse, ListPeopleRequest, ListPeopleResponse,
    ListTeamsRequest, ListTeamsResponse, ListUnassignedPeopleRequest, ListUnassignedPeopleResponse,
    PaginationResponse, Person, PlatformIdentity, ReactivatePersonRequest,
    ReactivatePersonResponse, RemovePersonFromTeamRequest, RemovePersonFromTeamResponse, Team,
    TeamType as ProtoTeamType, UpdatePersonRequest, UpdatePersonResponse, UpdateTeamRequest,
    UpdateTeamResponse,
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
                active: p.active,
                team_name: p.team_name,
                team_id: p.team_id.map(|id| id.to_string()),
            }
        })
        .collect()
}

fn team_type_to_proto(tt: TeamType) -> i32 {
    match tt {
        TeamType::Org => ProtoTeamType::Org.into(),
        TeamType::Group => ProtoTeamType::Group.into(),
        TeamType::Team => ProtoTeamType::Team.into(),
        TeamType::Squad => ProtoTeamType::Squad.into(),
    }
}

#[allow(clippy::result_large_err)]
fn proto_to_team_type(v: i32) -> Result<TeamType, Status> {
    match ProtoTeamType::try_from(v) {
        Ok(ProtoTeamType::Org) => Ok(TeamType::Org),
        Ok(ProtoTeamType::Group) => Ok(TeamType::Group),
        Ok(ProtoTeamType::Team) => Ok(TeamType::Team),
        Ok(ProtoTeamType::Squad) => Ok(TeamType::Squad),
        _ => Err(Status::invalid_argument("invalid team_type")),
    }
}

fn team_to_proto(t: TeamWithCount) -> Team {
    Team {
        id: t.id.to_string(),
        name: t.name,
        org_name: t.org_name,
        parent_team_id: t.parent_team_id.map(|id| id.to_string()),
        lead_id: t.lead_id.map(|id| id.to_string()),
        github_team_slug: t.github_team_slug,
        member_count: t.member_count,
        team_type: team_type_to_proto(t.team_type),
        total_member_count: 0,
        children: Vec::new(),
        lead_name: t.lead_name,
    }
}

/// Recursively populate a team's children and compute total member counts.
fn populate_team_tree(
    id: &str,
    proto_teams: &mut HashMap<String, Team>,
    children_map: &HashMap<String, Vec<String>>,
) -> Team {
    let child_ids: Vec<String> = children_map.get(id).cloned().unwrap_or_default();

    let children: Vec<Team> = child_ids
        .iter()
        .map(|cid| populate_team_tree(cid, proto_teams, children_map))
        .collect();

    let total: i32 = children.iter().map(|c| c.total_member_count).sum();

    let mut team = proto_teams.remove(id).unwrap_or_default();
    team.total_member_count = team.member_count + total;
    team.children = children;
    team
}

/// Build a tree of teams from a flat list, returning only root nodes.
fn build_team_tree(teams: Vec<TeamWithCount>) -> Vec<Team> {
    let mut proto_teams: HashMap<String, Team> = HashMap::new();
    let mut children_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut root_ids: Vec<String> = Vec::new();

    for t in teams {
        let id = t.id.to_string();
        let parent_id = t.parent_team_id.map(|p| p.to_string());
        proto_teams.insert(id.clone(), team_to_proto(t));

        if let Some(pid) = parent_id {
            children_map.entry(pid).or_default().push(id);
        } else {
            root_ids.push(id);
        }
    }

    root_ids
        .iter()
        .map(|id| populate_team_tree(id, &mut proto_teams, &children_map))
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
#[allow(clippy::too_many_lines)]
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

        let type_filter: Option<TeamType> = req.team_type.map(proto_to_team_type).transpose()?;

        let teams = self
            .repos
            .org
            .list_teams(parent_filter, type_filter)
            .await
            .map_err(db_err)?;

        let teams = teams.into_iter().map(team_to_proto).collect();
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

        Ok(Response::new(GetTeamResponse {
            team: Some(team_to_proto(team)),
            members,
        }))
    }

    async fn get_team_tree(
        &self,
        request: Request<GetTeamTreeRequest>,
    ) -> Result<Response<GetTeamTreeResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let all_teams = self.repos.org.get_all_teams().await.map_err(db_err)?;
        let roots = build_team_tree(all_teams);

        Ok(Response::new(GetTeamTreeResponse { roots }))
    }

    async fn create_team(
        &self,
        request: Request<CreateTeamRequest>,
    ) -> Result<Response<CreateTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_type = proto_to_team_type(req.team_type)?;
        let parent_id = req
            .parent_team_id
            .map(|id| id.parse::<Uuid>())
            .transpose()
            .map_err(|_| Status::invalid_argument("invalid parent_team_id"))?;
        let lead_id = req
            .lead_id
            .map(|id| id.parse::<Uuid>())
            .transpose()
            .map_err(|_| Status::invalid_argument("invalid lead_id"))?;

        let team = self
            .repos
            .org
            .create_team(
                &req.name,
                &req.org_name,
                team_type,
                parent_id,
                lead_id,
                req.github_team_slug.as_deref(),
            )
            .await
            .map_err(db_err)?;

        Ok(Response::new(CreateTeamResponse {
            team: Some(team_to_proto(team)),
        }))
    }

    async fn update_team(
        &self,
        request: Request<UpdateTeamRequest>,
    ) -> Result<Response<UpdateTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let id: Uuid = req
            .team_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;
        let parent_id = req
            .parent_team_id
            .map(|id| id.parse::<Uuid>())
            .transpose()
            .map_err(|_| Status::invalid_argument("invalid parent_team_id"))?;
        let lead_id = req
            .lead_id
            .map(|id| id.parse::<Uuid>())
            .transpose()
            .map_err(|_| Status::invalid_argument("invalid lead_id"))?;

        let team = self
            .repos
            .org
            .update_team(
                id,
                req.name.as_deref(),
                parent_id,
                lead_id,
                req.github_team_slug.as_deref(),
            )
            .await
            .map_err(db_err)?;

        Ok(Response::new(UpdateTeamResponse {
            team: Some(team_to_proto(team)),
        }))
    }

    async fn delete_team(
        &self,
        request: Request<DeleteTeamRequest>,
    ) -> Result<Response<DeleteTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let id: Uuid = req
            .team_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;

        self.repos.org.delete_team(id).await.map_err(db_err)?;

        Ok(Response::new(DeleteTeamResponse {}))
    }

    async fn list_people(
        &self,
        request: Request<ListPeopleRequest>,
    ) -> Result<Response<ListPeopleResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let pagination = req.pagination.unwrap_or_default();
        let sort_msg = req.sort.unwrap_or_default();

        let team_id: Option<Uuid> = req
            .team_id
            .map(|id| id.parse::<Uuid>())
            .transpose()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;

        let sort = SortParams::new(
            &sort_msg.field,
            sort_msg.descending,
            &["name", "email", "team_name", "active"],
        );

        let page_result = self
            .repos
            .org
            .list_people_paginated(ListPeopleParams {
                active_only: req.active_only.unwrap_or(false),
                search: req.search,
                team_id,
                filter: req.filter,
                page: PageRequest::new(pagination.page_size, &pagination.page_token),
                sort,
            })
            .await
            .map_err(db_err)?;

        let person_ids: Vec<Uuid> = page_result.items.iter().map(|r| r.id).collect();
        let identities = self
            .repos
            .org
            .get_identities_for_people(&person_ids)
            .await
            .map_err(db_err)?;

        let people = build_people(page_result.items, &identities);
        Ok(Response::new(ListPeopleResponse {
            people,
            pagination: Some(PaginationResponse {
                next_page_token: page_result.next_page_token.unwrap_or_default(),
                total_count: page_result.total_count as i32,
            }),
        }))
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

        let import_records: Vec<ImportRecord> = records
            .into_iter()
            .map(|r| ImportRecord {
                name: r.name,
                email: r.email,
                level: r.level,
                directory_id: r.directory_id,
                team: r.team,
                team_type: r.team_type,
                org: r.org,
                identities: r
                    .identities
                    .into_iter()
                    .map(|i| ImportIdentity {
                        platform: i.platform,
                        username: i.username,
                    })
                    .collect(),
                manager_name: r.manager_name,
                depth: r.depth,
                has_reports: r.has_reports,
                group: r.group,
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
            people_updated: result.people_updated,
            stale_people_count: result.stale_people_count,
            unassigned_count: result.unassigned_count,
        }))
    }

    async fn update_person(
        &self,
        request: Request<UpdatePersonRequest>,
    ) -> Result<Response<UpdatePersonResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let id: Uuid = req
            .person_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid person_id"))?;

        let person = self
            .repos
            .org
            .update_person(
                id,
                req.name.as_deref(),
                req.email.as_deref(),
                req.level.as_deref(),
            )
            .await
            .map_err(db_err)?;

        let identities = self
            .repos
            .org
            .get_identities_for_people(&[person.id])
            .await
            .map_err(db_err)?;

        let people = build_people(vec![person], &identities);

        Ok(Response::new(UpdatePersonResponse {
            person: people.into_iter().next(),
        }))
    }

    async fn deactivate_person(
        &self,
        request: Request<DeactivatePersonRequest>,
    ) -> Result<Response<DeactivatePersonResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let id: Uuid = req
            .person_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid person_id"))?;

        self.repos.org.deactivate_person(id).await.map_err(db_err)?;

        Ok(Response::new(DeactivatePersonResponse {}))
    }

    async fn reactivate_person(
        &self,
        request: Request<ReactivatePersonRequest>,
    ) -> Result<Response<ReactivatePersonResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let id: Uuid = req
            .person_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid person_id"))?;

        self.repos.org.reactivate_person(id).await.map_err(db_err)?;

        Ok(Response::new(ReactivatePersonResponse {}))
    }

    async fn assign_person_to_team(
        &self,
        request: Request<AssignPersonToTeamRequest>,
    ) -> Result<Response<AssignPersonToTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let person_id: Uuid = req
            .person_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid person_id"))?;
        let team_id: Uuid = req
            .team_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;

        self.repos
            .org
            .assign_person_to_team(person_id, team_id)
            .await
            .map_err(db_err)?;

        Ok(Response::new(AssignPersonToTeamResponse {}))
    }

    async fn remove_person_from_team(
        &self,
        request: Request<RemovePersonFromTeamRequest>,
    ) -> Result<Response<RemovePersonFromTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let person_id: Uuid = req
            .person_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid person_id"))?;
        let team_id: Uuid = req
            .team_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;

        self.repos
            .org
            .remove_person_from_team(person_id, team_id)
            .await
            .map_err(db_err)?;

        Ok(Response::new(RemovePersonFromTeamResponse {}))
    }

    async fn list_unassigned_people(
        &self,
        request: Request<ListUnassignedPeopleRequest>,
    ) -> Result<Response<ListUnassignedPeopleResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let people_rows = self
            .repos
            .org
            .list_unassigned_people()
            .await
            .map_err(db_err)?;

        let person_ids: Vec<Uuid> = people_rows.iter().map(|r| r.id).collect();
        let identities = self
            .repos
            .org
            .get_identities_for_people(&person_ids)
            .await
            .map_err(db_err)?;

        let people = build_people(people_rows, &identities);
        Ok(Response::new(ListUnassignedPeopleResponse { people }))
    }
}

/// Detect file format and parse into `DirectoryRecord` entries.
///
/// For HTML files, this also computes the team hierarchy from the directory
/// nesting structure: depth-1 people are group leaders, depth-2 people with
/// reports are team leaders, depth-3+ people with reports are squad leaders.
#[allow(clippy::result_large_err)]
fn parse_file_content(content: &str) -> Result<Vec<DirectoryRecord>, Status> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('<') || trimmed.starts_with("<!") {
        parse_html_to_records(content)
    } else {
        serde_json::from_str(content)
            .map_err(|e| Status::invalid_argument(format!("invalid JSON: {e}")))
    }
}

/// Parse HTML directory into `DirectoryRecord` entries with hierarchy information.
///
/// Determines which people are team/squad leaders by checking whether they have
/// reports (i.e. someone else lists them as their manager).
#[allow(clippy::result_large_err)]
fn parse_html_to_records(content: &str) -> Result<Vec<DirectoryRecord>, Status> {
    let people = parse_directory_html(content);
    if people.is_empty() {
        return Err(Status::invalid_argument(
            "no valid entries found in HTML directory file",
        ));
    }

    // Build set of people who have reports (someone names them as manager).
    let managers: HashSet<String> = people
        .iter()
        .filter_map(|p| p.manager_name.clone())
        .collect();

    Ok(people
        .into_iter()
        .map(|p| {
            let has_reports = managers.contains(&p.display_name);

            // Determine the team name this person belongs to or leads.
            // - depth 1 + has_reports → group leader, team = group name
            // - depth 2 + has_reports → team leader, team = "<name>'s Team"
            // - depth 3+ + has_reports → squad leader, team = "<name>'s Squad"
            // - depth 2+ without reports → IC, team = "<manager>'s Team" or Squad
            let (team, team_type) = derive_team_assignment(
                &p.display_name,
                p.depth,
                has_reports,
                p.group.as_ref(),
                p.manager_name.as_ref(),
            );

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
                team,
                team_type,
                org: Some("Canonical".to_owned()),
                identities,
                manager_name: p.manager_name,
                depth: Some(p.depth),
                has_reports,
                group: p.group,
            }
        })
        .collect())
}

/// Derive the team name and type for a person based on directory nesting.
fn derive_team_assignment(
    name: &str,
    depth: u32,
    has_reports: bool,
    group: Option<&String>,
    manager_name: Option<&String>,
) -> (Option<String>, Option<TeamType>) {
    match (depth, has_reports) {
        // VP / group leader or depth-2 IC — assign to group
        (1, _) | (2, false) => (group.cloned(), Some(TeamType::Group)),
        // Depth-2 with reports — team leader, auto-name from their name
        (2, true) => (Some(format!("{name}'s Team")), Some(TeamType::Team)),
        // Depth 3+ with reports — squad leader
        (_, true) => (Some(format!("{name}'s Squad")), Some(TeamType::Squad)),
        // Depth 3+ IC — assign to their manager's team/squad
        (d, false) if d >= 3 => manager_name.map_or_else(
            || (group.cloned(), Some(TeamType::Group)),
            |mgr| {
                if d == 3 {
                    // Manager is depth 2 → team
                    (Some(format!("{mgr}'s Team")), Some(TeamType::Team))
                } else {
                    // Manager is depth 3+ → squad
                    (Some(format!("{mgr}'s Squad")), Some(TeamType::Squad))
                }
            },
        ),
        _ => (None, None),
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
    team_type: Option<TeamType>,
    #[serde(default)]
    org: Option<String>,
    #[serde(default)]
    identities: Vec<DirectoryIdentity>,
    #[serde(default)]
    manager_name: Option<String>,
    #[serde(default)]
    depth: Option<u32>,
    #[serde(default)]
    has_reports: bool,
    #[serde(default)]
    group: Option<String>,
}

#[derive(Deserialize)]
struct DirectoryIdentity {
    platform: String,
    username: String,
}
