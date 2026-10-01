//! Legacy source-wide discovery of authored PRs for active saved accounts.
use std::fmt::Write as _;

use ps_core::ingestion::{ContributionInput, FailedItem, FetchResult, IngestionContext};
use ps_core::models::RateLimitInfo;
use tracing::{debug, warn};

use super::{
    Cursor, IngestionPhase, SEARCH_BATCH_SIZE, build_graphql_client, decrypt_token,
    is_valid_github_username, serialise_cursor,
};
use crate::infra::retry::retry_transient;

/// Transition from `TeamRepos` to `MemberSearch` phase.
pub(super) async fn transition(
    ctx: &IngestionContext,
    cur: &mut Cursor,
) -> Result<FetchResult, ps_core::Error> {
    // Include active saved accounts regardless of team membership.
    let usernames = ctx.repos.org.get_all_github_team_member_usernames().await?;

    if usernames.is_empty() {
        debug!("no active saved GitHub accounts found — skipping member search");
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
    fetch(ctx, cur).await
}

/// Search for cross-repo contributions by active saved accounts using GraphQL search.
///
/// Batches multiple usernames into a single query using OR semantics
/// (`author:u1 author:u2 ...`) to reduce the number of API calls.
pub(super) async fn fetch(
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

    let (mut items, cross_repo_count) = collect_contributions(ctx, cur, &page.items)?;

    // Fetch PR diffs concurrently and attach to enrichment content.
    let diff_outcome = super::diff::fetch_pr_diffs(ctx, &mut items).await;

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

fn collect_contributions(
    ctx: &IngestionContext,
    cur: &mut Cursor,
    prs: &[super::super::types::GraphQLSearchPr],
) -> Result<(Vec<ContributionInput>, u32), ps_core::Error> {
    let settings = &ctx.source_config.settings;
    let exclude_repos: Vec<String> = settings.get("exclude_repos").map_or(Ok(vec![]), |value| {
        serde_json::from_value(value.clone())
            .map_err(|_| ps_core::Error::Validation("invalid GitHub exclude_repos".into()))
    })?;
    let exclude_archived = settings
        .get("exclude_archived")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let mut items = Vec::new();
    let mut cross_repo_count = 0;

    for pr in prs {
        let Some(repo) = &pr.repository else {
            continue;
        };
        if pr.number.is_none() {
            continue;
        }
        let owner = &repo.owner.login;
        let name = &repo.name;
        if cur.ingested_repos.contains(&format!("{owner}/{name}")) {
            continue;
        }
        match super::repositories::eligible(repo, &cur.orgs, &exclude_repos, exclude_archived) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => {
                cur.failed_items.push(FailedItem {
                    key: format!("{owner}/{name}"),
                    error: error.to_string(),
                });
                continue;
            }
        }

        cross_repo_count += 1;
        if let Some(updated_at) = &pr.updated_at
            && cur
                .max_updated_at
                .as_ref()
                .is_none_or(|max| updated_at > max)
        {
            cur.max_updated_at = Some(updated_at.clone());
        }
        items.extend(super::convert::search_pr_to_contributions(owner, name, pr)?);
        super::reviews::queue(cur, owner, name, pr)?;
    }
    Ok((items, cross_repo_count))
}
