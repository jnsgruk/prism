use std::collections::BTreeMap;

use ps_proto::canonical::prism::v1::{
    AiProvider as ProtoAiProvider, ContributionState as ProtoContributionState,
    ContributionType as ProtoContributionType, EnrichmentType as ProtoEnrichmentType,
    InsightPeriod as ProtoInsightPeriod, PersonFilter as ProtoPersonFilter,
    Platform as ProtoPlatform, RunStatus as ProtoRunStatus,
};

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
//
// Delegates to canonical implementations in ps_proto::convert.
// ---------------------------------------------------------------------------

/// Convert a platform string (e.g. "github", "discourse-ubuntu") to proto enum
/// + optional instance.
pub fn platform_to_proto(platform_str: &str) -> (i32, Option<String>) {
    let (p, inst) = ProtoPlatform::from_db_str(platform_str);
    (p.into(), inst)
}

/// Convert proto platform enum + optional instance back to a platform string.
pub fn proto_to_platform_str(platform: i32, instance: Option<&str>) -> Option<String> {
    ProtoPlatform::try_from(platform).ok()?.to_db_str(instance)
}

/// Convert a `ContributionType` to proto enum i32.
pub fn contribution_type_to_proto(ct: ps_core::models::ContributionType) -> i32 {
    ProtoContributionType::from_db_str(ct.as_str()).into()
}

/// Convert proto contribution type i32 back to a domain enum.
pub fn proto_to_contribution_type(v: i32) -> Option<ps_core::models::ContributionType> {
    ProtoContributionType::try_from(v)
        .ok()?
        .to_db_str()?
        .parse()
        .ok()
}

/// Convert a `ContributionState` to proto enum i32.
pub fn contribution_state_to_proto(cs: ps_core::models::ContributionState) -> i32 {
    ProtoContributionState::from_db_str(cs.as_str()).into()
}

/// Convert a contribution state string to proto enum i32.
///
/// Accepts `&str` for cases where the state comes from a DB query as a string
/// that may not parse into a `ContributionState` (e.g. unknown states).
pub fn contribution_state_str_to_proto(s: &str) -> i32 {
    ProtoContributionState::from_db_str(s).into()
}

/// Convert proto contribution state i32 back to a domain enum.
pub fn proto_to_contribution_state(v: i32) -> Option<ps_core::models::ContributionState> {
    ProtoContributionState::try_from(v)
        .ok()?
        .to_db_str()?
        .parse()
        .ok()
}

/// Convert proto contribution type i32 back to a string (convenience wrapper).
pub fn proto_to_contribution_type_str(v: i32) -> Option<String> {
    proto_to_contribution_type(v).map(|ct| ct.to_string())
}

