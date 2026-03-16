use std::collections::HashSet;
use std::fmt::Write as _;

use async_trait::async_trait;
use ps_core::ingestion::{
    ContributionInput, FetchResult, IngestionContext, IngestionPlan, RepoTarget, Source,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::graphql::GitHubGraphQLClient;
use super::repos;
use super::types::{GraphQLPr, GraphQLSearchPr};

/// Default lookback window when no watermark exists (non-backfill runs).
const DEFAULT_LOOKBACK_DAYS: i64 = 7;

/// Rate limit threshold below which the member search phase is skipped.
const RATE_LIMIT_SEARCH_THRESHOLD: i32 = 200;

/// GitHub source adapter implementing the [`Source`] trait.
///
/// Uses the GraphQL API for fetching PRs + reviews in a single query per page,
/// and for searching cross-repo contributions by team members.
pub struct GitHubSource;

/// Which phase of ingestion the cursor is in.
#[derive(Debug, Clone, Serialize, Deserialize)]
enum IngestionPhase {
    /// Iterate team-mapped (or discovered) repos, fetching PRs + reviews.
    TeamRepos,
    /// Search for cross-repo contributions by team members.
    MemberSearch,
}

/// Serialised cursor for tracking position within a multi-phase ingestion run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)]
struct Cursor {
    phase: IngestionPhase,
    // -- TeamRepos phase fields --
    repo_index: usize,
    /// GraphQL cursor for pagination within a repo.
    graphql_cursor: Option<String>,
    watermark: Option<String>,
    /// Cached list of repos so we don't re-discover mid-run.
    repos: Vec<RepoTarget>,
    /// Track the latest `updated_at` timestamp seen across all items.
    max_updated_at: Option<String>,
    /// Configured org names (needed for member search query building).
    orgs: Vec<String>,
    // -- MemberSearch phase fields --
    /// Index into `search_users` for the current batch.
    search_user_index: usize,
    /// GraphQL cursor for pagination within a search query.
    search_graphql_cursor: Option<String>,
    /// Usernames to search for cross-repo contributions.
    search_users: Vec<String>,
    /// Repos already ingested in the `TeamRepos` phase (owner/repo pairs).
    ingested_repos: HashSet<(String, String)>,
    /// Last rate limit remaining value (used to decide whether to skip search).
    last_rate_limit_remaining: Option<i32>,
}

#[async_trait]
impl Source for GitHubSource {
    fn name(&self) -> &'static str {
        "github"
    }

    async fn plan(&self, ctx: &IngestionContext) -> Result<IngestionPlan, ps_core::Error> {
        plan_impl(ctx).await
    }

    async fn fetch_batch(
        &self,
        ctx: &IngestionContext,
        cursor: &str,
    ) -> Result<FetchResult, ps_core::Error> {
        fetch_batch_impl(ctx, cursor).await
    }

    async fn store_batch(
        &self,
        ctx: &IngestionContext,
        items: &[ContributionInput],
    ) -> Result<usize, ps_core::Error> {
        store_batch_impl(ctx, items).await
    }

    async fn advance_watermark(
        &self,
        ctx: &IngestionContext,
        new_watermark: &str,
        items_collected: i32,
    ) -> Result<(), ps_core::Error> {
        advance_watermark_impl(ctx, new_watermark, items_collected).await
    }

    fn initial_cursor(&self, plan: &IngestionPlan) -> String {
        let orgs: Vec<String> = plan
            .source_name
            .as_str()
            .parse::<serde_json::Value>()
            .ok()
            .and(None) // not used this way; orgs are embedded by plan_impl
            .unwrap_or_default();

        let cursor = Cursor {
            phase: IngestionPhase::TeamRepos,
            repo_index: 0,
            graphql_cursor: None,
            watermark: plan.watermark.clone(),
            repos: plan.repos.clone(),
            max_updated_at: plan.watermark.clone(),
            orgs,
            search_user_index: 0,
            search_graphql_cursor: None,
            search_users: vec![],
            ingested_repos: HashSet::new(),
            last_rate_limit_remaining: None,
        };
        serde_json::to_string(&cursor).unwrap_or_default()
    }
}

