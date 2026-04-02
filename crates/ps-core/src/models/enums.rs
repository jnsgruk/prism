use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgArgumentBuffer, PgTypeInfo, PgValueRef};
use sqlx::{Decode, Encode, Postgres, Type};

// ---------------------------------------------------------------------------
// Platform
// ---------------------------------------------------------------------------

/// The external platform a piece of data originates from, or that a platform
/// identity is linked to.
///
/// Most platforms have a fixed string representation (`github`, `jira`, etc.).
/// Discourse is instance-qualified: `discourse-{instance}` (e.g.
/// `discourse-ubuntu`, `discourse-snapcraft`) because each Discourse
/// installation is a separate source with its own credentials, watermarks,
/// and identity namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Github,
    Launchpad,
    Mattermost,
    Jira,
    /// Instance-qualified Discourse platform.  The `String` is the instance
    /// suffix (e.g. `"ubuntu"` for `"discourse-ubuntu"`).
    #[serde(
        serialize_with = "serialize_discourse",
        deserialize_with = "deserialize_discourse",
        untagged
    )]
    Discourse(String),
}

fn serialize_discourse<S: serde::Serializer>(
    instance: &String,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("discourse-{instance}"))
}

fn deserialize_discourse<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let s = String::deserialize(deserializer)?;
    s.strip_prefix("discourse-")
        .map(String::from)
        .ok_or_else(|| serde::de::Error::custom(format!("expected discourse-* prefix: {s}")))
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discourse(instance) => write!(f, "discourse-{instance}"),
            other => f.write_str(other.static_str()),
        }
    }
}

impl Platform {
    /// Return a `Cow` for the platform string.  Fixed variants borrow a
    /// `'static` str; `Discourse` allocates.
    pub fn as_cow(&self) -> Cow<'static, str> {
        match self {
            Self::Discourse(instance) => Cow::Owned(format!("discourse-{instance}")),
            other => Cow::Borrowed(other.static_str()),
        }
    }

    /// String slice for fixed (non-Discourse) variants.
    fn static_str(&self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Launchpad => "launchpad",
            Self::Mattermost => "mattermost",
            Self::Jira => "jira",
            // Discourse uses Display for dynamic formatting; this arm is
            // unreachable via the public API (Display dispatches before here).
            Self::Discourse(_) => "discourse",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    /// Returns `true` if this platform is any Discourse instance.
    pub fn is_discourse(&self) -> bool {
        matches!(self, Self::Discourse(_))
    }
}

impl FromStr for Platform {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "github" => Ok(Self::Github),
            "launchpad" => Ok(Self::Launchpad),
            "mattermost" => Ok(Self::Mattermost),
            "jira" => Ok(Self::Jira),
            other => {
                if let Some(instance) = other.strip_prefix("discourse-") {
                    if instance.is_empty() {
                        return Err("discourse instance name cannot be empty".into());
                    }
                    Ok(Self::Discourse(instance.to_owned()))
                } else {
                    Err(format!("invalid Platform: {s}"))
                }
            }
        }
    }
}

// Manual sqlx implementation for Platform (can't use the macro because
// Discourse carries a dynamic string).

impl Type<Postgres> for Platform {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("TEXT")
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        <&str as Type<Postgres>>::compatible(ty)
    }
}

impl Encode<'_, Postgres> for Platform {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let s = self.to_string();
        <String as Encode<Postgres>>::encode(s, buf)
    }
}

impl Decode<'_, Postgres> for Platform {
    fn decode(value: PgValueRef<'_>) -> Result<Self, BoxDynError> {
        let s = <&str as Decode<Postgres>>::decode(value)?;
        s.parse::<Platform>()
            .map_err(|e| -> BoxDynError { e.into() })
    }
}

// ---------------------------------------------------------------------------
// ContributionType
// ---------------------------------------------------------------------------

/// The kind of contribution tracked in `activity.contributions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionType {
    PullRequest,
    PrReview,
    JiraTicket,
    DiscoursePost,
    DiscourseTopic,
    DiscourseLike,
}

