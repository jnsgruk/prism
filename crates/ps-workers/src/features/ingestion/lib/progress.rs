use ps_core::ingestion::{ContributionInput, SkippedDiff};
use ps_core::models::RateLimitInfo;

/// Serialisable fetch result for Restate journaling.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SerFetchResult {
    pub items: Vec<ContributionInput>,
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub rate_limit: Option<RateLimitInfo>,
    /// Carries the latest cursor state for watermark extraction, even when
    /// `next_cursor` is `None` (final batch). Used by Discourse ingestion.
    #[serde(default)]
    pub etag: Option<String>,
    /// PR diffs skipped due to REST rate limiting (GitHub only).
    #[serde(default)]
    pub skipped_diffs: Vec<SkippedDiff>,
}

/// Source-specific progress tracking for the fetch-store loop.
pub trait ProgressTracker {
    /// Count items from a fetched batch (e.g. increment PR/ticket/topic counter).
    fn count_batch(&mut self, items: &[ContributionInput], stored: i32);

    /// Build a progress JSON object from current counters and cursor state.
    fn build_progress(&self, cursor: &str, rate_limit: Option<&RateLimitInfo>)
    -> serde_json::Value;

    /// Build the final "complete" progress JSON.
    fn build_final_progress(&self) -> serde_json::Value;
}

/// Specification for an ingestion handler, describing which secrets to decrypt
/// and what to call the item type in error summaries.
pub struct IngestionSpec {
    pub handler_name: &'static str,
    /// Secret key name for the API token (e.g. `"api_token"`, `"api_key"`).
    /// `None` if this source has no token.
    pub token_key: Option<&'static str>,
    /// Whether the token is required (error if missing) vs optional.
    pub token_required: bool,
    /// Secret key name for an email credential (Jira Basic auth).
    pub email_key: Option<&'static str>,
    /// Secret key name for an API username (Discourse).
    pub api_username_key: Option<&'static str>,
    /// Noun for items in error summaries (e.g. `"repo"`, `"project"`, `"category"`).
    pub item_noun: &'static str,
}
