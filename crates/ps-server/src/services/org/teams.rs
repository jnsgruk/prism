use ps_core::models::TeamType;
use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::{
    AssignGithubTeamResponse, CreateTeamResponse, DeleteTeamResponse,
    DismissTeamMappingSuggestionResponse, GetTeamMappingSuggestionsResponse, GetTeamResponse,
    GetTeamTreeResponse, ListGithubTeamsResponse, ListTeamGithubTeamsResponse, ListTeamsResponse,
    TeamMappingSuggestion as ProtoTeamMappingSuggestion, UnassignGithubTeamResponse,
    UpdateTeamResponse,
};
use tonic::{Response, Status};
use tracing::info;
use uuid::Uuid;

use super::conversions::{
    build_people, build_team_tree, github_team_to_proto, proto_to_team_type, team_to_proto,
};
use crate::services::common::db_err;

pub(super) async fn handle_list_teams(
    repos: &Repos,
    parent_team_id: Option<String>,
    team_type: Option<i32>,
) -> Result<Response<ListTeamsResponse>, Status> {
    let parent_filter: Option<Uuid> = parent_team_id
        .map(|id| id.parse::<Uuid>())
        .transpose()
        .map_err(|_| Status::invalid_argument("invalid parent_team_id"))?;

    let type_filter: Option<TeamType> = team_type.map(proto_to_team_type).transpose()?;

    let teams = repos
        .org
        .list_teams(parent_filter, type_filter)
        .await
        .map_err(db_err)?;

    let teams = teams.into_iter().map(team_to_proto).collect();
    Ok(Response::new(ListTeamsResponse { teams }))
}

