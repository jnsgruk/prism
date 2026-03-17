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
