use std::collections::HashMap;

use ps_core::models::TeamType;
use ps_core::repo::org::{
    IdentityRow, ListPeopleParams, PersonRow, TeamWithCount, github_teams::GitHubTeamRow,
};
use ps_core::repo::{PageRequest, Repos, SortParams};
use ps_proto::prism::v1::org_service_server::OrgService;
use ps_proto::prism::v1::{
    AssignGithubTeamRequest, AssignGithubTeamResponse, AssignPersonToTeamRequest,
    AssignPersonToTeamResponse, CreateTeamRequest, CreateTeamResponse, DeactivatePersonRequest,
    DeactivatePersonResponse, DeleteTeamRequest, DeleteTeamResponse,
    DismissTeamMappingSuggestionRequest, DismissTeamMappingSuggestionResponse,
    GetTeamMappingSuggestionsRequest, GetTeamMappingSuggestionsResponse, GetTeamRequest,
    GetTeamResponse, GetTeamTreeRequest, GetTeamTreeResponse, GitHubTeam as ProtoGitHubTeam,
    ImportDirectoryRequest, ImportDirectoryResponse, ImportJiraUsersRequest,
    ImportJiraUsersResponse, ListGithubTeamsRequest, ListGithubTeamsResponse, ListPeopleRequest,
    ListPeopleResponse, ListTeamGithubTeamsRequest, ListTeamGithubTeamsResponse, ListTeamsRequest,
    ListTeamsResponse, ListUnassignedPeopleRequest, ListUnassignedPeopleResponse,
    PaginationResponse, Person, PlatformIdentity, ReactivatePersonRequest,
    ReactivatePersonResponse, RemovePersonFromTeamRequest, RemovePersonFromTeamResponse, Team,
    TeamMappingSuggestion as ProtoTeamMappingSuggestion, TeamType as ProtoTeamType,
    UnassignGithubTeamRequest, UnassignGithubTeamResponse, UpdatePersonRequest,
    UpdatePersonResponse, UpdateTeamRequest, UpdateTeamResponse,
};
use tonic::{Request, Response, Status};
use tracing::info;
use uuid::Uuid;

use super::common::{db_err, require_auth};