async fn plan_impl(ctx: &IngestionContext) -> Result<IngestionPlan, ps_core::Error> {
    let settings = &ctx.source_config.settings;

    let orgs: Vec<String> = settings
        .get("orgs")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    if orgs.is_empty() {
        return Err(ps_core::Error::Validation(
            "GitHub source has no orgs configured".into(),
        ));
    }

    // Try to build repo list from team sync data (no API calls needed).
    let mapped_repos = ctx
        .repos
        .org
        .get_mapped_github_team_repos(ctx.source_config.id)
        .await?;

    let (final_repos, used_fallback) = if mapped_repos.is_empty() {
        // Fallback: no teams mapped yet, discover repos via REST (preserves
        // backwards compatibility for fresh setups before teams are configured).
        let exclude_archived = settings
            .get("exclude_archived")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        let exclude_repos: Vec<String> = settings
            .get("exclude_repos")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let token = decrypt_token(ctx).await?;
        let client = build_rest_client(ctx, &token);

        let discovered = repos::discover_repos(
            &client,
            &orgs,
            &ctx.repos.org,
            exclude_archived,
            &exclude_repos,
        )
        .await?;

        warn!(
            source = ctx.source_config.name,
            repos = discovered.len(),
            "no team mappings found — fell back to full org repo discovery"
        );

        (discovered, true)
    } else {
        let repos: Vec<RepoTarget> = mapped_repos
            .into_iter()
            .map(|(owner, repo)| RepoTarget { owner, repo })
            .collect();
        (repos, false)
    };

    // Load watermark. If none exists, default to 7 days ago.
    let watermark = ctx
        .repos
        .activity
        .get_watermark(&ctx.source_config.name)
        .await?;

    let effective_watermark = watermark.clone().or_else(|| {
        let seven_days_ago =
            time::OffsetDateTime::now_utc() - time::Duration::days(DEFAULT_LOOKBACK_DAYS);
        let wm = seven_days_ago
            .format(&time::format_description::well_known::Rfc3339)
            .ok();
        info!(
            source = ctx.source_config.name,
            default_watermark = ?wm,
            "no watermark found — defaulting to {DEFAULT_LOOKBACK_DAYS}-day lookback"
        );
        wm
    });

    info!(
        source = ctx.source_config.name,
        repos = final_repos.len(),
        watermark = ?effective_watermark,
        fallback_discovery = used_fallback,
        "planned GitHub ingestion"
    );

    Ok(IngestionPlan {
        source_name: ctx.source_config.name.clone(),
        watermark: effective_watermark,
        repos: final_repos,
    })
}

async fn fetch_batch_impl(
    ctx: &IngestionContext,
    cursor: &str,
) -> Result<FetchResult, ps_core::Error> {
    let mut cur: Cursor = serde_json::from_str(cursor)
        .map_err(|e| ps_core::Error::Internal(format!("invalid cursor: {e}")))?;

    match cur.phase {
        IngestionPhase::TeamRepos => fetch_team_repos(ctx, &mut cur).await,
        IngestionPhase::MemberSearch => fetch_member_search(ctx, &mut cur).await,
    }
}

