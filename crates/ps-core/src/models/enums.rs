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
/// identity is linked to.  Also used as the source-config "source type" since
/// there is currently a 1:1 mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Github,
    Launchpad,
    Mattermost,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Launchpad => "launchpad",
            Self::Mattermost => "mattermost",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        s.parse().ok()
    }
}

impl FromStr for Platform {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "github" => Ok(Self::Github),
            "launchpad" => Ok(Self::Launchpad),
            "mattermost" => Ok(Self::Mattermost),
            _ => Err(format!("invalid Platform: {s}")),
        }
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
        }
    }
}

impl FromStr for ContributionType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pull_request" => Ok(Self::PullRequest),
            "pr_review" => Ok(Self::PrReview),
            _ => Err(format!("invalid ContributionType: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// ContributionState
// ---------------------------------------------------------------------------

/// The state of a contribution (PR or review).
///
/// PR states are normalised to lowercase (`open`, `closed`, `merged`).
/// Review states come from the GitHub API (`APPROVED`, etc.) and are stored
/// in their original casing for compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionState {
    // PR states
    Open,
    Closed,
    Merged,
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

impl_sqlx_text!(Platform, |s: &str| s.parse().ok());
impl_sqlx_text!(ContributionType, |s: &str| s.parse().ok());
impl_sqlx_text!(ContributionState, |s: &str| s.parse().ok());
impl_sqlx_text!(IngestionStatus, |s: &str| s.parse().ok());
impl_sqlx_text!(PeriodType, |s: &str| s.parse().ok());
