//! Durable, orchestrator-independent ingestion contracts.
//!
//! API callers select saved person/source IDs. Admission resolves usernames and
//! platform account IDs from saved identities and freezes them for the run.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime, format_description};
use uuid::Uuid;

use crate::Error;
use crate::models::{PersonId, Platform, PlatformUsername, SourceId};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PipelineScope {
    #[default]
    All,
    Person {
        person_id: PersonId,
    },
}

impl PipelineScope {
    pub fn person_id(&self) -> Option<PersonId> {
        match self {
            Self::All => None,
            Self::Person { person_id } => Some(*person_id),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Person { .. } => "person",
        }
    }
}

/// Downstream work must honor the same person boundary as ingestion.
/// Scoped processing is deliberately an explicit contract for future handlers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProcessingScope {
    #[default]
    All,
    Person {
        person_id: PersonId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentitySnapshot {
    pub identity_id: Uuid,
    pub person_id: PersonId,
    /// Includes the specific instance for Discourse identities.
    pub platform: Platform,
    pub username: PlatformUsername,
    /// Stable account ID, required for Jira Cloud. Never a display name.
    pub platform_user_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedSource {
    /// Configuration ID, distinct from its platform or mutable display name.
    pub source_id: SourceId,
    pub source_name: String,
    pub platform: Platform,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<IdentitySnapshot>,
}

impl SelectedSource {
    pub fn validate(&self, scope: &PipelineScope) -> Result<(), Error> {
        if self.source_id.as_uuid().is_nil() || self.source_name.trim().is_empty() {
            return Err(Error::Validation("invalid selected source".into()));
        }
        if let Platform::Discourse(instance) = &self.platform
            && instance.trim().is_empty()
        {
            return Err(Error::Validation("Discourse instance is required".into()));
        }

        let Some(person_id) = scope.person_id() else {
            return Ok(());
        };
        let identity = self.identity.as_ref().ok_or_else(|| {
            Error::Validation("person source requires a saved identity snapshot".into())
        })?;
        if person_id.as_uuid().is_nil()
            || identity.identity_id.is_nil()
            || identity.person_id != person_id
            || identity.platform != self.platform
        {
            return Err(Error::Validation(
                "identity does not match person/source".into(),
            ));
        }
        if identity.username.trim().is_empty() {
            return Err(Error::Validation("canonical username is required".into()));
        }
        if self.platform == Platform::Jira
            && identity
                .platform_user_id
                .as_deref()
                .is_none_or(|id| id.trim().is_empty())
        {
            return Err(Error::Validation(
                "Jira Cloud account ID is required".into(),
            ));
        }
        Ok(())
    }
}

/// Input for a new versioned workflow entrypoint. Legacy `Option<String>`
/// workflow arguments remain unchanged so existing journals can replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineRequest {
    #[serde(default)]
    pub scope: PipelineScope,
    #[serde(default)]
    pub sources: Vec<SelectedSource>,
    /// YYYY-MM-DD in UTC. Required for Person; optional for All incremental runs.
    #[serde(default)]
    pub since_date: Option<String>,
    /// Frozen eligibility boundary. Discovery uses updated timestamps rather
    /// than imposing a created-at cutoff on parent PRs/topics.
    pub run_started_at: OffsetDateTime,
    #[serde(default)]
    pub processing: ProcessingScope,
}

impl PipelineRequest {
    pub fn validate(&self) -> Result<(), Error> {
        validate_dates(&self.scope, self.since_date.as_deref(), self.run_started_at)?;
        validate_processing(&self.scope, &self.processing)?;
        if self.scope.person_id().is_some() && self.sources.is_empty() {
            return Err(Error::Validation(
                "person backfill requires selected sources".into(),
            ));
        }
        let mut ids = HashSet::new();
        for source in &self.sources {
            source.validate(&self.scope)?;
            if !ids.insert(source.source_id) {
                return Err(Error::Validation("duplicate selected source".into()));
            }
        }
        Ok(())
    }
}

/// Per-source snapshot carried through handler, chunk, and adapter boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRunContext {
    pub pipeline_id: Uuid,
    #[serde(default)]
    pub scope: PipelineScope,
    pub source: SelectedSource,
    #[serde(default)]
    pub since_date: Option<String>,
    pub run_started_at: OffsetDateTime,
    #[serde(default)]
    pub processing: ProcessingScope,
}

impl SourceRunContext {
    pub fn validate(&self) -> Result<(), Error> {
        if self.pipeline_id.is_nil() {
            return Err(Error::Validation("pipeline ID is required".into()));
        }
        validate_dates(&self.scope, self.since_date.as_deref(), self.run_started_at)?;
        validate_processing(&self.scope, &self.processing)?;
        self.source.validate(&self.scope)
    }

    /// Contribution event time, not the discovery timestamp of its container.
    /// Reviews use submittedAt; Jira uses issue updated time/current assignee;
    /// Discourse uses each post/topic/like action time.
    pub fn contains_event(&self, event_time: OffsetDateTime) -> Result<bool, Error> {
        let lower = self
            .since_date
            .as_deref()
            .map(parse_since_date)
            .transpose()?;
        Ok(event_time <= self.run_started_at
            && lower.is_none_or(|date| event_time >= date.midnight().assume_utc()))
    }
}