impl fmt::Display for ContributionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ContributionType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PullRequest => "pull_request",
            Self::PrReview => "pr_review",
            Self::JiraTicket => "jira_ticket",
            Self::DiscoursePost => "discourse_post",
            Self::DiscourseTopic => "discourse_topic",
            Self::DiscourseLike => "discourse_like",
        }
    }
}

impl FromStr for ContributionType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pull_request" => Ok(Self::PullRequest),
            "pr_review" => Ok(Self::PrReview),
            "jira_ticket" => Ok(Self::JiraTicket),
            "discourse_post" => Ok(Self::DiscoursePost),
            "discourse_topic" => Ok(Self::DiscourseTopic),
            "discourse_like" => Ok(Self::DiscourseLike),
            _ => Err(format!("invalid ContributionType: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// ContributionState
// ---------------------------------------------------------------------------

/// The state of a contribution.
///
/// PR states are normalised to lowercase (`open`, `closed`, `merged`).
/// Review states come from the GitHub API (`APPROVED`, etc.) and are stored
/// in their original casing for compatibility.
/// Jira adds `in_progress` for tickets actively being worked on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionState {
    // Shared states
    Open,
    Closed,
    // PR states
    Merged,
    // Jira states
    InProgress,
    // Review states (GitHub API casing preserved via Display/sqlx)
    Approved,
    ChangesRequested,
    Commented,
    Pending,
    Dismissed,
}

impl fmt::Display for ContributionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ContributionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Merged => "merged",
            Self::InProgress => "in_progress",
            Self::Approved => "APPROVED",
            Self::ChangesRequested => "CHANGES_REQUESTED",
            Self::Commented => "COMMENTED",
            Self::Pending => "PENDING",
            Self::Dismissed => "DISMISSED",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        s.parse().ok()
    }
}

impl FromStr for ContributionState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Self::Open),
            "closed" => Ok(Self::Closed),
            "merged" => Ok(Self::Merged),
            "in_progress" => Ok(Self::InProgress),
            "APPROVED" => Ok(Self::Approved),
            "CHANGES_REQUESTED" => Ok(Self::ChangesRequested),
            "COMMENTED" => Ok(Self::Commented),
            "PENDING" => Ok(Self::Pending),
            "DISMISSED" => Ok(Self::Dismissed),
            _ => Err(format!("invalid ContributionState: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// IngestionStatus
// ---------------------------------------------------------------------------

/// The status of an ingestion run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestionStatus {
    Running,
    Completed,
    CompletedWithWarnings,
    Failed,
    Cancelled,
}

impl fmt::Display for IngestionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl IngestionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::CompletedWithWarnings => "completed_with_warnings",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl FromStr for IngestionStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "completed_with_warnings" => Ok(Self::CompletedWithWarnings),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("invalid IngestionStatus: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// PeriodType
// ---------------------------------------------------------------------------

/// The granularity of a metric snapshot period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeriodType {
    Week,
    Month,
    Quarter,
}

impl fmt::Display for PeriodType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PeriodType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Week => "week",
            Self::Month => "month",
            Self::Quarter => "quarter",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        s.parse().ok()
    }
}

impl FromStr for PeriodType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "week" => Ok(Self::Week),
            "month" => Ok(Self::Month),
            "quarter" => Ok(Self::Quarter),
            _ => Err(format!("invalid PeriodType: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// QueryStatus
// ---------------------------------------------------------------------------

/// The lifecycle status of an agentic query conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryStatus {
    Idle,
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl fmt::Display for QueryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl QueryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Returns `true` if this status represents a terminal state.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Completed | Self::Failed | Self::Cancelled
        )
    }
}

impl FromStr for QueryStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "idle" => Ok(Self::Idle),
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("invalid QueryStatus: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// EnrichmentType
// ---------------------------------------------------------------------------

/// The type of AI enrichment applied to a contribution.
///
/// Each variant targets specific contribution types:
/// - `ReviewDepth` / `Sentiment` → `pr_review`
/// - `Significance` → `pull_request`
/// - `Topic` → `discourse_topic`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrichmentType {
    ReviewDepth,
    Sentiment,
    Significance,
    Topic,
}