/// Fetch PRs + reviews for team repos using GraphQL.
async fn fetch_team_repos(
    ctx: &IngestionContext,
    cur: &mut Cursor,
) -> Result<FetchResult, ps_core::Error> {
    let Some(repo_target) = cur.repos.get(cur.repo_index) else {
        // All repos exhausted — transition to member search phase.
        return transition_to_member_search(ctx, cur).await;
    };

    let owner = &repo_target.owner;
    let repo = &repo_target.repo;

    if cur.graphql_cursor.is_none() {
        info!(
            source = ctx.source_config.name,
            repo = %format!("{owner}/{repo}"),
            repo_index = cur.repo_index,
            repos_total = cur.repos.len(),
            "starting repo"
        );
    }

    let token = decrypt_token(ctx).await?;
    let client = build_graphql_client(ctx, &token);

    let page = client
        .fetch_pull_requests(owner, repo, cur.graphql_cursor.as_deref())
        .await
        .map_err(|e| ps_core::Error::Internal(format!("GitHub GraphQL error: {e}")))?;

    cur.last_rate_limit_remaining = Some(page.rate_limit.remaining);

    if page.rate_limit.remaining < RATE_LIMIT_SEARCH_THRESHOLD {
        warn!(
            remaining = page.rate_limit.remaining,
            reset = %page.rate_limit.reset_at,
            "GitHub rate limit low"
        );
    }

    // Track ingested repos for filtering in the search phase.
    cur.ingested_repos.insert((owner.clone(), repo.clone()));

    // Filter out PRs that are older than our watermark (since GraphQL doesn't
    // support a `since` filter, we rely on UPDATED_AT ASC ordering and skip
    // items updated before the watermark).
    let mut items = Vec::new();

    for pr in &page.items {
        // If this PR was updated before our watermark, skip it — but keep
        // paginating since the sort is ASC and later PRs may be newer.
        if let Some(ref wm) = cur.watermark
            && pr.updated_at <= *wm
        {
            continue;
        }

        // Track max_updated_at for watermark advancement.
        match cur.max_updated_at {
            Some(ref max) if pr.updated_at > *max => {
                cur.max_updated_at = Some(pr.updated_at.clone());
            }
            None => {
                cur.max_updated_at = Some(pr.updated_at.clone());
            }
            _ => {}
        }

        let author = pr.author.as_ref().map_or("", |a| a.login.as_str());
        items.extend(graphql_pr_to_contributions(owner, repo, pr, author)?);
    }

    // Determine next cursor.
    let next_cursor = if page.has_next_page {
        cur.graphql_cursor = page.end_cursor;
        Some(serialise_cursor(cur)?)
    } else {
        let pr_count = items
            .iter()
            .filter(|i| i.contribution_type == "pull_request")
            .count();
        let review_count = items
            .iter()
            .filter(|i| i.contribution_type == "pr_review")
            .count();
        info!(
            source = ctx.source_config.name,
            repo = %format!("{owner}/{repo}"),
            prs = pr_count,
            reviews = review_count,
            "completed repo"
        );

        // Move to next repo.
        cur.repo_index += 1;
        cur.graphql_cursor = None;
        Some(serialise_cursor(cur)?)
    };

    info!(
        source = ctx.source_config.name,
        repo = %format!("{owner}/{repo}"),
        items = items.len(),
        rate_limit_remaining = page.rate_limit.remaining,
        "fetched batch"
    );

    Ok(FetchResult {
        items,
        next_cursor,
        rate_limit: Some(page.rate_limit),
        etag: None,
    })
}

/// Transition from `TeamRepos` to `MemberSearch` phase.
async fn transition_to_member_search(
    ctx: &IngestionContext,
    cur: &mut Cursor,
) -> Result<FetchResult, ps_core::Error> {
    // Check if rate limit budget is sufficient for search.
    if let Some(remaining) = cur.last_rate_limit_remaining
        && remaining < RATE_LIMIT_SEARCH_THRESHOLD
    {
        warn!(
            source = ctx.source_config.name,
            remaining, "skipping member search — rate limit budget low"
        );
        return Ok(FetchResult {
            items: vec![],
            next_cursor: None,
            rate_limit: None,
            etag: None,
        });
    }

    // Load team member usernames for search.
    let usernames = ctx
        .repos
        .org
        .get_mapped_github_team_member_usernames(ctx.source_config.id)
        .await?;

    if usernames.is_empty() {
        info!(
            source = ctx.source_config.name,
            "no team members found — skipping member search"
        );
        return Ok(FetchResult {
            items: vec![],
            next_cursor: None,
            rate_limit: None,
            etag: None,
        });
    }

    // Read orgs from settings.
    let orgs: Vec<String> = ctx
        .source_config
        .settings
        .get("orgs")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    info!(
        source = ctx.source_config.name,
        users = usernames.len(),
        orgs = orgs.len(),
        "starting member search phase"
    );

    cur.phase = IngestionPhase::MemberSearch;
    cur.search_users = usernames;
    cur.search_user_index = 0;
    cur.search_graphql_cursor = None;
    cur.orgs = orgs;

    // Immediately start the first search batch.
    fetch_member_search(ctx, cur).await
}

