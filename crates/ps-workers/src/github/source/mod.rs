mod fetch;
mod plan;
mod store;

use std::collections::HashSet;

use async_trait::async_trait;
use ps_core::ingestion::{
    ContributionInput, FetchResult, IngestionContext, IngestionPlan, RepoTarget, Source,
};
use serde::{Deserialize, Serialize};

use super::graphql::GitHubGraphQLClient;

/// Default lookback window when no watermark exists (non-backfill runs).
pub(super) const DEFAULT_LOOKBACK_DAYS: i64 = 7;

/// Rate limit threshold below which the member search phase is skipped.
pub(super) const RATE_LIMIT_SEARCH_THRESHOLD: i32 = 200;

/// Number of usernames to batch into a single GraphQL search query.
/// GitHub search supports multiple `author:` terms with OR semantics.
pub(super) const SEARCH_BATCH_SIZE: usize = 5;

/// Check whether a GitHub username matches the expected format.
///
/// GitHub usernames may contain alphanumerics and hyphens only. We reject
/// anything else to prevent GraphQL query injection via crafted usernames.
pub(super) fn is_valid_github_username(username: &str) -> bool {
    !username.is_empty()
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// GitHub source adapter implementing the [`Source`] trait.
///
/// Uses the GraphQL API for fetching PRs + reviews in a single query per page,
/// and for searching cross-repo contributions by team members.
pub struct GitHubSource;

/// Which phase of ingestion the cursor is in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum IngestionPhase {
    /// Iterate team-mapped (or discovered) repos, fetching PRs + reviews.
    TeamRepos,
    /// Search for cross-repo contributions by team members.
    MemberSearch,
}

/// Serialised cursor for tracking position within a multi-phase ingestion run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)]
pub(super) struct Cursor {
    pub(super) phase: IngestionPhase,
    // -- TeamRepos phase fields --
    pub(super) repo_index: usize,
    /// GraphQL cursor for pagination within a repo.
    pub(super) graphql_cursor: Option<String>,
    pub(super) watermark: Option<String>,
    /// Cached list of repos so we don't re-discover mid-run.
    pub(super) repos: Vec<RepoTarget>,
    /// Track the latest `updated_at` timestamp seen across all items.
    pub(super) max_updated_at: Option<String>,
    /// Configured org names (needed for member search query building).
    pub(super) orgs: Vec<String>,
    // -- MemberSearch phase fields --
    /// Index into `search_users` for the current batch.
    pub(super) search_user_index: usize,
    /// GraphQL cursor for pagination within a search query.
    pub(super) search_graphql_cursor: Option<String>,
    /// Usernames to search for cross-repo contributions.
    pub(super) search_users: Vec<String>,
    /// Repos already ingested in the `TeamRepos` phase ("owner/repo" keys).
    pub(super) ingested_repos: HashSet<String>,
    /// Last rate limit remaining value (used to decide whether to skip search).
    pub(super) last_rate_limit_remaining: Option<i32>,
    /// Items (repos) that errored during this run (for failure isolation).
    #[serde(default)]
    pub(super) failed_items: Vec<ps_core::ingestion::FailedItem>,
}

#[async_trait]
impl Source for GitHubSource {
    fn name(&self) -> &'static str {
        "github"
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

    fn initial_cursor(&self, _ctx: &IngestionContext, plan: &IngestionPlan) -> String {
        let cursor = Cursor {
            phase: IngestionPhase::TeamRepos,
            repo_index: 0,
            graphql_cursor: None,
            watermark: plan.watermark.clone(),
            repos: plan.repos.clone(),
            max_updated_at: plan.watermark.clone(),
            orgs: vec![],
            search_user_index: 0,
            search_graphql_cursor: None,
            search_users: vec![],
            ingested_repos: HashSet::new(),
            last_rate_limit_remaining: None,
            failed_items: vec![],
        };
        serde_json::to_string(&cursor).unwrap_or_default()
    }
}

/// Build a `GitHubGraphQLClient` from the ingestion context and a decrypted token.
pub(super) fn build_graphql_client(ctx: &IngestionContext, token: &str) -> GitHubGraphQLClient {
    let base_url = ctx
        .source_config
        .settings
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("https://api.github.com");
    GitHubGraphQLClient::new(ctx.http_client.clone(), base_url, token)
}

/// Build a REST `GitHubClient` for the fallback repo discovery path.
pub(super) fn build_rest_client(
    ctx: &IngestionContext,
    token: &str,
) -> super::client::GitHubClient {
    let base_url = ctx
        .source_config
        .settings
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("https://api.github.com");
    super::client::GitHubClient::new(ctx.http_client.clone(), base_url, token)
}

/// Get the pre-decrypted API token from `IngestionContext`.
///
/// The token is decrypted once per run in the handler (outside Restate
/// `ctx.run()` closures) to avoid journaling plaintext secrets.
pub(super) fn decrypt_token(ctx: &IngestionContext) -> Result<String, ps_core::Error> {
    ctx.token.clone().ok_or_else(|| {
        ps_core::Error::Validation("GitHub source has no api_token configured".into())
    })
}

pub(super) fn serialise_cursor(cur: &Cursor) -> Result<String, ps_core::Error> {
    serde_json::to_string(cur)
        .map_err(|e| ps_core::Error::Internal(format!("cursor serialisation: {e}")))
}

/// Parse an ISO 8601 datetime string into `OffsetDateTime`.
pub(super) fn parse_datetime(s: &str) -> Result<time::OffsetDateTime, ps_core::Error> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| ps_core::Error::Internal(format!("invalid datetime '{s}': {e}")))
}
