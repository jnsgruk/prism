//! Thin authenticated adapters for administrator-managed people and identities.
use super::conversions::build_people;
use crate::common::{db_err, proto_to_platform_str};
use ps_core::{
    Error,
    repo::{
        Repos,
        org::{CreatePersonParams, IdentityInput, ManualPersonResult, UpdateIdentityParams},
    },
};
use ps_proto::canonical::prism::v1::{
    self as proto, update_person_identity_request::PlatformUserIdChange,
};
use tonic::{Response, Status};
use uuid::Uuid;

#[allow(clippy::result_large_err)]
fn uuid(value: &str, field: &str) -> Result<Uuid, Status> {
    value
        .parse()
        .map_err(|_| Status::invalid_argument(format!("invalid {field}")))
}

#[allow(clippy::result_large_err)]
fn identity(input: proto::IdentityInput) -> Result<IdentityInput, Status> {
    let platform = proto_to_platform_str(input.platform, input.platform_instance.as_deref())
        .ok_or_else(|| Status::invalid_argument("invalid platform or missing instance"))?
        .parse()
        .map_err(|_| Status::invalid_argument("invalid platform"))?;
    if !matches!(platform, ps_core::models::Platform::Discourse(_))
        && input.platform_instance.is_some()
    {
        return Err(Status::invalid_argument(
            "instance is only supported for Discourse",
        ));
    }
    Ok(IdentityInput {
        platform,
        username: input.username,
        platform_user_id: input.platform_user_id,
    })
}

pub(super) fn write_err(error: Error) -> Status {
    match error {
        Error::Validation(message) => Status::invalid_argument(message),
        Error::NotFound(message) => Status::not_found(message),
        Error::Conflict(message) => Status::already_exists(message),
        other => db_err(other),
    }
}

fn person(result: ManualPersonResult) -> Option<proto::Person> {
    build_people(vec![result.person], &result.identities).pop()
}

pub(super) async fn create(
    repos: &Repos,
    req: proto::CreatePersonRequest,
) -> Result<Response<proto::CreatePersonResponse>, Status> {
    let params = CreatePersonParams {
        name: req.name,
        email: req.email,
        level: req.level,
        team_id: req
            .team_id
            .map(|id| uuid(&id, "team_id").map(Into::into))
            .transpose()?,
        identities: req
            .identities
            .into_iter()
            .map(identity)
            .collect::<Result<_, _>>()?,
    };
    Ok(Response::new(proto::CreatePersonResponse {
        person: person(repos.org.create_person(params).await.map_err(write_err)?),
    }))
}

pub(super) async fn add(
    repos: &Repos,
    req: proto::AddPersonIdentityRequest,
) -> Result<Response<proto::AddPersonIdentityResponse>, Status> {
    let id = uuid(&req.person_id, "person_id")?;
    let input = identity(
        req.identity
            .ok_or_else(|| Status::invalid_argument("identity is required"))?,
    )?;
    Ok(Response::new(proto::AddPersonIdentityResponse {
        person: person(
            repos
                .org
                .add_person_identity(id.into(), input)
                .await
                .map_err(write_err)?,
        ),
    }))
}

pub(super) async fn update(
    repos: &Repos,
    req: proto::UpdatePersonIdentityRequest,
) -> Result<Response<proto::UpdatePersonIdentityResponse>, Status> {
    let platform_user_id = match req.platform_user_id_change {
        None => None,
        Some(PlatformUserIdChange::PlatformUserId(id)) => Some(Some(id)),
        Some(PlatformUserIdChange::ClearPlatformUserId(true)) => Some(None),
        Some(PlatformUserIdChange::ClearPlatformUserId(false)) => {
            return Err(Status::invalid_argument(
                "clear_platform_user_id must be true",
            ));
        }
    };
    let params = UpdateIdentityParams {
        person_id: uuid(&req.person_id, "person_id")?.into(),
        identity_id: uuid(&req.identity_id, "identity_id")?,
        username: req.username,
        platform_user_id,
    };
    Ok(Response::new(proto::UpdatePersonIdentityResponse {
        person: person(
            repos
                .org
                .update_person_identity(params)
                .await
                .map_err(write_err)?,
        ),
    }))
}

pub(super) async fn remove(
    repos: &Repos,
    req: proto::RemovePersonIdentityRequest,
) -> Result<Response<proto::RemovePersonIdentityResponse>, Status> {
    let id = uuid(&req.person_id, "person_id")?;
    let identity_id = uuid(&req.identity_id, "identity_id")?;
    Ok(Response::new(proto::RemovePersonIdentityResponse {
        person: person(
            repos
                .org
                .remove_person_identity(id.into(), identity_id)
                .await
                .map_err(write_err)?,
        ),
    }))
}