/// Search for cross-repo contributions by team members using GraphQL search.
async fn fetch_member_search(
    ctx: &IngestionContext,
    cur: &mut Cursor,
) -> Result<FetchResult, ps_core::Error> {
    let Some(username) = cur.search_users.get(cur.search_user_index) else {
        // All users searched — we're done.
        info!(
            source = ctx.source_config.name,
            users_searched = cur.search_users.len(),
            "member search phase complete"
        );
        return Ok(FetchResult {
            items: vec![],
            next_cursor: None,
            rate_limit: None,
            etag: None,
        });
    };

    let token = decrypt_token(ctx).await?;
    let client = build_graphql_client(ctx, &token);

    // Build search query: author:{user} type:pr org:{org1} org:{org2} updated:>{watermark}
    let mut query = format!("author:{username} type:pr");
    for org in &cur.orgs {
        let _ = write!(query, " org:{org}");
    }
    if let Some(ref wm) = cur.watermark {
        let _ = write!(query, " updated:>{wm}");
    }

    let page = client
        .search_pull_requests(&query, cur.search_graphql_cursor.as_deref())
        .await
        .map_err(|e| ps_core::Error::Internal(format!("GitHub GraphQL search error: {e}")))?;

    cur.last_rate_limit_remaining = Some(page.rate_limit.remaining);

    // Convert search results to contributions, filtering out repos already ingested.
    let mut items = Vec::new();
    let mut cross_repo_count = 0u32;

    for search_pr in &page.items {
        let Some(ref repo_info) = search_pr.repository else {
            continue;
        };
        if search_pr.number.is_none() {
            continue;
        }

        let owner = &repo_info.owner.login;
        let repo = &repo_info.name;

        // Skip PRs in repos we already ingested.
        if cur.ingested_repos.contains(&(owner.clone(), repo.clone())) {
            continue;
        }

        cross_repo_count += 1;

        // Track max_updated_at.
        if let Some(ref updated_at) = search_pr.updated_at {
            match cur.max_updated_at {
                Some(ref max) if updated_at > max => {
                    cur.max_updated_at = Some(updated_at.clone());
                }
                None => {
                    cur.max_updated_at = Some(updated_at.clone());
                }
                _ => {}
            }
        }

        items.extend(search_pr_to_contributions(owner, repo, search_pr)?);
    }

    info!(
        source = ctx.source_config.name,
        user = username,
        results = page.items.len(),
        cross_repo_prs = cross_repo_count,
        rate_limit_remaining = page.rate_limit.remaining,
        "searched for member PRs"
    );

    // Determine next cursor.
    let next_cursor = if page.has_next_page {
        cur.search_graphql_cursor = page.end_cursor;
        Some(serialise_cursor(cur)?)
    } else {
        // Move to next user.
        cur.search_user_index += 1;
        cur.search_graphql_cursor = None;
        Some(serialise_cursor(cur)?)
    };

    Ok(FetchResult {
        items,
        next_cursor,
        rate_limit: Some(page.rate_limit),
        etag: None,
    })
}

/// Convert a GraphQL PR (from repo query) to `ContributionInputs`.
fn graphql_pr_to_contributions(
    owner: &str,
    repo: &str,
    pr: &GraphQLPr,
    author: &str,
) -> Result<Vec<ContributionInput>, ps_core::Error> {
    let mut items = Vec::new();

    let pr_state = if pr.merged_at.is_some() {
        "merged"
    } else {
        match pr.state.as_str() {
            "OPEN" => "open",
            "CLOSED" => "closed",
            "MERGED" => "merged",
            other => other,
        }
    };

    // Build state_history.
    let mut state_history = vec![serde_json::json!({
        "state": "open",
        "at": pr.created_at,
    })];
    if let Some(ref closed_at) = pr.closed_at {
        state_history.push(serde_json::json!({
            "state": pr_state,
            "at": closed_at,
        }));
    }

    let review_count = pr.reviews.nodes.len();
    let labels: Vec<&str> = pr
        .labels
        .as_ref()
        .map(|l| l.nodes.iter().map(|n| n.name.as_str()).collect())
        .unwrap_or_default();

    items.push(ContributionInput {
        platform: "github".into(),
        contribution_type: "pull_request".into(),
        platform_id: format!("{owner}/{repo}/pull/{}", pr.number),
        platform_username: author.to_string(),
        title: Some(pr.title.clone()),
        url: Some(pr.url.clone()),
        state: Some(pr_state.to_string()),
        created_at: parse_datetime(&pr.created_at)?,
        updated_at: Some(parse_datetime(&pr.updated_at)?),
        closed_at: pr.closed_at.as_deref().map(parse_datetime).transpose()?,
        metrics: serde_json::json!({
            "additions": pr.additions,
            "deletions": pr.deletions,
            "changed_files": pr.changed_files,
            "review_count": review_count,
            "draft": pr.is_draft,
        }),
        metadata: serde_json::json!({
            "head_ref": pr.head_ref_name,
            "base_ref": pr.base_ref_name,
            "labels": labels,
        }),
        content: None,
        state_history: Some(serde_json::Value::Array(state_history)),
    });

    // Map each review to a separate ContributionInput.
    for review in &pr.reviews.nodes {
        let reviewer = review.author.as_ref().map_or("", |a| a.login.as_str());

        let submitted_at = review
            .submitted_at
            .as_deref()
            .map(parse_datetime)
            .transpose()?;

        let review_id = review.database_id.unwrap_or(0);

        items.push(ContributionInput {
            platform: "github".into(),
            contribution_type: "pr_review".into(),
            platform_id: format!("{owner}/{repo}/review/{review_id}"),
            platform_username: reviewer.to_string(),
            title: Some(format!("Review on #{}", pr.number)),
            url: Some(format!("{}/reviews/{review_id}", pr.url)),
            state: Some(review.state.clone()),
            created_at: submitted_at.unwrap_or(parse_datetime(&pr.created_at)?),
            updated_at: submitted_at,
            closed_at: None,
            metrics: serde_json::json!({
                "review_state": review.state,
            }),
            metadata: serde_json::json!({
                "pr_number": pr.number,
                "pr_platform_id": format!("{owner}/{repo}/pull/{}", pr.number),
            }),
            content: review.body.clone(),
            state_history: None,
        });
    }

    Ok(items)
}

