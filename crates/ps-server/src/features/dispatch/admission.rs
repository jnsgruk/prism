use ps_core::ingestion::{
    IdentitySnapshot, PipelineRequest, PipelineScope, ProcessingScope, SelectedSource,
};
use ps_core::models::{PersonId, Platform};
use ps_proto::canonical::prism::v1::TriggerPipelineRequest;
use tonic::Status;
use uuid::Uuid;

use super::HandlersServiceImpl;
use crate::common::db_err;

impl HandlersServiceImpl {
    pub(crate) async fn resolve_pipeline_request(
        &self,
        request: &TriggerPipelineRequest,
    ) -> Result<PipelineRequest, Status> {
        let started = time::OffsetDateTime::now_utc();
        let scope = match &request.scope {
            Some(scope) => PipelineScope::Person {
                person_id: PersonId::from(
                    scope
                        .person_id
                        .parse::<Uuid>()
                        .map_err(|_| Status::invalid_argument("invalid person_id"))?,
                ),
            },
            None => PipelineScope::All,
        };
        let mut sources = self.repos.config.list_sources().await.map_err(db_err)?;
        let mut identities = Vec::new();
        if let Some(person_id) = scope.person_id() {
            identities = self
                .repos
                .org
                .get_active_person_backfill_identities(*person_id.as_uuid())
                .await
                .map_err(db_err)?
                .ok_or_else(|| Status::failed_precondition("person must exist and be active"))?;
            let selected = request
                .scope
                .as_ref()
                .map(|scope| &scope.source_ids)
                .ok_or_else(|| Status::invalid_argument("person scope is required"))?;
            if selected.is_empty() {
                return Err(Status::invalid_argument("select at least one source"));
            }
            let ids: Result<std::collections::HashSet<Uuid>, _> =
                selected.iter().map(|id| id.parse()).collect();
            let ids = ids.map_err(|_| Status::invalid_argument("invalid source_id"))?;
            if ids.len() != selected.len() {
                return Err(Status::invalid_argument("duplicate source_ids"));
            }
            sources.retain(|source| ids.contains(source.id.as_uuid()));
            if sources.len() != ids.len() || sources.iter().any(|source| !source.enabled) {
                return Err(Status::failed_precondition(
                    "selected sources must exist and be enabled",
                ));
            }
            if request.since_date.is_none() {
                return Err(Status::invalid_argument(
                    "person backfill requires since_date",
                ));
            }
        } else {
            sources.retain(|source| {
                source.enabled
                    && matches!(
                        source.source_type,
                        Platform::Github | Platform::Jira | Platform::Discourse(_)
                    )
            });
        }

        let mut selected_sources = Vec::new();
        for source in sources {
            if !matches!(
                source.source_type,
                Platform::Github | Platform::Jira | Platform::Discourse(_)
            ) {
                return Err(Status::failed_precondition(
                    "source does not support backfill",
                ));
            }
            let identity = if let Some(person_id) = scope.person_id() {
                if source.source_type == Platform::Jira
                    && source
                        .settings
                        .get("api_mode")
                        .is_some_and(|mode| mode.as_str() != Some("cloud"))
                {
                    return Err(Status::failed_precondition(
                        "person backfill supports Jira Cloud only",
                    ));
                }
                let matches: Vec<_> = identities
                    .iter()
                    .filter(|identity| identity.platform == source.source_type.to_string())
                    .collect();
                if matches.len() != 1 {
                    return Err(Status::failed_precondition(
                        "source requires exactly one saved identity for this person and instance",
                    ));
                }
                let identity = matches
                    .first()
                    .ok_or_else(|| Status::failed_precondition("saved identity is required"))?;
                Some(IdentitySnapshot {
                    identity_id: identity.id,
                    person_id,
                    platform: source.source_type.clone(),
                    username: identity.platform_username.clone().into(),
                    platform_user_id: identity.platform_user_id.clone(),
                })
            } else {
                None
            };
            selected_sources.push(SelectedSource {
                source_id: source.id,
                source_name: source.name,
                platform: source.source_type,
                identity,
            });
        }
        let processing =
            scope
                .person_id()
                .map_or(ProcessingScope::All, |person_id| ProcessingScope::Person {
                    person_id,
                });
        let resolved = PipelineRequest {
            scope,
            sources: selected_sources,
            since_date: request.since_date.clone(),
            run_started_at: started,
            processing,
        };
        resolved
            .validate()
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        Ok(resolved)
    }
}

pub(crate) fn matches_submission(
    pipeline: &ps_core::models::Pipeline,
    request: &TriggerPipelineRequest,
    caller_id: Uuid,
) -> bool {
    let Ok(snapshot) = serde_json::from_value::<PipelineRequest>(pipeline.request_snapshot.clone())
    else {
        return false;
    };
    if pipeline.requested_by != Some(caller_id) || snapshot.since_date != request.since_date {
        return false;
    }
    match (&snapshot.scope, &request.scope) {
        (PipelineScope::All, None) => true,
        (PipelineScope::Person { .. }, Some(scope)) => matches_person_selection(&snapshot, scope),
        _ => false,
    }
}

fn matches_person_selection(
    snapshot: &PipelineRequest,
    scope: &ps_proto::canonical::prism::v1::PersonBackfillScope,
) -> bool {
    let Ok(person_id) = scope.person_id.parse::<Uuid>() else {
        return false;
    };
    if snapshot.scope.person_id().map(|id| *id.as_uuid()) != Some(person_id) {
        return false;
    }
    let requested: Result<std::collections::HashSet<Uuid>, _> =
        scope.source_ids.iter().map(|id| id.parse()).collect();
    let Ok(requested) = requested else {
        return false;
    };
    let frozen: std::collections::HashSet<Uuid> = snapshot
        .sources
        .iter()
        .map(|source| *source.source_id.as_uuid())
        .collect();
    requested.len() == scope.source_ids.len() && frozen == requested
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_identity_matching_uses_uuid_values() {
        let person_id = Uuid::now_v7();
        let source_id = Uuid::now_v7();
        let snapshot = PipelineRequest {
            scope: PipelineScope::Person {
                person_id: person_id.into(),
            },
            sources: vec![SelectedSource {
                source_id: source_id.into(),
                source_name: "Saved source".into(),
                platform: Platform::Github,
                identity: None,
            }],
            since_date: Some("2020-01-01".into()),
            run_started_at: time::OffsetDateTime::now_utc(),
            processing: ProcessingScope::Person {
                person_id: person_id.into(),
            },
        };
        let mut scope = ps_proto::canonical::prism::v1::PersonBackfillScope {
            person_id: person_id.simple().to_string().to_uppercase(),
            source_ids: vec![source_id.to_string().to_uppercase()],
        };
        assert!(matches_person_selection(&snapshot, &scope));
        scope.source_ids.push(source_id.simple().to_string());
        assert!(!matches_person_selection(&snapshot, &scope));
        scope.source_ids = vec!["invalid".into()];
        assert!(!matches_person_selection(&snapshot, &scope));
    }
}