impl fmt::Display for EnrichmentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl EnrichmentType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReviewDepth => "review_depth",
            Self::Sentiment => "sentiment",
            Self::Significance => "significance",
            Self::Topic => "topic",
        }
    }

    /// All enrichment types that the scheduler should process.
    pub fn all() -> &'static [Self] {
        &[
            Self::ReviewDepth,
            Self::Sentiment,
            Self::Significance,
            Self::Topic,
        ]
    }

    /// Return the `ContributionType` this enrichment targets.
    pub fn contribution_type_filter(self) -> ContributionType {
        match self {
            Self::ReviewDepth | Self::Sentiment => ContributionType::PrReview,
            Self::Significance => ContributionType::PullRequest,
            Self::Topic => ContributionType::DiscourseTopic,
        }
    }
}

impl FromStr for EnrichmentType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "review_depth" => Ok(Self::ReviewDepth),
            "sentiment" => Ok(Self::Sentiment),
            "significance" => Ok(Self::Significance),
            "topic" => Ok(Self::Topic),
            _ => Err(format!("invalid EnrichmentType: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// sqlx TEXT round-tripping — macro to avoid repeating the boilerplate
// ---------------------------------------------------------------------------

macro_rules! impl_sqlx_text {
    ($ty:ty, $from_fn:expr) => {
        impl Type<Postgres> for $ty {
            fn type_info() -> PgTypeInfo {
                PgTypeInfo::with_name("TEXT")
            }

            fn compatible(ty: &PgTypeInfo) -> bool {
                <&str as Type<Postgres>>::compatible(ty)
            }
        }

        impl Encode<'_, Postgres> for $ty {
            fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
                <&str as Encode<Postgres>>::encode(self.as_str(), buf)
            }
        }

        impl Decode<'_, Postgres> for $ty {
            fn decode(value: PgValueRef<'_>) -> Result<Self, BoxDynError> {
                let s = <&str as Decode<Postgres>>::decode(value)?;
                let convert: fn(&str) -> Option<$ty> = $from_fn;
                convert(s).ok_or_else(|| format!("invalid {} value: {s}", stringify!($ty)).into())
            }
        }
    };
}

// Platform has manual sqlx implementation above (Discourse carries dynamic data).
impl_sqlx_text!(ContributionType, |s: &str| s.parse().ok());
impl_sqlx_text!(ContributionState, |s: &str| s.parse().ok());
impl_sqlx_text!(IngestionStatus, |s: &str| s.parse().ok());
impl_sqlx_text!(PeriodType, |s: &str| s.parse().ok());
impl_sqlx_text!(ResolutionStatus, |s: &str| s.parse().ok());
impl_sqlx_text!(Role, |s: &str| s.parse().ok());
impl_sqlx_text!(TaskType, |s: &str| s.parse().ok());
impl_sqlx_text!(AiProvider, |s: &str| s.parse().ok());
impl_sqlx_text!(EnrichmentType, |s: &str| s.parse().ok());
impl_sqlx_text!(QueryStatus, |s: &str| s.parse().ok());

// ---------------------------------------------------------------------------
// ResolutionStatus
// ---------------------------------------------------------------------------

/// The status of an identity resolution attempt for a person on a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatus {
    /// Not yet attempted — new person or new source.
    Pending,
    /// Successfully matched to a platform username.
    Resolved,
    /// Attempted but no match found.
    Unresolved,
    /// Admin manually set the identity — skip auto-resolution.
    Manual,
}

impl fmt::Display for ResolutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ResolutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Resolved => "resolved",
            Self::Unresolved => "unresolved",
            Self::Manual => "manual",
        }
    }
}

impl FromStr for ResolutionStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "resolved" => Ok(Self::Resolved),
            "unresolved" => Ok(Self::Unresolved),
            "manual" => Ok(Self::Manual),
            _ => Err(format!("invalid ResolutionStatus: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Admin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(Self::Admin),
            _ => Err(format!("invalid Role: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// TaskType (AI task categories)
// ---------------------------------------------------------------------------

/// The type of AI task being performed, used for cost tracking and routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    Enrichment,
    Insights,
    Agentic,
    Embeddings,
}

impl fmt::Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TaskType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enrichment => "enrichment",
            Self::Insights => "insights",
            Self::Agentic => "agentic",
            Self::Embeddings => "embeddings",
        }
    }
}

