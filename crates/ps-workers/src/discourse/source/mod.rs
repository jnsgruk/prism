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

    fn initial_cursor(&self, plan: &IngestionPlan) -> String {
        let cursor = Cursor {
            watermark: plan.watermark.clone(),
            page: 0,
            category_ids: vec![],
            min_posts: 0,
            base_url: String::new(),
            instance: String::new(),
            max_bumped_at: plan.watermark.clone(),
            has_more: true,
        };
        serde_json::to_string(&cursor).unwrap_or_default()
    }
}

/// Decrypt the Discourse API key from the source config secrets.
///
/// Returns an empty string if no API key is configured — Discourse public
/// endpoints work without authentication (with stricter rate limits).
pub(crate) async fn decrypt_api_key(ctx: &IngestionContext) -> Result<String, ps_core::Error> {
    if let Some(ref token) = ctx.token {
        return Ok(token.clone());
    }

    let encrypted = ctx
        .repos
        .config
        .get_encrypted_secret(ctx.source_config.id, "api_key")
        .await?;

    match encrypted {
        Some(enc) => {
            let decrypted = ps_core::crypto::decrypt(&ctx.secret_key, &enc)
                .map_err(|e| ps_core::Error::Encryption(e.to_string()))?;
            String::from_utf8(decrypted)
                .map_err(|e| ps_core::Error::Internal(format!("invalid api_key encoding: {e}")))
        }
        None => Ok(String::new()),
    }
}

/// Decrypt the optional Discourse API username from secrets.
pub(crate) async fn decrypt_api_username(ctx: &IngestionContext) -> Result<String, ps_core::Error> {
    let encrypted = ctx
        .repos
        .config
        .get_encrypted_secret(ctx.source_config.id, "api_username")
        .await?;

    match encrypted {
        Some(enc) => {
            let decrypted = ps_core::crypto::decrypt(&ctx.secret_key, &enc)
                .map_err(|e| ps_core::Error::Encryption(e.to_string()))?;
            String::from_utf8(decrypted)
                .map_err(|e| ps_core::Error::Internal(format!("invalid api_username: {e}")))
        }
        None => Ok("system".to_string()),
    }
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