/// Convert a GraphQL search PR to `ContributionInputs`.
fn search_pr_to_contributions(
    owner: &str,
    repo: &str,
    pr: &GraphQLSearchPr,
) -> Result<Vec<ContributionInput>, ps_core::Error> {
    let number = pr.number.unwrap_or(0);
    let title = pr.title.clone().unwrap_or_default();
    let url = pr.url.clone().unwrap_or_default();
    let created_at_str = pr.created_at.as_deref().unwrap_or("1970-01-01T00:00:00Z");
    let updated_at_str = pr.updated_at.as_deref().unwrap_or("1970-01-01T00:00:00Z");
    let author = pr.author.as_ref().map_or("", |a| a.login.as_str());

    let pr_state = if pr.merged_at.is_some() {
        "merged"
    } else {
        match pr.state.as_deref().unwrap_or("OPEN") {
            "OPEN" => "open",
            "CLOSED" => "closed",
            "MERGED" => "merged",
            other => other,
        }
    };

    let mut items = Vec::new();

    let mut state_history = vec![serde_json::json!({
        "state": "open",
        "at": created_at_str,
    })];
    if let Some(ref closed_at) = pr.closed_at {
        state_history.push(serde_json::json!({
            "state": pr_state,
            "at": closed_at,
        }));
    }

    let review_count = pr.reviews.as_ref().map_or(0, |r| r.nodes.len());
    let labels: Vec<&str> = pr
        .labels
        .as_ref()
        .map(|l| l.nodes.iter().map(|n| n.name.as_str()).collect())
        .unwrap_or_default();

    items.push(ContributionInput {
        platform: "github".into(),
        contribution_type: "pull_request".into(),
        platform_id: format!("{owner}/{repo}/pull/{number}"),
        platform_username: author.to_string(),
        title: Some(title),
        url: Some(url.clone()),
        state: Some(pr_state.to_string()),
        created_at: parse_datetime(created_at_str)?,
        updated_at: Some(parse_datetime(updated_at_str)?),
        closed_at: pr.closed_at.as_deref().map(parse_datetime).transpose()?,
        metrics: serde_json::json!({
            "additions": pr.additions,
            "deletions": pr.deletions,
            "changed_files": pr.changed_files,
            "review_count": review_count,
            "draft": pr.is_draft.unwrap_or(false),
        }),
        metadata: serde_json::json!({
            "head_ref": pr.head_ref_name,
            "base_ref": pr.base_ref_name,
            "labels": labels,
        }),
        content: None,
        state_history: Some(serde_json::Value::Array(state_history)),
    });

    // Map reviews.
    if let Some(reviews) = &pr.reviews {
        for review in &reviews.nodes {
            let reviewer = review.author.as_ref().map_or("", |a| a.login.as_str());

            let submitted_at = review
                .submitted_at
                .as_deref()
                .map(parse_datetime)
                .transpose()?;

            let review_id = review.database_id.unwrap_or(0);

            items.push(ContributionInput {
                platform: "github".into(),
                contribution_type: "pr_review".into(),
                platform_id: format!("{owner}/{repo}/review/{review_id}"),
                platform_username: reviewer.to_string(),
                title: Some(format!("Review on #{number}")),
                url: Some(format!("{url}/reviews/{review_id}")),
                state: Some(review.state.clone()),
                created_at: submitted_at.unwrap_or(parse_datetime(created_at_str)?),
                updated_at: submitted_at,
                closed_at: None,
                metrics: serde_json::json!({
                    "review_state": review.state,
                }),
                metadata: serde_json::json!({
                    "pr_number": number,
                    "pr_platform_id": format!("{owner}/{repo}/pull/{number}"),
                }),
                content: review.body.clone(),
                state_history: None,
            });
        }
    }

    Ok(items)
}