impl FromStr for TaskType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "enrichment" => Ok(Self::Enrichment),
            "insights" => Ok(Self::Insights),
            "agentic" => Ok(Self::Agentic),
            "embeddings" => Ok(Self::Embeddings),
            _ => Err(format!("invalid TaskType: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// AiProvider
// ---------------------------------------------------------------------------

/// Supported AI provider backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    Google,
    OpenRouter,
}

impl fmt::Display for AiProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AiProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::OpenRouter => "openrouter",
        }
    }
}

impl FromStr for AiProvider {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "google" => Ok(Self::Google),
            "openrouter" => Ok(Self::OpenRouter),
            _ => Err(format!("invalid AiProvider: {s}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Platform --

    #[test]
    fn platform_fixed_roundtrip() {
        for (variant, expected) in [
            (Platform::Github, "github"),
            (Platform::Launchpad, "launchpad"),
            (Platform::Mattermost, "mattermost"),
            (Platform::Jira, "jira"),
        ] {
            assert_eq!(variant.to_string(), expected);
            assert_eq!(expected.parse::<Platform>().unwrap(), variant);
        }
    }

    #[test]
    fn platform_discourse_roundtrip() {
        let p = Platform::Discourse("ubuntu".into());
        assert_eq!(p.to_string(), "discourse-ubuntu");
        assert_eq!(
            "discourse-ubuntu".parse::<Platform>().unwrap(),
            Platform::Discourse("ubuntu".into())
        );
    }

    #[test]
    fn platform_discourse_empty_instance_rejected() {
        assert!("discourse-".parse::<Platform>().is_err());
    }

    #[test]
    fn platform_unknown_errors() {
        assert!("unknown".parse::<Platform>().is_err());
    }

    #[test]
    fn platform_is_discourse() {
        assert!(Platform::Discourse("x".into()).is_discourse());
        assert!(!Platform::Github.is_discourse());
    }

    // -- ContributionType --

    #[test]
    fn contribution_type_roundtrip() {
        for variant in [
            ContributionType::PullRequest,
            ContributionType::PrReview,
            ContributionType::JiraTicket,
            ContributionType::DiscoursePost,
            ContributionType::DiscourseTopic,
            ContributionType::DiscourseLike,
        ] {
            let s = variant.to_string();
            assert_eq!(s.parse::<ContributionType>().unwrap(), variant);
        }
    }

    #[test]
    fn contribution_type_unknown_errors() {
        assert!("bogus".parse::<ContributionType>().is_err());
    }

    // -- ContributionState --

    #[test]
    fn contribution_state_roundtrip() {
        for variant in [
            ContributionState::Open,
            ContributionState::Closed,
            ContributionState::Merged,
            ContributionState::InProgress,
            ContributionState::Approved,
            ContributionState::ChangesRequested,
            ContributionState::Commented,
            ContributionState::Pending,
            ContributionState::Dismissed,
        ] {
            let s = variant.to_string();
            assert_eq!(s.parse::<ContributionState>().unwrap(), variant);
        }
    }

    #[test]
    fn contribution_state_preserves_github_casing() {
        assert_eq!(ContributionState::Approved.as_str(), "APPROVED");
        assert_eq!(
            ContributionState::ChangesRequested.as_str(),
            "CHANGES_REQUESTED"
        );
    }

    #[test]
    fn contribution_state_unknown_errors() {
        assert!("unknown".parse::<ContributionState>().is_err());
    }

    // -- IngestionStatus --

    #[test]
    fn ingestion_status_roundtrip() {
        for variant in [
            IngestionStatus::Running,
            IngestionStatus::Completed,
            IngestionStatus::CompletedWithWarnings,
            IngestionStatus::Failed,
            IngestionStatus::Cancelled,
        ] {
            let s = variant.to_string();
            assert_eq!(s.parse::<IngestionStatus>().unwrap(), variant);
        }
    }

    #[test]
    fn ingestion_status_unknown_errors() {
        assert!("bogus".parse::<IngestionStatus>().is_err());
    }

    // -- PeriodType --

    #[test]
    fn period_type_roundtrip() {
        for variant in [PeriodType::Week, PeriodType::Month, PeriodType::Quarter] {
            let s = variant.to_string();
            assert_eq!(s.parse::<PeriodType>().unwrap(), variant);
        }
    }

    #[test]
    fn period_type_unknown_errors() {
        assert!("yearly".parse::<PeriodType>().is_err());
    }

    // -- ResolutionStatus --

    #[test]
    fn resolution_status_roundtrip() {
        for variant in [
            ResolutionStatus::Pending,
            ResolutionStatus::Resolved,
            ResolutionStatus::Unresolved,
            ResolutionStatus::Manual,
        ] {
            let s = variant.to_string();
            assert_eq!(s.parse::<ResolutionStatus>().unwrap(), variant);
        }
    }

    #[test]
    fn resolution_status_unknown_errors() {
        assert!("auto".parse::<ResolutionStatus>().is_err());
    }

    // -- Role --

    #[test]
    fn role_roundtrip() {
        assert_eq!(Role::Admin.to_string(), "admin");
        assert_eq!("admin".parse::<Role>().unwrap(), Role::Admin);
    }

    #[test]
    fn role_unknown_errors() {
        assert!("viewer".parse::<Role>().is_err());
    }

    // -- TaskType --

    #[test]
    fn task_type_roundtrip() {
        for variant in [
            TaskType::Enrichment,
            TaskType::Insights,
            TaskType::Agentic,
            TaskType::Embeddings,
        ] {
            let s = variant.to_string();
            assert_eq!(s.parse::<TaskType>().unwrap(), variant);
        }
    }

    #[test]
    fn task_type_unknown_errors() {
        assert!("training".parse::<TaskType>().is_err());
    }

    // -- AiProvider --

    #[test]
    fn ai_provider_roundtrip() {
        for variant in [AiProvider::Google, AiProvider::OpenRouter] {
            let s = variant.to_string();
            assert_eq!(s.parse::<AiProvider>().unwrap(), variant);
        }
    }

    #[test]
    fn ai_provider_unknown_errors() {
        assert!("anthropic".parse::<AiProvider>().is_err());
    }

    // -- QueryStatus --

    #[test]
    fn query_status_roundtrip() {
        for variant in [
            QueryStatus::Idle,
            QueryStatus::Pending,
            QueryStatus::Running,
            QueryStatus::Completed,
            QueryStatus::Failed,
            QueryStatus::Cancelled,
        ] {
            let s = variant.to_string();
            assert_eq!(s.parse::<QueryStatus>().unwrap(), variant);
        }
    }

    #[test]
    fn query_status_is_terminal() {
        assert!(QueryStatus::Idle.is_terminal());
        assert!(QueryStatus::Completed.is_terminal());
        assert!(QueryStatus::Failed.is_terminal());
        assert!(QueryStatus::Cancelled.is_terminal());
        assert!(!QueryStatus::Pending.is_terminal());
        assert!(!QueryStatus::Running.is_terminal());
    }

    #[test]
    fn query_status_unknown_errors() {
        assert!("unknown".parse::<QueryStatus>().is_err());
    }

    // -- EnrichmentType --

    #[test]
    fn enrichment_type_roundtrip() {
        for variant in EnrichmentType::all() {
            let s = variant.to_string();
            assert_eq!(s.parse::<EnrichmentType>().unwrap(), *variant);
        }
    }

    #[test]
    fn enrichment_type_contribution_type_filter() {
        assert_eq!(
            EnrichmentType::ReviewDepth.contribution_type_filter(),
            ContributionType::PrReview
        );
        assert_eq!(
            EnrichmentType::Sentiment.contribution_type_filter(),
            ContributionType::PrReview
        );
        assert_eq!(
            EnrichmentType::Significance.contribution_type_filter(),
            ContributionType::PullRequest
        );
        assert_eq!(
            EnrichmentType::Topic.contribution_type_filter(),
            ContributionType::DiscourseTopic
        );
    }

    #[test]
    fn enrichment_type_unknown_errors() {
        assert!("unknown".parse::<EnrichmentType>().is_err());
    }
}
