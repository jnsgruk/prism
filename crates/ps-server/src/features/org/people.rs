use ps_core::repo::org::ListPeopleParams;
use ps_core::repo::{PageRequest, Repos, SortParams};
use ps_proto::canonical::prism::v1::{
    AssignPersonToTeamResponse, DeactivatePersonResponse, ImportDirectoryResponse,
    ImportJiraUsersResponse, ListPeopleResponse, ListUnassignedPeopleResponse, PaginationResponse,
    ReactivatePersonResponse, RemovePersonFromTeamResponse, UpdatePersonResponse,
};
use tonic::{Response, Status};
use tracing::{info, warn};
use uuid::Uuid;

use super::conversions::build_people;
use crate::common::db_err;

pub(super) async fn handle_list_people(
    repos: &Repos,
    active_only: Option<bool>,
    search: Option<String>,
    team_id: Option<String>,
    filter: i32,
    pagination: ps_proto::canonical::prism::v1::PaginationRequest,
    sort_msg: ps_proto::canonical::prism::v1::SortOrder,
) -> Result<Response<ListPeopleResponse>, Status> {
    let filter = crate::common::person_filter_to_str(filter);
    let team_id: Option<Uuid> = team_id
        .map(|id| id.parse::<Uuid>())
        .transpose()
        .map_err(|_| Status::invalid_argument("invalid team_id"))?;

    let sort = SortParams::new(
        &sort_msg.field,
        sort_msg.descending,
        &["name", "email", "team_name", "active"],
    );

    let page_result = repos
        .org
        .list_people_paginated(ListPeopleParams {
            active_only: active_only.unwrap_or(false),
            search,
            team_id,
            filter,
            page: PageRequest::new(pagination.page_size, &pagination.page_token),
            sort,
        })
        .await
        .map_err(db_err)?;

    let person_ids: Vec<Uuid> = page_result.items.iter().map(|r| r.id).collect();
    let identities = repos
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

pub(super) async fn handle_import_directory(
    repos: &Repos,
    file_content: Vec<u8>,
) -> Result<Response<ImportDirectoryResponse>, Status> {
    let content = String::from_utf8(file_content)
        .map_err(|_| Status::invalid_argument("file content is not valid UTF-8"))?;

    let import_records = ps_core::directory::parse_file_content(&content)
        .map_err(|e| Status::invalid_argument(e.to_string()))?;

    let result = repos
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

    // Seed identity resolution rows for all configured Discourse/Jira sources.
    // This ensures new people get pending resolution entries so the next
    // resolution run picks them up automatically.
    if let Ok(sources) = repos.config.list_sources().await {
        let platforms: Vec<String> = sources
            .iter()
            .filter(|s| {
                s.enabled
                    && (s.source_type.is_discourse()
                        || s.source_type == ps_core::models::Platform::Jira)
            })
            .map(|s| s.source_type.to_string())
            .collect();

        if !platforms.is_empty() {
            match repos
                .org
                .ensure_resolution_rows_for_platforms(&platforms)
                .await
            {
                Ok(count) if count > 0 => {
                    info!(
                        count,
                        "seeded pending identity resolution rows after directory import"
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("failed to seed resolution rows: {e}");
                }
            }
        }
    }

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

pub(super) async fn handle_update_person(
    repos: &Repos,
    person_id: String,
    name: Option<String>,
    email: Option<String>,
    level: Option<String>,
) -> Result<Response<UpdatePersonResponse>, Status> {
    let id: Uuid = person_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid person_id"))?;

    let person = repos
        .org
        .update_person(id, name.as_deref(), email.as_deref(), level.as_deref())
        .await
        .map_err(db_err)?;

    let identities = repos
        .org
        .get_identities_for_people(&[person.id])
        .await
        .map_err(db_err)?;

    let people = build_people(vec![person], &identities);

    Ok(Response::new(UpdatePersonResponse {
        person: people.into_iter().next(),
    }))
}

pub(super) async fn handle_deactivate_person(
    repos: &Repos,
    person_id: String,
) -> Result<Response<DeactivatePersonResponse>, Status> {
    let id: Uuid = person_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid person_id"))?;

    repos.org.deactivate_person(id).await.map_err(db_err)?;

    Ok(Response::new(DeactivatePersonResponse {}))
}

pub(super) async fn handle_reactivate_person(
    repos: &Repos,
    person_id: String,
) -> Result<Response<ReactivatePersonResponse>, Status> {
    let id: Uuid = person_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid person_id"))?;

    repos.org.reactivate_person(id).await.map_err(db_err)?;

    Ok(Response::new(ReactivatePersonResponse {}))
}

pub(super) async fn handle_assign_person_to_team(
    repos: &Repos,
    person_id: String,
    team_id: String,
) -> Result<Response<AssignPersonToTeamResponse>, Status> {
    let person_id: Uuid = person_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid person_id"))?;
    let team_id: Uuid = team_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid team_id"))?;

    repos
        .org
        .assign_person_to_team(person_id.into(), team_id.into())
        .await
        .map_err(db_err)?;

    Ok(Response::new(AssignPersonToTeamResponse {}))
}

pub(super) async fn handle_remove_person_from_team(
    repos: &Repos,
    person_id: String,
    team_id: String,
) -> Result<Response<RemovePersonFromTeamResponse>, Status> {
    let person_id: Uuid = person_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid person_id"))?;
    let team_id: Uuid = team_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid team_id"))?;

    repos
        .org
        .remove_person_from_team(person_id.into(), team_id.into())
        .await
        .map_err(db_err)?;

    Ok(Response::new(RemovePersonFromTeamResponse {}))
}

pub(super) async fn handle_list_unassigned_people(
    repos: &Repos,
) -> Result<Response<ListUnassignedPeopleResponse>, Status> {
    let people_rows = repos.org.list_unassigned_people().await.map_err(db_err)?;

    let person_ids: Vec<Uuid> = people_rows.iter().map(|r| r.id).collect();
    let identities = repos
        .org
        .get_identities_for_people(&person_ids)
        .await
        .map_err(db_err)?;

    let people = build_people(people_rows, &identities);
    Ok(Response::new(ListUnassignedPeopleResponse { people }))
}

pub(super) async fn handle_import_jira_users(
    repos: &Repos,
    file_content: Vec<u8>,
) -> Result<Response<ImportJiraUsersResponse>, Status> {
    let content = String::from_utf8(file_content)
        .map_err(|_| Status::invalid_argument("file content is not valid UTF-8"))?;

    let (records, mut warnings) = ps_core::directory::parse_jira_user_csv(&content)
        .map_err(|e| Status::invalid_argument(e.to_string()))?;

    let (identities_mapped, unmatched_users, import_warnings) = repos
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
