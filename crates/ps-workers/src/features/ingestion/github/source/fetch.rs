use std::fmt::Write as _;

use ps_core::ingestion::{
    ContributionInput, ContributionMetadata, ContributionMetrics, FetchResult, IngestionContext,
};
use ps_core::models::{ContributionState, ContributionType, Platform, RateLimitInfo};
use tracing::{debug, warn};

use super::super::client::GitHubClient;
use super::super::types::GraphQLSearchPr;
use super::{
    Cursor, IngestionPhase, RATE_LIMIT_SEARCH_THRESHOLD, SEARCH_BATCH_SIZE, build_graphql_client,
    decrypt_token, is_valid_github_username, parse_datetime, serialise_cursor,
};
use crate::infra::retry::retry_transient;
use ps_core::ingestion::FailedItem;

/// Max size for combined PR diff content stored in enrichment queue (~20KB).
const MAX_DIFF_SIZE: usize = 20_000;

/// When REST rate limit remaining drops below this, pause until reset.
const REST_RATE_LIMIT_FLOOR: i32 = 50;

pub(super) async fn fetch_batch_impl(
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

/// Fetch PRs + reviews for team repos using GraphQL search.
///
/// Uses the `search` query with `repo:{owner}/{repo} type:pr updated:>{watermark}`
/// so GitHub filters server-side rather than us paginating through all history.
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
        debug!(
            repo = %format!("{owner}/{repo}"),
            repo_index = cur.repo_index,
            repos_total = cur.repos.len(),
            "starting repo"
        );
    }

    let token = decrypt_token(ctx)?;
    let client = build_graphql_client(ctx, &token);

    // Build search query with server-side updated filter.
    let mut query = format!("repo:{owner}/{repo} type:pr");
    if let Some(ref wm) = cur.watermark
        && !wm.is_empty()
    {
        let _ = write!(query, " updated:>{wm}");
    }

    debug!(
        %query,
        "executing GitHub search query"
    );

    let graphql_cursor = cur.graphql_cursor.as_deref().map(String::from);
    let page = match retry_transient(
        &format!("repo {owner}/{repo}"),
        super::super::graphql::GraphQLClientError::is_transient,
        || client.search_pull_requests(&query, graphql_cursor.as_deref()),
    )
    .await
    {
        Ok(page) => page,
        Err(ref e @ super::super::graphql::GraphQLClientError::GraphQL { ref rate_limit, .. })
            if e.to_string().contains("rate limit") =>
        {
            // GraphQL rate limit exhausted — return with rate_limit info so
            // fetch_store_loop can ctx.sleep() durably. Don't advance cursor.
            warn!(
                source = ctx.source_config.name,
                repo = %format!("{owner}/{repo}"),
                "GraphQL rate limit exhausted, deferring for durable sleep"
            );
            let mut rl = rate_limit.clone().unwrap_or(RateLimitInfo {
                remaining: 0,
                limit: 5000,
                reset_at: time::OffsetDateTime::now_utc() + time::Duration::hours(1),
            });
            // GraphQL rate limit is cost-based — the API can reject a query
            // even when `remaining > 0` (insufficient points for the query
            // cost).  Force remaining to 0 so `compute_batch_action` triggers
            // a durable sleep instead of spinning in a tight retry loop.
            rl.remaining = 0;
            return Ok(FetchResult {
                items: vec![],
                next_cursor: Some(serialise_cursor(cur)?),
                rate_limit: Some(rl),
                display_rate_limit: None,
                etag: None,
                skipped_diffs: vec![],
            });
        }
        Err(e) => {
            warn!(
                source = ctx.source_config.name,
                repo = %format!("{owner}/{repo}"),
                error = %e,
                "skipping repo due to fetch error"
            );
            cur.failed_items.push(FailedItem {
                key: format!("{owner}/{repo}"),
                error: e.to_string(),
            });
            cur.repo_index += 1;
            cur.graphql_cursor = None;
            return Ok(FetchResult {
                items: vec![],
                next_cursor: Some(serialise_cursor(cur)?),
                rate_limit: None,
                display_rate_limit: None,
                etag: None,
                skipped_diffs: vec![],
            });
        }
    };

    debug!(
        results = page.items.len(),
        rate_limit = page.rate_limit.remaining,
        "GitHub search query returned"
    );

    cur.last_rate_limit_remaining = Some(page.rate_limit.remaining);

    if page.rate_limit.remaining < RATE_LIMIT_SEARCH_THRESHOLD {
        warn!(
            remaining = page.rate_limit.remaining,
            reset = %page.rate_limit.reset_at,
            "GitHub rate limit low"
        );
    }

    // Track ingested repos for filtering in the search phase.
    cur.ingested_repos.insert(format!("{owner}/{repo}"));

    let mut items = Vec::new();

    for search_pr in &page.items {
        let Some(ref updated_at) = search_pr.updated_at else {
            continue;
        };

        // Track max_updated_at for watermark advancement.
        if cur
            .max_updated_at
            .as_ref()
            .is_none_or(|max| updated_at > max)
        {
            cur.max_updated_at = Some(updated_at.clone());
        }

        items.extend(search_pr_to_contributions(owner, repo, search_pr)?);
    }

    // Fetch PR diffs concurrently and attach to enrichment content.
    let diff_outcome = fetch_pr_diffs(ctx, &mut items).await;

    // Determine next cursor.
    let next_cursor = if page.has_next_page {
        cur.graphql_cursor = page.end_cursor;
        Some(serialise_cursor(cur)?)
    } else {
        let pr_count = items
            .iter()
            .filter(|i| i.contribution_type == ContributionType::PullRequest)
            .count();
        let review_count = items
            .iter()
            .filter(|i| i.contribution_type == ContributionType::PrReview)
            .count();
        debug!(
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

    debug!(
        repo = %format!("{owner}/{repo}"),
        items = items.len(),
        rate_limit_remaining = page.rate_limit.remaining,
        "fetched batch"
    );

    Ok(FetchResult {
        items,
        next_cursor,
        rate_limit: diff_outcome.rate_limit.or(Some(page.rate_limit.clone())),
        display_rate_limit: Some(page.rate_limit),
        etag: None,
        skipped_diffs: diff_outcome.skipped,
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
            display_rate_limit: None,
            etag: None,
            skipped_diffs: vec![],
        });
    }

    // Load all GitHub usernames for active team members — includes users from
    // teams without a GitHub team mapping.
    let usernames = ctx.repos.org.get_all_github_team_member_usernames().await?;

    if usernames.is_empty() {
        debug!("no team members found — skipping member search");
        return Ok(FetchResult {
            items: vec![],
            next_cursor: None,
            rate_limit: None,
            display_rate_limit: None,
            etag: None,
            skipped_diffs: vec![],
        });
    }

    // Read orgs from settings.
    let orgs: Vec<String> = ctx
        .source_config
        .settings
        .get("orgs")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    debug!(
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
///
/// Batches multiple usernames into a single query using OR semantics
/// (`author:u1 author:u2 ...`) to reduce the number of API calls.
async fn fetch_member_search(
    ctx: &IngestionContext,
    cur: &mut Cursor,
) -> Result<FetchResult, ps_core::Error> {
    if cur.search_user_index >= cur.search_users.len() {
        // All users searched — we're done.
        debug!(
            users_searched = cur.search_users.len(),
            "member search phase complete"
        );
        return Ok(FetchResult {
            items: vec![],
            next_cursor: None,
            rate_limit: None,
            display_rate_limit: None,
            etag: None,
            skipped_diffs: vec![],
        });
    }

    let token = decrypt_token(ctx)?;
    let client = build_graphql_client(ctx, &token);

    // Build search query with a batch of usernames.
    let batch_end = (cur.search_user_index + SEARCH_BATCH_SIZE).min(cur.search_users.len());
    let batch = cur
        .search_users
        .get(cur.search_user_index..batch_end)
        .unwrap_or_default();

    let mut query = String::from("type:pr");
    for user in batch {
        if !is_valid_github_username(user) {
            warn!(username = %user, "skipping username with invalid characters in member search");
            continue;
        }
        let _ = write!(query, " author:{user}");
    }
    for org in &cur.orgs {
        let _ = write!(query, " org:{org}");
    }
    if let Some(ref wm) = cur.watermark
        && !wm.is_empty()
    {
        let _ = write!(query, " updated:>{wm}");
    }

    let search_cursor = cur.search_graphql_cursor.as_deref().map(String::from);
    let page = match retry_transient(
        "member search",
        super::super::graphql::GraphQLClientError::is_transient,
        || client.search_pull_requests(&query, search_cursor.as_deref()),
    )
    .await
    {
        Ok(page) => page,
        Err(ref e @ super::super::graphql::GraphQLClientError::GraphQL { ref rate_limit, .. })
            if e.to_string().contains("rate limit") =>
        {
            warn!("GraphQL rate limit exhausted during member search, deferring for durable sleep");
            let mut rl = rate_limit.clone().unwrap_or(RateLimitInfo {
                remaining: 0,
                limit: 5000,
                reset_at: time::OffsetDateTime::now_utc() + time::Duration::hours(1),
            });
            // Force remaining to 0 — see team-repos handler comment above.
            rl.remaining = 0;
            return Ok(FetchResult {
                items: vec![],
                next_cursor: Some(serialise_cursor(cur)?),
                rate_limit: Some(rl),
                display_rate_limit: None,
                etag: None,
                skipped_diffs: vec![],
            });
        }
        Err(e) => {
            let batch_desc = batch.join(", ");
            warn!(
                source = ctx.source_config.name,
                users = %batch_desc,
                error = %e,
                "skipping user batch due to search error"
            );
            cur.failed_items.push(FailedItem {
                key: format!("member_search[{batch_desc}]"),
                error: e.to_string(),
            });
            cur.search_user_index = batch_end;
            cur.search_graphql_cursor = None;
            return Ok(FetchResult {
                items: vec![],
                next_cursor: Some(serialise_cursor(cur)?),
                rate_limit: None,
                display_rate_limit: None,
                etag: None,
                skipped_diffs: vec![],
            });
        }
    };

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
        if cur.ingested_repos.contains(&format!("{owner}/{repo}")) {
            continue;
        }

        cross_repo_count += 1;

        // Track max_updated_at.
        if let Some(ref updated_at) = search_pr.updated_at
            && cur
                .max_updated_at
                .as_ref()
                .is_none_or(|max| updated_at > max)
        {
            cur.max_updated_at = Some(updated_at.clone());
        }

        items.extend(search_pr_to_contributions(owner, repo, search_pr)?);
    }

    // Fetch PR diffs concurrently and attach to enrichment content.
    let diff_outcome = fetch_pr_diffs(ctx, &mut items).await;

    debug!(
        batch_start = cur.search_user_index,
        batch_end,
        results = page.items.len(),
        cross_repo_prs = cross_repo_count,
        rate_limit_remaining = page.rate_limit.remaining,
        "searched for member PRs"
    );

    // Determine next cursor.
    let next_cursor = if page.has_next_page {
        // More pages for this batch of users.
        cur.search_graphql_cursor = page.end_cursor;
        Some(serialise_cursor(cur)?)
    } else {
        // Move to next batch of users.
        cur.search_user_index = batch_end;
        cur.search_graphql_cursor = None;
        Some(serialise_cursor(cur)?)
    };

    Ok(FetchResult {
        items,
        next_cursor,
        rate_limit: diff_outcome.rate_limit.or(Some(page.rate_limit.clone())),
        display_rate_limit: Some(page.rate_limit),
        etag: None,
        skipped_diffs: diff_outcome.skipped,
    })
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
        ContributionState::Merged
    } else {
        match pr.state.as_deref().unwrap_or("OPEN") {
            "CLOSED" => ContributionState::Closed,
            "MERGED" => ContributionState::Merged,
            _ => ContributionState::Open,
        }
    };

    let mut items = Vec::new();

    let mut state_history = vec![serde_json::json!({
        "state": ContributionState::Open.as_str(),
        "at": created_at_str,
    })];
    if let Some(ref closed_at) = pr.closed_at {
        state_history.push(serde_json::json!({
            "state": pr_state.as_str(),
            "at": closed_at,
        }));
    }

    let review_count = pr.reviews.as_ref().map_or(0, |r| r.nodes.len());
    let labels: Vec<&str> = pr
        .labels
        .as_ref()
        .map(|l| l.nodes.iter().map(|n| n.name.as_str()).collect())
        .unwrap_or_default();

    // Build enrichment content blob for this PR.
    // Diff will be attached later by fetch_pr_diffs().
    // Only PRs with >50 lines changed are eligible for significance enrichment
    // (the only enrichment type targeting pull_requests), so skip small PRs.
    let lines_changed = pr.additions.unwrap_or(0) + pr.deletions.unwrap_or(0);
    let pr_enrichment = if lines_changed > 50 {
        Some(serde_json::json!({
            "title": &title,
            "description": pr.body_text.as_deref().unwrap_or(""),
            "labels": labels,
            "additions": pr.additions.unwrap_or(0),
            "deletions": pr.deletions.unwrap_or(0),
            "changed_files": pr.changed_files.unwrap_or(0),
            "draft": pr.is_draft.unwrap_or(false),
        }))
    } else {
        None
    };

    // Clone title before moving it — reviews need pr_title.
    let pr_title_for_reviews = title.clone();

    items.push(ContributionInput {
        platform: Platform::Github,
        contribution_type: ContributionType::PullRequest,
        platform_id: format!("{owner}/{repo}/pull/{number}").into(),
        platform_username: author.to_lowercase().into(),
        title: Some(title),
        url: Some(url.clone()),
        state: Some(pr_state),
        created_at: parse_datetime(created_at_str)?,
        updated_at: Some(parse_datetime(updated_at_str)?),
        closed_at: pr.closed_at.as_deref().map(parse_datetime).transpose()?,
        #[allow(clippy::cast_possible_wrap)]
        metrics: serde_json::to_value(ContributionMetrics {
            additions: pr.additions.map(|v| v as i32),
            deletions: pr.deletions.map(|v| v as i32),
            changed_files: pr.changed_files.map(|v| v as i32),
            review_count: Some(review_count as i32),
            draft: Some(pr.is_draft.unwrap_or(false)),
            ..Default::default()
        })
        .unwrap_or_default(),
        metadata: serde_json::to_value(ContributionMetadata {
            repo: Some(format!("{owner}/{repo}")),
            head_ref: pr.head_ref_name.clone(),
            base_ref: pr.base_ref_name.clone(),
            labels: if labels.is_empty() {
                None
            } else {
                Some(labels.into_iter().map(String::from).collect())
            },
            ..Default::default()
        })
        .unwrap_or_default(),
        content: None,
        state_history: Some(serde_json::Value::Array(state_history)),
        enrichment_content: pr_enrichment,
    });

    // Map reviews.
    if let Some(reviews) = &pr.reviews {
        for review in &reviews.nodes {
            items.push(search_review_to_contribution(
                owner,
                repo,
                number,
                &url,
                &pr_title_for_reviews,
                created_at_str,
                review,
            )?);
        }
    }

    Ok(items)
}