pub fn parse_since_date(value: &str) -> Result<Date, Error> {
    let format = format_description::parse_borrowed::<2>("[year]-[month]-[day]")
        .map_err(|error| Error::Internal(error.to_string()))?;
    let date = Date::parse(value, &format)
        .map_err(|_| Error::Validation("since_date must be YYYY-MM-DD".into()))?;
    if date.to_string() != value {
        return Err(Error::Validation("since_date must be YYYY-MM-DD".into()));
    }
    Ok(date)
}

fn validate_dates(
    scope: &PipelineScope,
    since: Option<&str>,
    boundary: OffsetDateTime,
) -> Result<(), Error> {
    if scope.person_id().is_some() && since.is_none() {
        return Err(Error::Validation(
            "person backfill requires since_date".into(),
        ));
    }
    if let Some(value) = since
        && parse_since_date(value)?.midnight().assume_utc() > boundary
    {
        return Err(Error::Validation(
            "since_date must not be after the run boundary".into(),
        ));
    }
    Ok(())
}

fn validate_processing(scope: &PipelineScope, processing: &ProcessingScope) -> Result<(), Error> {
    match (scope, processing) {
        (PipelineScope::All, ProcessingScope::All) => Ok(()),
        (
            PipelineScope::Person { person_id },
            ProcessingScope::Person {
                person_id: processing_person,
            },
        ) if person_id == processing_person => Ok(()),
        _ => Err(Error::Validation(
            "processing scope must match ingestion scope".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person_request(platform: Platform) -> PipelineRequest {
        let person_id = PersonId::now_v7();
        PipelineRequest {
            scope: PipelineScope::Person { person_id },
            sources: vec![SelectedSource {
                source_id: SourceId::now_v7(),
                source_name: "saved source".into(),
                platform: platform.clone(),
                identity: Some(IdentitySnapshot {
                    identity_id: Uuid::now_v7(),
                    person_id,
                    platform,
                    username: "canonical-login".into(),
                    platform_user_id: Some("account-123".into()),
                }),
            }],
            since_date: Some("2026-01-01".into()),
            run_started_at: OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
            processing: ProcessingScope::Person { person_id },
        }
    }

    #[test]
    fn person_snapshot_roundtrip_and_validation() {
        let request = person_request(Platform::Discourse("ubuntu".into()));
        request.validate().unwrap();
        let decoded: PipelineRequest =
            serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn unscoped_defaults_to_all() {
        let value = serde_json::json!({"run_started_at": OffsetDateTime::UNIX_EPOCH});
        let request: PipelineRequest = serde_json::from_value(value).unwrap();
        assert_eq!(request.scope, PipelineScope::All);
        assert_eq!(request.processing, ProcessingScope::All);
        request.validate().unwrap();
    }

    #[test]
    fn missing_required_person_fields_fail() {
        let mut request = person_request(Platform::Github);
        request.since_date = None;
        assert!(request.validate().is_err());
        request.since_date = Some("2026-01-01".into());
        request.sources.clear();
        assert!(request.validate().is_err());
    }

    #[test]
    fn jira_requires_stable_account_id() {
        let mut request = person_request(Platform::Jira);
        request.sources[0]
            .identity
            .as_mut()
            .unwrap()
            .platform_user_id = None;
        assert!(request.validate().is_err());
    }

    #[test]
    fn discourse_instances_and_people_cannot_match_interchangeably() {
        let mut request = person_request(Platform::Discourse("ubuntu".into()));
        request.sources[0].identity.as_mut().unwrap().platform =
            Platform::Discourse("snapcraft".into());
        assert!(request.validate().is_err());
        request.sources[0].identity.as_mut().unwrap().platform =
            request.sources[0].platform.clone();
        request.sources[0].identity.as_mut().unwrap().person_id = PersonId::now_v7();
        assert!(request.validate().is_err());
    }

    #[test]
    fn malformed_ids_dates_and_processing_fail() {
        let request = person_request(Platform::Github);
        let mut value = serde_json::to_value(&request).unwrap();
        value["sources"][0]["source_id"] = "invalid-id".into();
        assert!(serde_json::from_value::<PipelineRequest>(value).is_err());
        for value in ["2026-2-01", "2026-02-30", "tomorrow"] {
            assert!(parse_since_date(value).is_err());
        }
        let mut request = request;
        request.processing = ProcessingScope::All;
        assert!(request.validate().is_err());
    }

    #[test]
    fn event_window_is_inclusive_and_independent_of_parent_creation() {
        let request = person_request(Platform::Github);
        let context = SourceRunContext {
            pipeline_id: Uuid::now_v7(),
            scope: request.scope,
            source: request.sources[0].clone(),
            since_date: request.since_date,
            run_started_at: request.run_started_at,
            processing: request.processing,
        };
        context.validate().unwrap();
        let lower = parse_since_date("2026-01-01")
            .unwrap()
            .midnight()
            .assume_utc();
        assert!(context.contains_event(lower).unwrap());
        assert!(context.contains_event(context.run_started_at).unwrap());
        assert!(
            !context
                .contains_event(lower - time::Duration::seconds(1))
                .unwrap()
        );
        assert!(
            !context
                .contains_event(context.run_started_at + time::Duration::seconds(1))
                .unwrap()
        );
        // The old parent PR's creation timestamp is intentionally absent: a
        // qualifying submitted review is eligible regardless of that timestamp.
    }

    #[test]
    fn source_and_pipeline_ids_must_be_non_nil_and_selected_once() {
        let mut request = person_request(Platform::Github);
        request.sources.push(request.sources[0].clone());
        assert!(request.validate().is_err());
        request.sources.pop();
        request.sources[0].source_id = SourceId::new(Uuid::nil());
        assert!(request.validate().is_err());
    }
}