pub(super) async fn handle_get_team(
    repos: &Repos,
    team_id: String,
) -> Result<Response<GetTeamResponse>, Status> {
    let team_id: Uuid = team_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid team_id"))?;

    let team = repos
        .org
        .get_team(team_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("team not found"))?;

    let member_people = repos
        .org
        .get_team_members(team_id.into())
        .await
        .map_err(db_err)?;

    let person_ids: Vec<Uuid> = member_people.iter().map(|r| r.id).collect();
    let identities = repos
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

pub(super) async fn handle_get_team_tree(
    repos: &Repos,
) -> Result<Response<GetTeamTreeResponse>, Status> {
    let all_teams = repos.org.get_all_teams().await.map_err(db_err)?;
    let roots = build_team_tree(all_teams);

    Ok(Response::new(GetTeamTreeResponse { roots }))
}

pub(super) async fn handle_create_team(
    repos: &Repos,
    name: String,
    org_name: String,
    team_type_val: i32,
    parent_team_id: Option<String>,
    lead_id: Option<String>,
) -> Result<Response<CreateTeamResponse>, Status> {
    let team_type = proto_to_team_type(team_type_val)?;
    let parent_id = parent_team_id
        .map(|id| id.parse::<Uuid>())
        .transpose()
        .map_err(|_| Status::invalid_argument("invalid parent_team_id"))?;
    let lead_id = lead_id
        .map(|id| id.parse::<Uuid>())
        .transpose()
        .map_err(|_| Status::invalid_argument("invalid lead_id"))?;

    let team = repos
        .org
        .create_team(&name, &org_name, team_type, parent_id, lead_id)
        .await
        .map_err(db_err)?;

    Ok(Response::new(CreateTeamResponse {
        team: Some(team_to_proto(team)),
    }))
}

pub(super) async fn handle_update_team(
    repos: &Repos,
    team_id: String,
    name: Option<String>,
    parent_team_id: Option<String>,
    lead_id: Option<String>,
) -> Result<Response<UpdateTeamResponse>, Status> {
    let id: Uuid = team_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid team_id"))?;
    let parent_id = parent_team_id
        .map(|id| id.parse::<Uuid>())
        .transpose()
        .map_err(|_| Status::invalid_argument("invalid parent_team_id"))?;
    let lead_id = lead_id
        .map(|id| id.parse::<Uuid>())
        .transpose()
        .map_err(|_| Status::invalid_argument("invalid lead_id"))?;

    let team = repos
        .org
        .update_team(id, name.as_deref(), parent_id, lead_id)
        .await
        .map_err(db_err)?;

    Ok(Response::new(UpdateTeamResponse {
        team: Some(team_to_proto(team)),
    }))
}

pub(super) async fn handle_delete_team(
    repos: &Repos,
    team_id: String,
) -> Result<Response<DeleteTeamResponse>, Status> {
    let id: Uuid = team_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid team_id"))?;

    repos.org.delete_team(id).await.map_err(db_err)?;

    Ok(Response::new(DeleteTeamResponse {}))
}

pub(super) async fn handle_list_github_teams(
    repos: &Repos,
    search: Option<String>,
    github_org: Option<String>,
) -> Result<Response<ListGithubTeamsResponse>, Status> {
    let search = search.filter(|s| !s.is_empty());
    let github_org = github_org.filter(|s| !s.is_empty());

    let rows = repos
        .org
        .list_github_teams(search.as_deref(), github_org.as_deref())
        .await
        .map_err(db_err)?;

    let teams = rows.into_iter().map(github_team_to_proto).collect();
    Ok(Response::new(ListGithubTeamsResponse { teams }))
}

pub(super) async fn handle_list_team_github_teams(
    repos: &Repos,
    team_id: String,
) -> Result<Response<ListTeamGithubTeamsResponse>, Status> {
    let team_id =
        Uuid::parse_str(&team_id).map_err(|_| Status::invalid_argument("invalid team_id"))?;

    let rows = repos
        .org
        .list_team_github_teams(team_id)
        .await
        .map_err(db_err)?;

    let teams = rows.into_iter().map(github_team_to_proto).collect();
    Ok(Response::new(ListTeamGithubTeamsResponse { teams }))
}

pub(super) async fn handle_assign_github_team(
    repos: &Repos,
    team_id: String,
    github_team_id: String,
) -> Result<Response<AssignGithubTeamResponse>, Status> {
    let team_id =
        Uuid::parse_str(&team_id).map_err(|_| Status::invalid_argument("invalid team_id"))?;
    let github_team_id = Uuid::parse_str(&github_team_id)
        .map_err(|_| Status::invalid_argument("invalid github_team_id"))?;

    repos
        .org
        .assign_github_team(team_id, github_team_id)
        .await
        .map_err(db_err)?;

    info!(%team_id, %github_team_id, "assigned GitHub team to Prism team");
    Ok(Response::new(AssignGithubTeamResponse {}))
}

pub(super) async fn handle_unassign_github_team(
    repos: &Repos,
    team_id: String,
    github_team_id: String,
) -> Result<Response<UnassignGithubTeamResponse>, Status> {
    let team_id =
        Uuid::parse_str(&team_id).map_err(|_| Status::invalid_argument("invalid team_id"))?;
    let github_team_id = Uuid::parse_str(&github_team_id)
        .map_err(|_| Status::invalid_argument("invalid github_team_id"))?;

    repos
        .org
        .unassign_github_team(team_id, github_team_id)
        .await
        .map_err(db_err)?;

    info!(%team_id, %github_team_id, "unassigned GitHub team from Prism team");
    Ok(Response::new(UnassignGithubTeamResponse {}))
}

pub(super) async fn handle_get_team_mapping_suggestions(
    repos: &Repos,
) -> Result<Response<GetTeamMappingSuggestionsResponse>, Status> {
    let rows = repos
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

pub(super) async fn handle_dismiss_team_mapping_suggestion(
    repos: &Repos,
    team_id: String,
    github_team_id: String,
) -> Result<Response<DismissTeamMappingSuggestionResponse>, Status> {
    let team_id =
        Uuid::parse_str(&team_id).map_err(|_| Status::invalid_argument("invalid team_id"))?;
    let github_team_id = Uuid::parse_str(&github_team_id)
        .map_err(|_| Status::invalid_argument("invalid github_team_id"))?;

    repos
        .org
        .dismiss_github_team_suggestion(team_id, github_team_id)
        .await
        .map_err(db_err)?;

    Ok(Response::new(DismissTeamMappingSuggestionResponse {}))
}
