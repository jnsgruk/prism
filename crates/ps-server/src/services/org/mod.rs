mod conversions;
mod people;
mod teams;

use ps_core::repo::Repos;
use ps_proto::prism::v1::org_service_server::OrgService;
use ps_proto::prism::v1::{
    AssignGithubTeamRequest, AssignGithubTeamResponse, AssignPersonToTeamRequest,
    AssignPersonToTeamResponse, CreateTeamRequest, CreateTeamResponse, DeactivatePersonRequest,
    DeactivatePersonResponse, DeleteTeamRequest, DeleteTeamResponse,
    DismissTeamMappingSuggestionRequest, DismissTeamMappingSuggestionResponse,
    GetTeamMappingSuggestionsRequest, GetTeamMappingSuggestionsResponse, GetTeamRequest,
    GetTeamResponse, GetTeamTreeRequest, GetTeamTreeResponse, ImportDirectoryRequest,
    ImportDirectoryResponse, ImportJiraUsersRequest, ImportJiraUsersResponse,
    ListGithubTeamsRequest, ListGithubTeamsResponse, ListPeopleRequest, ListPeopleResponse,
    ListTeamGithubTeamsRequest, ListTeamGithubTeamsResponse, ListTeamsRequest, ListTeamsResponse,
    ListUnassignedPeopleRequest, ListUnassignedPeopleResponse, ReactivatePersonRequest,
    ReactivatePersonResponse, RemovePersonFromTeamRequest, RemovePersonFromTeamResponse,
    UnassignGithubTeamRequest, UnassignGithubTeamResponse, UpdatePersonRequest,
    UpdatePersonResponse, UpdateTeamRequest, UpdateTeamResponse,
};
use tonic::{Request, Response, Status};

use super::common::require_auth;

pub struct OrgServiceImpl {
    repos: Repos,
}

impl OrgServiceImpl {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }
}

#[tonic::async_trait]
impl OrgService for OrgServiceImpl {
    async fn list_teams(
        &self,
        request: Request<ListTeamsRequest>,
    ) -> Result<Response<ListTeamsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        teams::handle_list_teams(&self.repos, req.parent_team_id, req.team_type).await
    }

    async fn get_team(
        &self,
        request: Request<GetTeamRequest>,
    ) -> Result<Response<GetTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        teams::handle_get_team(&self.repos, req.team_id).await
    }

    async fn get_team_tree(
        &self,
        request: Request<GetTeamTreeRequest>,
    ) -> Result<Response<GetTeamTreeResponse>, Status> {
        let _ctx = require_auth(&request)?;
        teams::handle_get_team_tree(&self.repos).await
    }

    async fn create_team(
        &self,
        request: Request<CreateTeamRequest>,
    ) -> Result<Response<CreateTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        teams::handle_create_team(
            &self.repos,
            req.name,
            req.org_name,
            req.team_type,
            req.parent_team_id,
            req.lead_id,
        )
        .await
    }

    async fn update_team(
        &self,
        request: Request<UpdateTeamRequest>,
    ) -> Result<Response<UpdateTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        teams::handle_update_team(
            &self.repos,
            req.team_id,
            req.name,
            req.parent_team_id,
            req.lead_id,
        )
        .await
    }

    async fn delete_team(
        &self,
        request: Request<DeleteTeamRequest>,
    ) -> Result<Response<DeleteTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        teams::handle_delete_team(&self.repos, req.team_id).await
    }

    async fn list_people(
        &self,
        request: Request<ListPeopleRequest>,
    ) -> Result<Response<ListPeopleResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        let pagination = req.pagination.unwrap_or_default();
        let sort_msg = req.sort.unwrap_or_default();
        people::handle_list_people(
            &self.repos,
            req.active_only,
            req.search,
            req.team_id,
            req.filter,
            pagination,
            sort_msg,
        )
        .await
    }

    async fn import_directory(
        &self,
        request: Request<ImportDirectoryRequest>,
    ) -> Result<Response<ImportDirectoryResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        people::handle_import_directory(&self.repos, req.file_content).await
    }

    async fn update_person(
        &self,
        request: Request<UpdatePersonRequest>,
    ) -> Result<Response<UpdatePersonResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        people::handle_update_person(&self.repos, req.person_id, req.name, req.email, req.level)
            .await
    }

    async fn deactivate_person(
        &self,
        request: Request<DeactivatePersonRequest>,
    ) -> Result<Response<DeactivatePersonResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        people::handle_deactivate_person(&self.repos, req.person_id).await
    }

    async fn reactivate_person(
        &self,
        request: Request<ReactivatePersonRequest>,
    ) -> Result<Response<ReactivatePersonResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        people::handle_reactivate_person(&self.repos, req.person_id).await
    }

    async fn assign_person_to_team(
        &self,
        request: Request<AssignPersonToTeamRequest>,
    ) -> Result<Response<AssignPersonToTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        people::handle_assign_person_to_team(&self.repos, req.person_id, req.team_id).await
    }

    async fn remove_person_from_team(
        &self,
        request: Request<RemovePersonFromTeamRequest>,
    ) -> Result<Response<RemovePersonFromTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        people::handle_remove_person_from_team(&self.repos, req.person_id, req.team_id).await
    }

    async fn list_unassigned_people(
        &self,
        request: Request<ListUnassignedPeopleRequest>,
    ) -> Result<Response<ListUnassignedPeopleResponse>, Status> {
        let _ctx = require_auth(&request)?;
        people::handle_list_unassigned_people(&self.repos).await
    }

    async fn list_github_teams(
        &self,
        request: Request<ListGithubTeamsRequest>,
    ) -> Result<Response<ListGithubTeamsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        teams::handle_list_github_teams(&self.repos, req.search, req.github_org).await
    }

    async fn list_team_github_teams(
        &self,
        request: Request<ListTeamGithubTeamsRequest>,
    ) -> Result<Response<ListTeamGithubTeamsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        teams::handle_list_team_github_teams(&self.repos, req.team_id).await
    }

    async fn assign_github_team(
        &self,
        request: Request<AssignGithubTeamRequest>,
    ) -> Result<Response<AssignGithubTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        teams::handle_assign_github_team(&self.repos, req.team_id, req.github_team_id).await
    }

    async fn unassign_github_team(
        &self,
        request: Request<UnassignGithubTeamRequest>,
    ) -> Result<Response<UnassignGithubTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        teams::handle_unassign_github_team(&self.repos, req.team_id, req.github_team_id).await
    }

    async fn get_team_mapping_suggestions(
        &self,
        request: Request<GetTeamMappingSuggestionsRequest>,
    ) -> Result<Response<GetTeamMappingSuggestionsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        teams::handle_get_team_mapping_suggestions(&self.repos).await
    }

    async fn dismiss_team_mapping_suggestion(
        &self,
        request: Request<DismissTeamMappingSuggestionRequest>,
    ) -> Result<Response<DismissTeamMappingSuggestionResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        teams::handle_dismiss_team_mapping_suggestion(&self.repos, req.team_id, req.github_team_id)
            .await
    }

    async fn import_jira_users(
        &self,
        request: Request<ImportJiraUsersRequest>,
    ) -> Result<Response<ImportJiraUsersResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();
        people::handle_import_jira_users(&self.repos, req.file_content).await
    }
}