/// Convert proto contribution state i32 back to a string (convenience wrapper).
pub fn proto_to_contribution_state_str(v: i32) -> Option<String> {
    proto_to_contribution_state(v).map(|cs| cs.to_string())
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

/// Convert an `AiProvider` to proto enum i32.
pub fn ai_provider_to_proto(provider: ps_core::models::AiProvider) -> i32 {
    match provider {
        ps_core::models::AiProvider::Google => ProtoAiProvider::Google.into(),
        ps_core::models::AiProvider::OpenRouter => ProtoAiProvider::Openrouter.into(),
    }
}

/// Convert proto AI provider i32 back to a string (convenience wrapper).
pub fn proto_to_ai_provider_str(v: i32) -> Option<String> {
    proto_to_ai_provider(v).map(|p| p.to_string())
}

/// Convert proto AI provider i32 back to a domain enum.
pub fn proto_to_ai_provider(v: i32) -> Option<ps_core::models::AiProvider> {
    match ProtoAiProvider::try_from(v) {
        Ok(ProtoAiProvider::Google) => Some(ps_core::models::AiProvider::Google),
        Ok(ProtoAiProvider::Openrouter) => Some(ps_core::models::AiProvider::OpenRouter),
        _ => None,
    }
}

/// Convert an `EnrichmentType` to proto enum i32.
pub fn enrichment_type_to_proto(et: ps_core::models::EnrichmentType) -> i32 {
    match et {
        ps_core::models::EnrichmentType::ReviewDepth => ProtoEnrichmentType::ReviewDepth.into(),
        ps_core::models::EnrichmentType::Sentiment => ProtoEnrichmentType::Sentiment.into(),
        ps_core::models::EnrichmentType::Significance => ProtoEnrichmentType::Significance.into(),
        ps_core::models::EnrichmentType::Topic => ProtoEnrichmentType::Topic.into(),
    }
}

/// Convert proto enrichment type i32 back to an `EnrichmentType`.
pub fn proto_to_enrichment_type(v: i32) -> Option<ps_core::models::EnrichmentType> {
    match ProtoEnrichmentType::try_from(v) {
        Ok(ProtoEnrichmentType::ReviewDepth) => Some(ps_core::models::EnrichmentType::ReviewDepth),
        Ok(ProtoEnrichmentType::Sentiment) => Some(ps_core::models::EnrichmentType::Sentiment),
        Ok(ProtoEnrichmentType::Significance) => {
            Some(ps_core::models::EnrichmentType::Significance)
        }
        Ok(ProtoEnrichmentType::Topic) => Some(ps_core::models::EnrichmentType::Topic),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_roundtrip_github() {
        let (proto, inst) = platform_to_proto("github");
        assert_eq!(proto, i32::from(ProtoPlatform::Github));
        assert!(inst.is_none());
        assert_eq!(
            proto_to_platform_str(proto, None),
            Some("github".to_string())
        );
    }

    #[test]
    fn platform_roundtrip_discourse_instance() {
        let (proto, inst) = platform_to_proto("discourse-ubuntu");
        assert_eq!(proto, i32::from(ProtoPlatform::Discourse));
        assert_eq!(inst.as_deref(), Some("ubuntu"));
        assert_eq!(
            proto_to_platform_str(proto, inst.as_deref()),
            Some("discourse-ubuntu".to_string())
        );
    }

    #[test]
    fn platform_roundtrip_bare_discourse() {
        let (proto, inst) = platform_to_proto("discourse");
        assert_eq!(proto, i32::from(ProtoPlatform::Discourse));
        assert!(inst.is_none());
        assert_eq!(
            proto_to_platform_str(proto, None),
            Some("discourse".to_string())
        );
    }

    #[test]
    fn contribution_type_roundtrip() {
        use ps_core::models::ContributionType;
        for (ct, expected) in [
            (
                ContributionType::PullRequest,
                ProtoContributionType::PullRequest,
            ),
            (ContributionType::PrReview, ProtoContributionType::PrReview),
            (
                ContributionType::JiraTicket,
                ProtoContributionType::JiraTicket,
            ),
            (
                ContributionType::DiscourseTopic,
                ProtoContributionType::DiscourseTopic,
            ),
            (
                ContributionType::DiscoursePost,
                ProtoContributionType::DiscoursePost,
            ),
            (
                ContributionType::DiscourseLike,
                ProtoContributionType::DiscourseLike,
            ),
        ] {
            let proto = contribution_type_to_proto(ct);
            assert_eq!(proto, i32::from(expected), "failed for {ct}");
            assert_eq!(
                proto_to_contribution_type(proto),
                Some(ct),
                "reverse failed for {ct}"
            );
        }
    }

    #[test]
    fn contribution_state_roundtrip() {
        use ps_core::models::ContributionState;
        for (cs, expected) in [
            (ContributionState::Open, ProtoContributionState::Open),
            (ContributionState::Closed, ProtoContributionState::Closed),
            (ContributionState::Merged, ProtoContributionState::Merged),
            (
                ContributionState::InProgress,
                ProtoContributionState::InProgress,
            ),
            (
                ContributionState::Approved,
                ProtoContributionState::Approved,
            ),
            (
                ContributionState::ChangesRequested,
                ProtoContributionState::ChangesRequested,
            ),
        ] {
            let proto = contribution_state_to_proto(cs);
            assert_eq!(proto, i32::from(expected), "failed for {cs}");
            assert_eq!(
                proto_to_contribution_state(proto),
                Some(cs),
                "reverse failed for {cs}"
            );
        }
    }

    #[test]
    fn unknown_protos_return_none() {
        assert!(proto_to_contribution_type(999).is_none());
        assert!(proto_to_contribution_state(999).is_none());
        assert!(proto_to_platform_str(999, None).is_none());
    }
}