async fn store_batch_impl(
    ctx: &IngestionContext,
    items: &[ContributionInput],
) -> Result<usize, ps_core::Error> {
    if items.is_empty() {
        return Ok(0);
    }

    // Collect unique usernames for batch identity resolution.
    let usernames: Vec<String> = items
        .iter()
        .map(|i| i.platform_username.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let person_map = ctx
        .repos
        .org
        .batch_resolve_person_ids("github", &usernames)
        .await?;

    let mut stored = 0usize;
    let mut skipped = 0usize;
    for item in items {
        let Some(person_id) = person_map.get(&item.platform_username).copied() else {
            skipped += 1;
            continue;
        };
        let id = Uuid::now_v7();

        ctx.repos
            .activity
            .upsert_contribution(id, Some(person_id), item)
            .await?;

        stored += 1;
    }

    if skipped > 0 {
        info!(
            source = ctx.source_config.name,
            stored,
            skipped_identities = skipped,
            "stored batch with unresolved identities"
        );
    } else {
        debug!(stored, "stored batch");
    }

    Ok(stored)
}

async fn advance_watermark_impl(
    ctx: &IngestionContext,
    new_watermark: &str,
    items_collected: i32,
) -> Result<(), ps_core::Error> {
    let old_watermark = ctx
        .repos
        .activity
        .get_watermark(&ctx.source_config.name)
        .await?;

    ctx.repos
        .activity
        .upsert_watermark(&ctx.source_config.name, new_watermark, items_collected)
        .await?;

    info!(
        source = ctx.source_config.name,
        old_watermark = ?old_watermark,
        new_watermark = new_watermark,
        items_collected,
        "advanced watermark"
    );
    Ok(())
}

/// Build a `GitHubGraphQLClient` from the ingestion context and a decrypted token.
fn build_graphql_client(ctx: &IngestionContext, token: &str) -> GitHubGraphQLClient {
    let base_url = ctx
        .source_config
        .settings
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("https://api.github.com");
    GitHubGraphQLClient::new(ctx.http_client.clone(), base_url, token)
}

/// Build a REST `GitHubClient` for the fallback repo discovery path.
fn build_rest_client(ctx: &IngestionContext, token: &str) -> super::client::GitHubClient {
    let base_url = ctx
        .source_config
        .settings
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("https://api.github.com");
    super::client::GitHubClient::new(ctx.http_client.clone(), base_url, token)
}

/// Decrypt the GitHub API token from the source's encrypted secrets.
async fn decrypt_token(ctx: &IngestionContext) -> Result<String, ps_core::Error> {
    let encrypted = ctx
        .repos
        .config
        .get_encrypted_secret(ctx.source_config.id, "api_token")
        .await?
        .ok_or_else(|| {
            ps_core::Error::Validation("GitHub source has no api_token configured".into())
        })?;

    let decrypted = ps_core::crypto::decrypt(&ctx.secret_key, &encrypted)
        .map_err(|e| ps_core::Error::Encryption(e.to_string()))?;

    String::from_utf8(decrypted)
        .map_err(|e| ps_core::Error::Internal(format!("invalid token encoding: {e}")))
}

fn serialise_cursor(cur: &Cursor) -> Result<String, ps_core::Error> {
    serde_json::to_string(cur)
        .map_err(|e| ps_core::Error::Internal(format!("cursor serialisation: {e}")))
}

/// Parse an ISO 8601 datetime string into `OffsetDateTime`.
fn parse_datetime(s: &str) -> Result<time::OffsetDateTime, ps_core::Error> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| ps_core::Error::Internal(format!("invalid datetime '{s}': {e}")))
}
