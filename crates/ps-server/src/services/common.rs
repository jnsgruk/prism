use std::collections::BTreeMap;

use crate::interceptor::AuthContext;
use ps_proto::canonical::prism::v1::{
    AiProvider as ProtoAiProvider, ContributionState as ProtoContributionState,
    ContributionType as ProtoContributionType, EnrichmentType as ProtoEnrichmentType,
    InsightPeriod as ProtoInsightPeriod, PersonFilter as ProtoPersonFilter,
    Platform as ProtoPlatform, RunStatus as ProtoRunStatus,
};
use tonic::{Request, Status};
use tracing::error;

/// Extract the authenticated user context from a gRPC request.
///
/// Returns `Unauthenticated` if the auth interceptor did not attach a context
/// (i.e. the RPC is not on the public allow-list but no valid token was sent).
#[allow(clippy::result_large_err)]
pub fn require_auth<T>(request: &Request<T>) -> Result<AuthContext, Status> {
    request
        .extensions()
        .get::<AuthContext>()
        .cloned()
        .ok_or_else(|| Status::unauthenticated("not authenticated"))
}

/// Extract authenticated user context and verify the user has the admin role.
#[allow(clippy::result_large_err)]
pub fn require_admin<T>(request: &Request<T>) -> Result<AuthContext, Status> {
    let ctx = require_auth(request)?;
    if ctx.role != ps_core::models::Role::Admin {
        return Err(Status::permission_denied("admin role required"));
    }
    Ok(ctx)
}

/// Map a database/repo error to a gRPC `Internal` status.
///
/// Logs the full error server-side but returns a generic message to the client
/// to avoid leaking internal details (table names, constraints, query fragments).
pub fn db_err(e: impl std::fmt::Display) -> Status {
    error!(error = %e, "database error");
    Status::internal("internal error")
}

/// Map a backup I/O error to a gRPC `Internal` status.
pub fn backup_err(e: impl std::fmt::Display) -> Status {
    error!(error = %e, "backup error");
    Status::internal("internal error")
}

/// Convert an `OffsetDateTime` to a prost `Timestamp`.
pub fn to_timestamp(dt: time::OffsetDateTime) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: dt.unix_timestamp(),
        nanos: 0,
    }
}

// ---------------------------------------------------------------------------
// serde_json ↔ prost_types conversion
// ---------------------------------------------------------------------------

/// Convert `serde_json::Value` to `prost_types::Struct`.
pub fn serde_json_to_prost_struct(value: &serde_json::Value) -> prost_types::Struct {
    match value {
        serde_json::Value::Object(map) => {
            let fields = map
                .iter()
                .map(|(k, v)| (k.clone(), serde_json_to_prost_value(v)))
                .collect();
            prost_types::Struct { fields }
        }
        _ => prost_types::Struct {
            fields: BTreeMap::new(),
        },
    }
}

fn serde_json_to_prost_value(value: &serde_json::Value) -> prost_types::Value {
    let kind = match value {
        serde_json::Value::Null => Some(prost_types::value::Kind::NullValue(0)),
        serde_json::Value::Bool(b) => Some(prost_types::value::Kind::BoolValue(*b)),
        // 0.0 is a safe default for the JSON-to-Protobuf Number conversion:
        // serde_json::Number::as_f64() only returns None for values outside f64
        // range, which are not representable in proto's double either.
        serde_json::Value::Number(n) => Some(prost_types::value::Kind::NumberValue(
            n.as_f64().unwrap_or(0.0),
        )),
        serde_json::Value::String(s) => Some(prost_types::value::Kind::StringValue(s.clone())),
        serde_json::Value::Array(arr) => {
            let values = arr.iter().map(serde_json_to_prost_value).collect();
            Some(prost_types::value::Kind::ListValue(
                prost_types::ListValue { values },
            ))
        }
        serde_json::Value::Object(_) => Some(prost_types::value::Kind::StructValue(
            serde_json_to_prost_struct(value),
        )),
    };
    prost_types::Value { kind }
}

/// Convert `prost_types::Struct` back to `serde_json::Value`.
pub fn prost_struct_to_serde_json(s: &prost_types::Struct) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = s
        .fields
        .iter()
        .map(|(k, v)| (k.clone(), prost_value_to_serde_json(v)))
        .collect();
    serde_json::Value::Object(map)
}

