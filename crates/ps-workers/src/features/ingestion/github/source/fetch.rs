use super::convert::search_pr_to_contributions;
use super::diff::fetch_pr_diffs;
pub(crate) use super::diff::{DiffFetchResult, fetch_single_pr_diff};
use std::fmt::Write as _;

use ps_core::ingestion::{FetchResult, IngestionContext};
use ps_core::models::{ContributionType, RateLimitInfo};
use tracing::{debug, warn};

use super::{
    Cursor, IngestionPhase, SEARCH_BATCH_SIZE, build_graphql_client, decrypt_token,
    is_valid_github_username, serialise_cursor,
};
use crate::infra::retry::retry_transient;
use ps_core::ingestion::FailedItem;

pub(super) async fn fetch_batch_impl(
    ctx: &IngestionContext,
    cursor: &str,
) -> Result<FetchResult, ps_core::Error> {
    let mut cur: Cursor = serde_json::from_str(cursor)
        .map_err(|e| ps_core::Error::Internal(format!("invalid cursor: {e}")))?;

    if ctx.person_request()?.is_some()
        && (!matches!(cur.phase, IngestionPhase::PersonSearch) || cur.person.is_none())
    {
        return Err(ps_core::Error::Validation(
            "person run requires a scoped GitHub cursor".into(),
        ));
    }

    if let Some(person) = &cur.person {
        super::person::validate_cursor(ctx, person)?;
    }

    let mut result = if cur.pending_reviews.is_empty() {
        match cur.phase {
            IngestionPhase::TeamRepos => fetch_team_repos(ctx, &mut cur).await?,
            IngestionPhase::MemberSearch => fetch_member_search(ctx, &mut cur).await?,
            IngestionPhase::PersonSearch => super::person::fetch(ctx, &mut cur).await?,
        }
    } else {
        super::reviews::fetch(ctx, &mut cur).await?
    };

    if ctx.advances_global_watermark()
        && result.next_cursor.is_none()
        && cur.pending_reviews.is_empty()
        && cur.failed_items.is_empty()
    {
        // Another repository can contain older PRs, so even an exhausted
        // current review queue cannot make the source-wide maximum safe yet.
        // Return the completed cursor for coordinator finalization, after all
        // contribution stores have committed successfully.
        cur.completed_max_updated_at = cur.max_updated_at.clone();
        result.etag = Some(serialise_cursor(&cur)?);
    }

    if ctx.person_request()?.is_some() && result.next_cursor.is_none() {
        let mut final_cursor = serde_json::to_value(&cur)
            .map_err(|error| ps_core::Error::Internal(error.to_string()))?;
        if let Some(object) = final_cursor.as_object_mut() {
            object.insert("discovery_complete".into(), true.into());
        }
        result.etag = Some(final_cursor.to_string());
    }

    Ok(result)
}

/// Fetch PRs + reviews for team repos using GraphQL search.
///
/// Uses the `search` query with `repo:{owner}/{repo} type:pr updated:>{watermark}`
/// so GitHub filters server-side rather than us paginating through all history.
async fn fetch_team_repos(
    ctx: &IngestionContext,
    cur: &mut Cursor,
) -> Result<FetchResult, ps_core::Error> {
    let Some(repo_target) = cur.repos.get(cur.repo_index).cloned() else {
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
            if let Some(limit) = super::reviews::rate_limited(&e) {
                return super::reviews::result(cur, vec![], Some(limit));
            }
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
        super::reviews::queue(cur, owner, repo, search_pr)?;
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
            if let Some(limit) = super::reviews::rate_limited(&e) {
                return super::reviews::result(cur, vec![], Some(limit));
            }
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
        super::reviews::queue(cur, owner, repo, search_pr)?;
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
