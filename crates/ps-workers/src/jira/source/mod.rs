mod fetch;
mod plan;
mod store;

use async_trait::async_trait;
use ps_core::ingestion::{ContributionInput, FetchResult, IngestionContext, IngestionPlan, Source};
use serde::{Deserialize, Serialize};

/// Default lookback window when no watermark exists.
pub(crate) const DEFAULT_LOOKBACK_DAYS: i64 = 30;

/// Maximum results per JQL search page.
pub(crate) const MAX_RESULTS_PER_PAGE: i64 = 50;

/// Jira source adapter implementing the [`Source`] trait.
pub struct JiraSource;

/// Serialised cursor for tracking position within a Jira ingestion run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Cursor {
    /// JQL watermark filter value (ISO 8601 datetime).
    pub(crate) watermark: Option<String>,
    /// Configured project keys.
    pub(crate) projects: Vec<String>,
    /// Which project we're currently fetching (index into `projects`).
    #[serde(default)]
    pub(crate) project_index: usize,
    /// Cursor token for Jira's cursor-based pagination.
    pub(crate) next_page_token: Option<String>,
    /// Track the latest `updated` timestamp seen across all items.
    pub(crate) max_updated_at: Option<String>,
    /// Base URL for constructing issue URLs.
    pub(crate) base_url: String,
    /// Story points custom field name.
    pub(crate) story_points_field: Option<String>,
    /// API mode: "cloud" or "server".
    pub(crate) api_mode: String,
    /// Items that errored during this run (for failure isolation).
    #[serde(default)]
    pub(crate) failed_items: Vec<ps_core::ingestion::FailedItem>,
}

#[async_trait]
impl Source for JiraSource {
    fn name(&self) -> &'static str {
        "jira"
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
            projects: plan.items.clone(),
            project_index: 0,
            next_page_token: None,
            max_updated_at: plan.watermark.clone(),
            base_url: String::new(),
            story_points_field: None,
            api_mode: "cloud".into(),
            failed_items: vec![],
        };
        serde_json::to_string(&cursor).unwrap_or_default()
    }
}

/// Get the pre-decrypted Jira API token from `IngestionContext`.
///
/// The token is decrypted once per run in the handler (outside Restate
/// `ctx.run()` closures) to avoid journaling plaintext secrets.
pub(crate) fn decrypt_token(ctx: &IngestionContext) -> Result<String, ps_core::Error> {
    ctx.token
        .clone()
        .ok_or_else(|| ps_core::Error::Validation("Jira source has no api_token configured".into()))
}

/// Get the pre-decrypted Jira email from `IngestionContext`.
///
/// Returns `None` if no email was configured (e.g. Server/DC mode).
pub(crate) fn decrypt_email(ctx: &IngestionContext) -> Option<String> {
    ctx.email.clone()
}

pub(crate) fn serialise_cursor(cur: &Cursor) -> Result<String, ps_core::Error> {
    serde_json::to_string(cur)
        .map_err(|e| ps_core::Error::Internal(format!("cursor serialisation: {e}")))
}

/// Parse a Jira datetime string (ISO 8601 with timezone offset) into `OffsetDateTime`.
pub(crate) fn parse_jira_datetime(s: &str) -> Result<time::OffsetDateTime, ps_core::Error> {
    // Jira returns datetimes like "2024-01-15T10:30:00.000+0000"
    // Try RFC 3339 first, then fall back to a more lenient parse.
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .or_else(|_| {
            // Jira sometimes uses "+0000" instead of "+00:00"
            let normalized = normalize_jira_datetime(s);
            time::OffsetDateTime::parse(&normalized, &time::format_description::well_known::Rfc3339)
        })
        .map_err(|e| ps_core::Error::Internal(format!("invalid datetime '{s}': {e}")))
}

/// Normalize Jira datetime format to RFC 3339.
fn normalize_jira_datetime(s: &str) -> String {
    // Handle "+0000" → "+00:00" and similar timezone offsets without colon
    if let Some(pos) = s.rfind('+').or_else(|| {
        // Find the last '-' that's part of the timezone (after 'T')
        let t_pos = s.find('T')?;
        s[t_pos..].rfind('-').map(|p| t_pos + p)
    }) {
        let tz = &s[pos..];
        if tz.len() == 5 && !tz.contains(':') {
            return format!("{}{}:{}", &s[..pos], &tz[..3], &tz[3..]);
        }
    }
    s.to_string()
}