/// Build `Person` proto messages from person rows + their platform identities.
fn build_people(people: Vec<PersonRow>, identities: &[IdentityRow]) -> Vec<Person> {
    // Index identities by person_id for O(N+M) instead of O(N*M) lookup.
    let mut identity_map: HashMap<Uuid, Vec<&IdentityRow>> = HashMap::new();
    for i in identities {
        identity_map.entry(i.person_id).or_default().push(i);
    }

    people
        .into_iter()
        .map(|p| {
            let person_identities: Vec<PlatformIdentity> = identity_map
                .get(&p.id)
                .map(|ids| {
                    ids.iter()
                        .map(|i| PlatformIdentity {
                            platform: i.platform.clone(),
                            username: i.platform_username.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();

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

fn github_team_to_proto(t: GitHubTeamRow) -> ProtoGitHubTeam {
    ProtoGitHubTeam {
        id: t.id.to_string(),
        source_id: t.source_id.to_string(),
        github_org: t.github_org,
        github_team_id: t.github_team_id,
        slug: t.slug,
        name: t.name,
        description: t.description,
        member_count: t.member_count,
        repo_count: t.repo_count,
    }
}

fn team_to_proto(t: TeamWithCount) -> Team {
    Team {
        id: t.id.to_string(),
        name: t.name,
        org_name: t.org_name,
        parent_team_id: t.parent_team_id.map(|id| id.to_string()),
        lead_id: t.lead_id.map(|id| id.to_string()),
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
            .create_team(&req.name, &req.org_name, team_type, parent_id, lead_id)
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
            .update_team(id, req.name.as_deref(), parent_id, lead_id)
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

        let import_records = ps_core::directory::parse_file_content(&content)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

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

    async fn list_github_teams(
        &self,
        request: Request<ListGithubTeamsRequest>,
    ) -> Result<Response<ListGithubTeamsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let search = req.search.filter(|s| !s.is_empty());
        let github_org = req.github_org.filter(|s| !s.is_empty());

        let rows = self
            .repos
            .org
            .list_github_teams(search.as_deref(), github_org.as_deref())
            .await
            .map_err(db_err)?;

        let teams = rows.into_iter().map(github_team_to_proto).collect();
        Ok(Response::new(ListGithubTeamsResponse { teams }))
    }

    async fn list_team_github_teams(
        &self,
        request: Request<ListTeamGithubTeamsRequest>,
    ) -> Result<Response<ListTeamGithubTeamsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_id = Uuid::parse_str(&req.team_id)
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;

        let rows = self
            .repos
            .org
            .list_team_github_teams(team_id)
            .await
            .map_err(db_err)?;

        let teams = rows.into_iter().map(github_team_to_proto).collect();
        Ok(Response::new(ListTeamGithubTeamsResponse { teams }))
    }

    async fn assign_github_team(
        &self,
        request: Request<AssignGithubTeamRequest>,
    ) -> Result<Response<AssignGithubTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_id = Uuid::parse_str(&req.team_id)
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;
        let github_team_id = Uuid::parse_str(&req.github_team_id)
            .map_err(|_| Status::invalid_argument("invalid github_team_id"))?;

        self.repos
            .org
            .assign_github_team(team_id, github_team_id)
            .await
            .map_err(db_err)?;

        info!(%team_id, %github_team_id, "assigned GitHub team to Prism team");
        Ok(Response::new(AssignGithubTeamResponse {}))
    }

    async fn unassign_github_team(
        &self,
        request: Request<UnassignGithubTeamRequest>,
    ) -> Result<Response<UnassignGithubTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_id = Uuid::parse_str(&req.team_id)
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;
        let github_team_id = Uuid::parse_str(&req.github_team_id)
            .map_err(|_| Status::invalid_argument("invalid github_team_id"))?;

        self.repos
            .org
            .unassign_github_team(team_id, github_team_id)
            .await
            .map_err(db_err)?;

        info!(%team_id, %github_team_id, "unassigned GitHub team from Prism team");
        Ok(Response::new(UnassignGithubTeamResponse {}))
    }

    async fn get_team_mapping_suggestions(
        &self,
        request: Request<GetTeamMappingSuggestionsRequest>,
    ) -> Result<Response<GetTeamMappingSuggestionsResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let rows = self
            .repos
            .org
            .get_team_mapping_suggestions()
            .await
            .map_err(db_err)?;

        let suggestions = rows
            .into_iter()
            .map(|s| ProtoTeamMappingSuggestion {
                github_team_id: s.github_team_id.to_string(),
                github_team_name: s.github_team_name,
                github_org: s.github_org,
                github_team_slug: s.github_team_slug,
                prism_team_id: s.prism_team_id.to_string(),
                prism_team_name: s.prism_team_name,
                overlap_count: s.overlap_count,
                github_coverage: s.github_coverage,
                prism_coverage: s.prism_coverage,
            })
            .collect();

        Ok(Response::new(GetTeamMappingSuggestionsResponse {
            suggestions,
        }))
    }

    async fn dismiss_team_mapping_suggestion(
        &self,
        request: Request<DismissTeamMappingSuggestionRequest>,
    ) -> Result<Response<DismissTeamMappingSuggestionResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_id = Uuid::parse_str(&req.team_id)
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;
        let github_team_id = Uuid::parse_str(&req.github_team_id)
            .map_err(|_| Status::invalid_argument("invalid github_team_id"))?;

        self.repos
            .org
            .dismiss_github_team_suggestion(team_id, github_team_id)
            .await
            .map_err(db_err)?;

        Ok(Response::new(DismissTeamMappingSuggestionResponse {}))
    }

    async fn import_jira_users(
        &self,
        request: Request<ImportJiraUsersRequest>,
    ) -> Result<Response<ImportJiraUsersResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let content = String::from_utf8(req.file_content)
            .map_err(|_| Status::invalid_argument("file content is not valid UTF-8"))?;

        let (records, mut warnings) = ps_core::directory::parse_jira_user_csv(&content)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let (identities_mapped, unmatched_users, import_warnings) = self
            .repos
            .org
            .import_jira_users(&records)
            .await
            .map_err(db_err)?;

        warnings.extend(import_warnings);

        info!(
            identities_mapped,
            unmatched_users,
            warnings = warnings.len(),
            "Jira user import complete"
        );

        Ok(Response::new(ImportJiraUsersResponse {
            identities_mapped,
            unmatched_users,
            warnings,
        }))
    }
}
