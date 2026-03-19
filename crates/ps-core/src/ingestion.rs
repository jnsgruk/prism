use crate::Error;
use crate::models::{ContributionState, ContributionType, Platform, RateLimitInfo, SourceConfig};
use crate::repo::Repos;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Shared context provided to every source adapter during ingestion.
///
/// All secrets are pre-decrypted once at the start of the ingestion run
/// (outside Restate `ctx.run()` closures) so that plaintext material is
/// never journaled by the Restate runtime.
#[derive(Clone)]
pub struct IngestionContext {
    pub repos: Repos,
    pub source_config: SourceConfig,
    pub http_client: reqwest::Client,
    /// Pre-decrypted API token (GitHub PAT, Jira API token, Discourse API key).
    pub token: Option<String>,
    /// Pre-decrypted email for Jira Cloud Basic auth.
    pub email: Option<String>,
    /// Pre-decrypted API username for Discourse.
    pub api_username: Option<String>,
}

/// Known metric fields for a contribution. Stored as JSONB in the database.
///
/// Uses `#[serde(default)]` on all fields for forward compatibility — new fields
/// can be added without breaking existing serialized data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContributionMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_files: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_hours: Option<f64>,
}

/// Known metadata fields for a contribution. Stored as JSONB in the database.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContributionMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_platform_id: Option<String>,
}

/// A single contribution to upsert into `activity.contributions`.
///
/// The `platform_username` field is used for identity resolution:
/// we look up `org.platform_identities` to find the corresponding `person_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionInput {
    pub platform: Platform,
    pub contribution_type: ContributionType,
    pub platform_id: String,
    pub platform_username: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub state: Option<ContributionState>,
    pub created_at: OffsetDateTime,
    pub updated_at: Option<OffsetDateTime>,
    pub closed_at: Option<OffsetDateTime>,
    pub metrics: serde_json::Value,
    pub metadata: serde_json::Value,
    pub content: Option<String>,
    pub state_history: Option<serde_json::Value>,
    /// Structured content blob for enrichment queue. Populated during fetch,
    /// consumed during store to create enrichment queue entries. Not persisted
    /// on the contribution itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrichment_content: Option<serde_json::Value>,
}

/// The plan produced by a source adapter at the start of an ingestion run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionPlan {
    pub source_name: String,
    pub watermark: Option<String>,
    pub repos: Vec<RepoTarget>,
    /// Generic iteration targets (Jira project keys, Discourse category IDs).
    /// GitHub uses `repos` instead.
    #[serde(default)]
    pub items: Vec<String>,
}

/// An item (repo, project, category) that failed during an ingestion run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedItem {
    /// Human-readable identifier (e.g. "canonical/lxd", "PROJ-1", "category:5").
    pub key: String,
    /// Error message from the failed fetch.
    pub error: String,
}

/// A GitHub org/repo pair to fetch data from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoTarget {
    pub owner: String,
    pub repo: String,
}

/// Result of fetching a single batch of data from an external API.
#[derive(Debug, Clone)]
pub struct FetchResult {
    pub items: Vec<ContributionInput>,
    /// Opaque cursor for the next batch. `None` means no more data.
    pub next_cursor: Option<String>,
    pub rate_limit: Option<RateLimitInfo>,
    pub etag: Option<String>,
}

/// Orchestrator-agnostic interface for data sources.
///
/// Each method maps to a discrete retriable step in the Restate handler.
/// Business logic is kept here; the orchestrator controls retry policy,
/// timeouts, and checkpointing.
#[async_trait]
pub trait Source: Send + Sync {
    /// Human-readable name for logging and UI display.
    fn name(&self) -> &'static str;

    /// Determine what work needs to be done based on configuration and the
    /// current watermark.
    async fn plan(&self, ctx: &IngestionContext) -> Result<IngestionPlan, Error>;

    /// Fetch a single batch of data from the external API.
    ///
    /// `cursor` is an opaque string that the source interprets (e.g. serialised
    /// JSON with repo index + page number). On the first call, pass the initial
    /// cursor from the plan.
    async fn fetch_batch(&self, ctx: &IngestionContext, cursor: &str)
    -> Result<FetchResult, Error>;

    /// Persist a batch of fetched items to the database, resolving platform
    /// identities to `person_id` values.
    async fn store_batch(
        &self,
        ctx: &IngestionContext,
        items: &[ContributionInput],
    ) -> Result<usize, Error>;

    /// Update the watermark after a successful store, recording the new
    /// high-water mark and item count.
    async fn advance_watermark(
        &self,
        ctx: &IngestionContext,
        new_watermark: &str,
        items_collected: i32,
    ) -> Result<(), Error>;

    /// Return an initial cursor string for the given plan.
    ///
    /// The default implementation returns a JSON cursor starting at
    /// repo index 0, page 1, with the plan's watermark. Source
    /// implementations may override to include source-specific config
    /// (e.g. Jira projects, Discourse categories).
    fn initial_cursor(&self, ctx: &IngestionContext, plan: &IngestionPlan) -> String {
        let _ = ctx; // default impl doesn't need context
        serde_json::json!({
            "repo_index": 0,
            "page": 1,
            "watermark": plan.watermark,
        })
        .to_string()
    }

    /// Report current rate limit status. Returns `None` if not tracked.
    fn rate_limit_status(&self) -> Option<RateLimitInfo> {
        None
    }
}

/// Unique identifier for a contribution: `(platform, platform_id)`.
///
/// Used for identity resolution caching within a batch.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ContributionKey {
    pub platform: Platform,
    pub platform_id: String,
}

impl ContributionInput {
    pub fn key(&self) -> ContributionKey {
        ContributionKey {
            platform: self.platform.clone(),
            platform_id: self.platform_id.clone(),
        }
    }
}
