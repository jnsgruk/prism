use ps_core::ingestion::{ContributionInput, SkippedDiff};
use ps_core::models::RateLimitInfo;

/// Journaled decision from each `fetch_store_loop` iteration.
///
/// `fetch_batch()` is not journaled (large responses), but its result
/// controls which journaled operations run next. By journaling this small
/// decision enum, the journal sequence becomes deterministic on replay
/// regardless of what `fetch_batch()` returns after a pod restart.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BatchAction {
    /// Rate limit exhausted, no items — sleep then retry same cursor.
    SleepForRateLimit {
        wait_secs: u64,
        /// Etag cursor update (Jira/Discourse) to apply before sleeping.
        #[serde(default)]
        etag_cursor: Option<String>,
    },
    /// Normal processing path.
    Process {
        item_count: usize,
        has_watermark: bool,
        next_cursor: Option<String>,
        #[serde(default)]
        etag_cursor: Option<String>,
        skipped_diffs: SkippedDiffAction,
    },
}

/// Sub-decision for handling PR diffs skipped due to REST rate limiting.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SkippedDiffAction {
    None,
    RetryOnly,
    SleepThenRetry { wait_secs: u64 },
}

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
    /// Secret key for the API token. `None` if this source has no token.
    pub token_key: Option<ps_core::models::SecretKey>,
    /// Whether the token is required (error if missing) vs optional.
    pub token_required: bool,
    /// Secret key for an email credential (Jira Basic auth).
    pub email_key: Option<ps_core::models::SecretKey>,
    /// Secret key for an API username (Discourse).
    pub api_username_key: Option<ps_core::models::SecretKey>,
    /// Noun for items in error summaries (e.g. `"repo"`, `"project"`, `"category"`).
    pub item_noun: &'static str,
}
