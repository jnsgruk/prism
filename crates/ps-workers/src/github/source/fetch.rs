use std::fmt::Write as _;

use ps_core::ingestion::{
    ContributionInput, ContributionMetadata, ContributionMetrics, FetchResult, IngestionContext,
};
use ps_core::models::{ContributionState, ContributionType, Platform};
use tracing::{info, warn};

use super::{
    Cursor, IngestionPhase, RATE_LIMIT_SEARCH_THRESHOLD, SEARCH_BATCH_SIZE, build_graphql_client,
    decrypt_token, is_valid_github_username, parse_datetime, serialise_cursor,
};
use crate::github::types::GraphQLSearchPr;

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
        info!(
            source = ctx.source_config.name,
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
    if let Some(ref wm) = cur.watermark {
        let _ = write!(query, " updated:>{wm}");
    }

    let page = client
        .search_pull_requests(&query, cur.graphql_cursor.as_deref())
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

    // Load all GitHub usernames for active team members — includes users from
    // teams without a GitHub team mapping.
    let usernames = ctx.repos.org.get_all_github_team_member_usernames().await?;

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
///
/// Batches multiple usernames into a single query using OR semantics
/// (`author:u1 author:u2 ...`) to reduce the number of API calls.
async fn fetch_member_search(
    ctx: &IngestionContext,
    cur: &mut Cursor,
) -> Result<FetchResult, ps_core::Error> {
    if cur.search_user_index >= cur.search_users.len() {
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

    info!(
        source = ctx.source_config.name,
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
        rate_limit: Some(page.rate_limit),
        etag: None,
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

    items.push(ContributionInput {
        platform: Platform::Github,
        contribution_type: ContributionType::PullRequest,
        platform_id: format!("{owner}/{repo}/pull/{number}"),
        platform_username: author.to_string(),
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
        enrichment_content: None,
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

            let review_state = ContributionState::from_str_opt(&review.state);

            items.push(ContributionInput {
                platform: Platform::Github,
                contribution_type: ContributionType::PrReview,
                platform_id: format!("{owner}/{repo}/review/{review_id}"),
                platform_username: reviewer.to_string(),
                title: Some(format!("Review on #{number}")),
                url: Some(format!("{url}/reviews/{review_id}")),
                state: review_state,
                created_at: submitted_at.unwrap_or(parse_datetime(created_at_str)?),
                updated_at: submitted_at,
                closed_at: None,
                metrics: serde_json::to_value(ContributionMetrics {
                    review_state: Some(review.state.clone()),
                    ..Default::default()
                })
                .unwrap_or_default(),
                metadata: serde_json::to_value(ContributionMetadata {
                    repo: Some(format!("{owner}/{repo}")),
                    pr_number: Some(number),
                    pr_platform_id: Some(format!("{owner}/{repo}/pull/{number}")),
                    ..Default::default()
                })
                .unwrap_or_default(),
                content: review.body.clone(),
                state_history: None,
                enrichment_content: None,
            });
        }
    }

    Ok(items)
}