/// Convert a single GraphQL review into a `ContributionInput`.
fn search_review_to_contribution(
    owner: &str,
    repo: &str,
    pr_number: u32,
    pr_url: &str,
    pr_title: &str,
    pr_created_at: &str,
    review: &super::super::types::GraphQLReview,
) -> Result<ContributionInput, ps_core::Error> {
    let reviewer = review.author.as_ref().map_or("", |a| a.login.as_str());

    let submitted_at = review
        .submitted_at
        .as_deref()
        .map(parse_datetime)
        .transpose()?;

    let review_id = review.database_id.unwrap_or(0);
    let review_state = ContributionState::from_str_opt(&review.state);

    let inline_comments: Vec<serde_json::Value> = review
        .comments
        .as_ref()
        .map(|c| {
            c.nodes
                .iter()
                .filter_map(|comment| {
                    let body = comment.body.as_deref().unwrap_or("");
                    if body.is_empty() {
                        return None;
                    }
                    Some(serde_json::json!({
                        "path": comment.path.as_deref().unwrap_or(""),
                        "body": body,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    let review_body = review.body.as_deref().unwrap_or("");
    let review_enrichment = if !review_body.is_empty() || !inline_comments.is_empty() {
        Some(serde_json::json!({
            "pr_title": pr_title,
            "pr_number": pr_number,
            "state": review.state,
            "body": review_body,
            "inline_comments": inline_comments,
        }))
    } else {
        None
    };

    Ok(ContributionInput {
        platform: Platform::Github,
        contribution_type: ContributionType::PrReview,
        platform_id: format!("{owner}/{repo}/review/{review_id}").into(),
        platform_username: reviewer.to_lowercase().into(),
        title: Some(format!("Review on #{pr_number}")),
        url: Some(format!("{pr_url}#pullrequestreview-{review_id}")),
        state: review_state,
        created_at: submitted_at.unwrap_or(parse_datetime(pr_created_at)?),
        updated_at: submitted_at,
        closed_at: None,
        metrics: serde_json::to_value(ContributionMetrics {
            review_state: Some(review.state.clone()),
            ..Default::default()
        })
        .unwrap_or_default(),
        metadata: serde_json::to_value(ContributionMetadata {
            repo: Some(format!("{owner}/{repo}")),
            pr_number: Some(pr_number),
            pr_platform_id: Some(format!("{owner}/{repo}/pull/{pr_number}")),
            ..Default::default()
        })
        .unwrap_or_default(),
        content: review.body.clone(),
        state_history: None,
        enrichment_content: review_enrichment,
    })
}

/// Outcome of a `fetch_pr_diffs()` call.
struct DiffFetchOutcome {
    /// Rate limit info if we hit the REST limit (for durable sleep).
    rate_limit: Option<RateLimitInfo>,
    /// PRs that were skipped due to rate limiting.
    skipped: Vec<ps_core::ingestion::SkippedDiff>,
}

/// Fetch PR diffs via the REST API (`/pulls/{number}/files`) and attach to
/// enrichment content.
///
/// Uses the proper REST rate limit headers (`x-ratelimit-remaining`,
/// `x-ratelimit-reset`) for backoff. Each PR costs 1+ REST API calls
/// (paginated at 100 files). The REST rate limit pool (5,000/hr) is separate
/// from GraphQL, so diff fetches don't compete with PR/review queries.
///
/// When rate-limited, returns immediately with the skipped PRs instead of
/// sleeping. The caller is responsible for durable sleep + retry.
async fn fetch_pr_diffs(
    ctx: &IngestionContext,
    items: &mut [ContributionInput],
) -> DiffFetchOutcome {
    let token = ctx.token.as_deref().unwrap_or("");
    if token.is_empty() {
        return DiffFetchOutcome {
            rate_limit: None,
            skipped: vec![],
        };
    }

    let api_base = ctx
        .source_config
        .settings
        .get("base_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.github.com");

    let client = GitHubClient::new(ctx.http_client.clone(), api_base, token);

    // Collect (index, owner, repo, pr_number) for PR items.
    let pr_targets: Vec<(usize, String, String, u32)> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.contribution_type == ContributionType::PullRequest)
        .filter_map(|(i, item)| {
            // platform_id format: "{owner}/{repo}/pull/{number}"
            let parts: Vec<&str> = item.platform_id.split('/').collect();
            let owner = parts.first()?;
            let repo = parts.get(1)?;
            let number: u32 = parts.get(3)?.parse().ok()?;
            Some((i, (*owner).to_string(), (*repo).to_string(), number))
        })
        .collect();

    if pr_targets.is_empty() {
        return DiffFetchOutcome {
            rate_limit: None,
            skipped: vec![],
        };
    }

    let mut attached = 0u32;

    let mut i = 0;
    #[allow(clippy::indexing_slicing)] // i is always < pr_targets.len() (loop guard)
    while i < pr_targets.len() {
        let (idx, ref owner, ref repo, pr_number) = pr_targets[i];

        match fetch_single_pr_diff(&client, owner, repo, pr_number).await {
            DiffFetchResult::Ok(diff_text) => {
                // Safety: idx comes from enumerate() on items.
                #[allow(clippy::indexing_slicing)]
                if let Some(ref mut enrichment) = items[idx].enrichment_content
                    && let Some(obj) = enrichment.as_object_mut()
                {
                    obj.insert("diff".to_string(), serde_json::Value::String(diff_text));
                    attached += 1;
                }
                i += 1;
            }
            DiffFetchResult::RateLimited(rate_limit) => {
                // Don't sleep — collect remaining PRs as skipped and return.
                let skipped: Vec<ps_core::ingestion::SkippedDiff> = pr_targets[i..]
                    .iter()
                    .map(
                        |(idx, owner, repo, pr_number)| ps_core::ingestion::SkippedDiff {
                            item_index: *idx,
                            owner: owner.clone(),
                            repo: repo.clone(),
                            pr_number: *pr_number,
                        },
                    )
                    .collect();
                warn!(
                    skipped = skipped.len(),
                    reset_at = %rate_limit.reset_at,
                    "REST rate limit hit, deferring remaining diffs for durable retry"
                );
                if attached > 0 {
                    debug!(
                        count = attached,
                        total = pr_targets.len(),
                        "attached PR diffs via REST API (partial)"
                    );
                }
                return DiffFetchOutcome {
                    rate_limit: Some(rate_limit),
                    skipped,
                };
            }
            DiffFetchResult::Failed => {
                i += 1;
            }
        }
    }

    if attached > 0 {
        debug!(
            count = attached,
            total = pr_targets.len(),
            "attached PR diffs via REST API"
        );
    }

    DiffFetchOutcome {
        rate_limit: None,
        skipped: vec![],
    }
}

/// Result of fetching diff content for a single PR.
pub(crate) enum DiffFetchResult {
    /// Combined patch text (truncated to `MAX_DIFF_SIZE`).
    Ok(String),
    /// Hit rate limit — caller should sleep until reset.
    RateLimited(RateLimitInfo),
    /// Non-retryable error (logged internally).
    Failed,
}

/// Fetch file patches for a single PR, paginating as needed, and combine into
/// a single diff string.
pub(crate) async fn fetch_single_pr_diff(
    client: &GitHubClient,
    owner: &str,
    repo: &str,
    pr_number: u32,
) -> DiffFetchResult {
    let mut combined = String::new();
    let mut page = 1u32;

    loop {
        let label = format!("PR {owner}/{repo}#{pr_number} files");
        let page_result = match retry_transient(
            &label,
            super::super::client::GitHubError::is_transient,
            || client.list_pr_files(owner, repo, pr_number, page),
        )
        .await
        {
            Ok(r) => r,
            Err(super::super::client::GitHubError::Api {
                status, rate_limit, ..
            }) if status == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                debug!(
                    owner,
                    repo,
                    pr_number,
                    remaining = rate_limit.remaining,
                    reset_at = %rate_limit.reset_at,
                    "PR files endpoint returned 429"
                );
                return DiffFetchResult::RateLimited(rate_limit);
            }
            Err(e) => {
                debug!(
                    owner,
                    repo,
                    pr_number,
                    error = %e,
                    "failed to fetch PR files"
                );
                return DiffFetchResult::Failed;
            }
        };

        // Check rate limit proactively — pause before we exhaust the budget.
        if page_result.rate_limit.remaining < REST_RATE_LIMIT_FLOOR
            && page_result.rate_limit.remaining > 0
        {
            debug!(
                remaining = page_result.rate_limit.remaining,
                "REST rate limit running low, returning what we have"
            );
            // Don't abort entirely — return whatever we've built so far.
            break;
        }
        if page_result.rate_limit.remaining == 0 {
            // Already exhausted — signal caller to pause.
            if combined.is_empty() {
                return DiffFetchResult::RateLimited(page_result.rate_limit);
            }
            // We have partial content — return it rather than losing it.
            break;
        }

        // Assemble patches from this page.
        for file in &page_result.items {
            if let Some(ref patch) = file.patch {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                // Add file header for context.
                let _ = writeln!(combined, "--- a/{}", file.filename);
                let _ = writeln!(combined, "+++ b/{}", file.filename);
                combined.push_str(patch);

                if combined.len() >= MAX_DIFF_SIZE {
                    // Truncate on a line boundary (floor to char boundary first
                    // to avoid panicking on multi-byte UTF-8 characters).
                    let safe_end = combined.floor_char_boundary(MAX_DIFF_SIZE);
                    let at_line = combined[..safe_end].rfind('\n').unwrap_or(safe_end);
                    combined.truncate(at_line);
                    combined.push_str("\n...(truncated)");
                    return DiffFetchResult::Ok(combined);
                }
            }
        }

        match page_result.next_page {
            Some(next) => page = next,
            None => break,
        }
    }

    if combined.is_empty() {
        DiffFetchResult::Failed
    } else {
        DiffFetchResult::Ok(combined)
    }
}