fn prost_value_to_serde_json(v: &prost_types::Value) -> serde_json::Value {
    match &v.kind {
        Some(prost_types::value::Kind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(prost_types::value::Kind::NumberValue(n)) => {
            serde_json::json!(n)
        }
        Some(prost_types::value::Kind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(prost_types::value::Kind::ListValue(list)) => {
            let arr: Vec<serde_json::Value> =
                list.values.iter().map(prost_value_to_serde_json).collect();
            serde_json::Value::Array(arr)
        }
        Some(prost_types::value::Kind::StructValue(s)) => prost_struct_to_serde_json(s),
        Some(prost_types::value::Kind::NullValue(_)) | None => serde_json::Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Domain enum ↔ proto enum conversions
// ---------------------------------------------------------------------------

/// Convert a platform string (e.g. "github", "discourse-ubuntu") to proto enum
/// + optional instance.
pub fn platform_to_proto(platform_str: &str) -> (i32, Option<String>) {
    if let Some(instance) = platform_str.strip_prefix("discourse-") {
        (ProtoPlatform::Discourse.into(), Some(instance.to_string()))
    } else {
        let proto = match platform_str {
            "github" => ProtoPlatform::Github,
            "jira" => ProtoPlatform::Jira,
            "launchpad" => ProtoPlatform::Launchpad,
            "mattermost" => ProtoPlatform::Mattermost,
            _ => ProtoPlatform::Unspecified,
        };
        (proto.into(), None)
    }
}

/// Convert proto platform enum + optional instance back to a platform string.
pub fn proto_to_platform_str(platform: i32, instance: Option<&str>) -> Option<String> {
    match ProtoPlatform::try_from(platform) {
        Ok(ProtoPlatform::Github) => Some("github".to_string()),
        Ok(ProtoPlatform::Jira) => Some("jira".to_string()),
        Ok(ProtoPlatform::Launchpad) => Some("launchpad".to_string()),
        Ok(ProtoPlatform::Mattermost) => Some("mattermost".to_string()),
        Ok(ProtoPlatform::Discourse) => match instance {
            Some(inst) => Some(format!("discourse-{inst}")),
            None => Some("discourse".to_string()),
        },
        _ => None,
    }
}

/// Convert a contribution type string to proto enum i32.
pub fn contribution_type_to_proto(s: &str) -> i32 {
    match s {
        "pull_request" => ProtoContributionType::PullRequest.into(),
        "pr_review" => ProtoContributionType::PrReview.into(),
        "jira_ticket" => ProtoContributionType::JiraTicket.into(),
        "discourse_topic" => ProtoContributionType::DiscourseTopic.into(),
        "discourse_post" => ProtoContributionType::DiscoursePost.into(),
        "discourse_like" => ProtoContributionType::DiscourseLike.into(),
        _ => ProtoContributionType::Unspecified.into(),
    }
}

/// Convert proto contribution type i32 back to a string.
pub fn proto_to_contribution_type_str(v: i32) -> Option<String> {
    match ProtoContributionType::try_from(v) {
        Ok(ProtoContributionType::PullRequest) => Some("pull_request".to_string()),
        Ok(ProtoContributionType::PrReview) => Some("pr_review".to_string()),
        Ok(ProtoContributionType::JiraTicket) => Some("jira_ticket".to_string()),
        Ok(ProtoContributionType::DiscourseTopic) => Some("discourse_topic".to_string()),
        Ok(ProtoContributionType::DiscoursePost) => Some("discourse_post".to_string()),
        Ok(ProtoContributionType::DiscourseLike) => Some("discourse_like".to_string()),
        _ => None,
    }
}

/// Convert a contribution state string to proto enum i32.
pub fn contribution_state_to_proto(s: &str) -> i32 {
    match s {
        "open" => ProtoContributionState::Open.into(),
        "closed" => ProtoContributionState::Closed.into(),
        "merged" => ProtoContributionState::Merged.into(),
        "in_progress" => ProtoContributionState::InProgress.into(),
        "APPROVED" => ProtoContributionState::Approved.into(),
        "CHANGES_REQUESTED" => ProtoContributionState::ChangesRequested.into(),
        "COMMENTED" => ProtoContributionState::Commented.into(),
        "PENDING" => ProtoContributionState::Pending.into(),
        "DISMISSED" => ProtoContributionState::Dismissed.into(),
        _ => ProtoContributionState::Unspecified.into(),
    }
}

/// Convert proto contribution state i32 back to a string.
pub fn proto_to_contribution_state_str(v: i32) -> Option<String> {
    match ProtoContributionState::try_from(v) {
        Ok(ProtoContributionState::Open) => Some("open".to_string()),
        Ok(ProtoContributionState::Closed) => Some("closed".to_string()),
        Ok(ProtoContributionState::Merged) => Some("merged".to_string()),
        Ok(ProtoContributionState::InProgress) => Some("in_progress".to_string()),
        Ok(ProtoContributionState::Approved) => Some("APPROVED".to_string()),
        Ok(ProtoContributionState::ChangesRequested) => Some("CHANGES_REQUESTED".to_string()),
        Ok(ProtoContributionState::Commented) => Some("COMMENTED".to_string()),
        Ok(ProtoContributionState::Pending) => Some("PENDING".to_string()),
        Ok(ProtoContributionState::Dismissed) => Some("DISMISSED".to_string()),
        Ok(ProtoContributionState::Done) => Some("done".to_string()),
        _ => None,
    }
}

/// Convert an `IngestionStatus` to proto `RunStatus` i32.
pub fn run_status_to_proto(status: &ps_core::models::IngestionStatus) -> i32 {
    match status {
        ps_core::models::IngestionStatus::Running => ProtoRunStatus::Running.into(),
        ps_core::models::IngestionStatus::Completed => ProtoRunStatus::Completed.into(),
        ps_core::models::IngestionStatus::CompletedWithWarnings => {
            ProtoRunStatus::CompletedWithWarnings.into()
        }
        ps_core::models::IngestionStatus::Failed => ProtoRunStatus::Failed.into(),
        ps_core::models::IngestionStatus::Cancelled => ProtoRunStatus::Cancelled.into(),
    }
}

/// Convert an AI provider string to proto enum i32.
pub fn ai_provider_to_proto(s: &str) -> i32 {
    match s {
        "google" => ProtoAiProvider::Google.into(),
        "openrouter" => ProtoAiProvider::Openrouter.into(),
        _ => ProtoAiProvider::Unspecified.into(),
    }
}

/// Convert proto AI provider i32 back to a string.
pub fn proto_to_ai_provider_str(v: i32) -> Option<String> {
    match ProtoAiProvider::try_from(v) {
        Ok(ProtoAiProvider::Google) => Some("google".to_string()),
        Ok(ProtoAiProvider::Openrouter) => Some("openrouter".to_string()),
        _ => None,
    }
}

/// Convert an enrichment type string to proto enum i32.
pub fn enrichment_type_to_proto(s: &str) -> i32 {
    match s {
        "review_depth" => ProtoEnrichmentType::ReviewDepth.into(),
        "sentiment" => ProtoEnrichmentType::Sentiment.into(),
        "significance" => ProtoEnrichmentType::Significance.into(),
        "topic" => ProtoEnrichmentType::Topic.into(),
        _ => ProtoEnrichmentType::Unspecified.into(),
    }
}

/// Convert proto enrichment type i32 back to a string.
pub fn proto_to_enrichment_type_str(v: i32) -> Option<String> {
    match ProtoEnrichmentType::try_from(v) {
        Ok(ProtoEnrichmentType::ReviewDepth) => Some("review_depth".to_string()),
        Ok(ProtoEnrichmentType::Sentiment) => Some("sentiment".to_string()),
        Ok(ProtoEnrichmentType::Significance) => Some("significance".to_string()),
        Ok(ProtoEnrichmentType::Topic) => Some("topic".to_string()),
        _ => None,
    }
}

/// Convert proto `InsightPeriod` i32 to a period string.
pub fn insight_period_to_str(v: i32) -> Option<String> {
    match ProtoInsightPeriod::try_from(v) {
        Ok(ProtoInsightPeriod::LastWeek) => Some("last_week".to_string()),
        Ok(ProtoInsightPeriod::LastMonth) => Some("last_month".to_string()),
        Ok(ProtoInsightPeriod::LastQuarter) => Some("last_quarter".to_string()),
        Ok(ProtoInsightPeriod::LastYear) => Some("last_year".to_string()),
        _ => None,
    }
}

/// Convert proto `PersonFilter` i32 to a filter string.
pub fn person_filter_to_str(v: i32) -> Option<String> {
    match ProtoPersonFilter::try_from(v) {
        Ok(ProtoPersonFilter::Unassigned) => Some("unassigned".to_string()),
        Ok(ProtoPersonFilter::Inactive) => Some("inactive".to_string()),
        _ => None,
    }
}
