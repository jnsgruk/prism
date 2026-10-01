use super::convert::search_pr_to_contributions;
use super::diff::fetch_pr_diffs;
pub(crate) use super::diff::{DiffFetchResult, fetch_single_pr_diff};
use std::fmt::Write as _;

use ps_core::ingestion::{FetchResult, IngestionContext};
use ps_core::models::{ContributionType, RateLimitInfo};
use tracing::{debug, warn};

use super::{Cursor, IngestionPhase, build_graphql_client, decrypt_token, serialise_cursor};
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
            IngestionPhase::MemberSearch => super::members::fetch(ctx, &mut cur).await?,
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
        return super::members::transition(ctx, cur).await;
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
