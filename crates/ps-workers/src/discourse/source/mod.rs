mod fetch;
mod plan;
mod store;

use async_trait::async_trait;
use ps_core::ingestion::{ContributionInput, FetchResult, IngestionContext, IngestionPlan, Source};
use serde::{Deserialize, Serialize};

/// Default lookback: ingest topics updated in the last 30 days when no watermark exists.
pub(crate) const DEFAULT_LOOKBACK_DAYS: i64 = 30;

/// Maximum number of topics to fetch per `/latest.json` page.
///
/// Discourse returns ~30 topics per page by default; we paginate until
/// we reach topics older than the watermark.
pub(crate) const MAX_PAGES_PER_RUN: u32 = 50;

/// Discourse source adapter implementing the [`Source`] trait.
pub struct DiscourseSource;

/// Serialised cursor for tracking position within a Discourse ingestion run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Cursor {
    /// ISO 8601 timestamp watermark — topics with `bumped_at` before this are skipped.
    pub(crate) watermark: Option<String>,
    /// Current page in `/latest.json` (0-indexed).
    pub(crate) page: u32,
    /// Category IDs to filter (empty = all).
    pub(crate) category_ids: Vec<i64>,
    /// Which category we're currently fetching (index into `category_ids`).
    #[serde(default)]
    pub(crate) category_index: usize,
    /// Minimum posts threshold.
    pub(crate) min_posts: i32,
    /// Base URL for constructing topic/post URLs.
    pub(crate) base_url: String,
    /// Instance suffix (e.g. `"ubuntu"` for `"discourse-ubuntu"`).
    pub(crate) instance: String,
    /// Track the latest `bumped_at` timestamp seen across all topics.
    pub(crate) max_bumped_at: Option<String>,
    /// Whether there are more pages to fetch.
    pub(crate) has_more: bool,
    /// Category ID → name map, fetched once on page 0 and reused across pages.
    #[serde(default)]
    pub(crate) category_map: std::collections::HashMap<i64, String>,
    /// Items that errored during this run (for failure isolation).
    #[serde(default)]
    pub(crate) failed_items: Vec<ps_core::ingestion::FailedItem>,
}

#[async_trait]
impl Source for DiscourseSource {
    fn name(&self) -> &'static str {
        "discourse"
    }

    async fn plan(&self, ctx: &IngestionContext) -> Result<IngestionPlan, ps_core::Error> {
        plan::plan_impl(ctx).await
    }

    async fn fetch_batch(
        &self,
        ctx: &IngestionContext,
        cursor: &str,
    ) -> Result<FetchResult, ps_core::Error> {
        fetch::fetch_batch_impl(ctx, cursor).await
    }

    async fn store_batch(
        &self,
        ctx: &IngestionContext,
        items: &[ContributionInput],
    ) -> Result<usize, ps_core::Error> {
        store::store_batch_impl(ctx, items).await
    }

    async fn advance_watermark(
        &self,
        ctx: &IngestionContext,
        new_watermark: &str,
        items_collected: i32,
    ) -> Result<(), ps_core::Error> {
        store::advance_watermark_impl(ctx, new_watermark, items_collected).await
    }

    fn initial_cursor(&self, ctx: &IngestionContext, plan: &IngestionPlan) -> String {
        let settings = &ctx.source_config.settings;

        let base_url = settings
            .get("base_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim_end_matches('/')
            .to_string();

        let categories: Vec<i64> = settings
            .get("categories")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let min_posts = settings
            .get("min_posts")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0) as i32;

        let instance = extract_instance(&ctx.source_config.name);

        let cursor = Cursor {
            watermark: plan.watermark.clone(),
            page: 0,
            category_ids: categories,
            category_index: 0,
            min_posts,
            base_url,
            instance,
            max_bumped_at: plan.watermark.clone(),
            has_more: true,
            category_map: std::collections::HashMap::new(),
            failed_items: vec![],
        };
        serde_json::to_string(&cursor).unwrap_or_default()
    }
}

/// Get the pre-decrypted Discourse API key from `IngestionContext`.
///
/// Returns an empty string if no API key is configured — Discourse public
/// endpoints work without authentication (with stricter rate limits).
pub(crate) fn decrypt_api_key(ctx: &IngestionContext) -> String {
    ctx.token.clone().unwrap_or_default()
}

/// Get the pre-decrypted Discourse API username from `IngestionContext`.
///
/// Defaults to `"system"` if not configured.
pub(crate) fn decrypt_api_username(ctx: &IngestionContext) -> String {
    ctx.api_username
        .clone()
        .unwrap_or_else(|| "system".to_string())
}

pub(crate) fn serialise_cursor(cur: &Cursor) -> Result<String, ps_core::Error> {
    serde_json::to_string(cur)
        .map_err(|e| ps_core::Error::Internal(format!("cursor serialisation: {e}")))
}

/// Extract the instance suffix from the source config name.
///
/// Source names follow the pattern `"discourse-{instance}"`.
pub(crate) fn extract_instance(source_name: &str) -> String {
    source_name
        .strip_prefix("discourse-")
        .unwrap_or(source_name)
        .to_string()
}
