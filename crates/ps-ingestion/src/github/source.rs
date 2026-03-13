use async_trait::async_trait;
use ps_core::ingestion::{
    ContributionInput, FetchResult, IngestionContext, IngestionPlan, RepoTarget, Source,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::client::{GitHubClient, ListPullsParams};
use super::etag;
use super::identity;
use super::repos;

/// GitHub source adapter implementing the [`Source`] trait.
///
/// Fetches pull requests and PR reviews from configured GitHub organisations.
pub struct GitHubSource;

/// Serialised cursor for tracking position within a multi-repo, multi-page fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cursor {
    repo_index: usize,
    page: u32,
    watermark: Option<String>,
    /// Cached list of repos so we don't re-discover mid-run.
    repos: Vec<RepoTarget>,
    /// Track the latest `updated_at` timestamp seen across all items.
    max_updated_at: Option<String>,
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
        let cursor = Cursor {
            repo_index: 0,
            page: 1,
            watermark: plan.watermark.clone(),
            repos: plan.repos.clone(),
            max_updated_at: plan.watermark.clone(),
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

    let exclude_archived = settings
        .get("exclude_archived")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let exclude_repos: Vec<String> = settings
        .get("exclude_repos")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let token = decrypt_token(ctx).await?;
    let client = build_client(ctx, &token);

    let discovered_repos = repos::discover_repos(
        &client,
        &orgs,
        ctx.repos.org.pool(),
        exclude_archived,
        &exclude_repos,
    )
    .await?;

    let watermark = ctx
        .repos
        .activity
        .get_watermark(&ctx.source_config.name)
        .await?;

    info!(
        source = ctx.source_config.name,
        repos = discovered_repos.len(),
        watermark = ?watermark,
        "planned GitHub ingestion"
    );

    Ok(IngestionPlan {
        source_name: ctx.source_config.name.clone(),
        watermark,
        repos: discovered_repos,
    })
}

async fn fetch_batch_impl(
    ctx: &IngestionContext,
    cursor: &str,
) -> Result<FetchResult, ps_core::Error> {
    let mut cur: Cursor = serde_json::from_str(cursor)
        .map_err(|e| ps_core::Error::Internal(format!("invalid cursor: {e}")))?;

    // Check if we've exhausted all repos
    let Some(repo_target) = cur.repos.get(cur.repo_index) else {
        return Ok(FetchResult {
            items: vec![],
            next_cursor: None,
            rate_limit: None,
            etag: None,
        });
    };

    let owner = repo_target.owner.clone();
    let repo = repo_target.repo.clone();

    let token = decrypt_token(ctx).await?;
    let client = build_client(ctx, &token);

    // Check ETag cache for first page
    let endpoint_key =
        etag::normalise_endpoint(&format!("{}/repos/{owner}/{repo}/pulls", client.base_url()));
    let cached_etag = if cur.page == 1 {
        ctx.repos
            .activity
            .get_cached_etag(&ctx.source_config.name, &endpoint_key)
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let result = client
        .list_pulls(&ListPullsParams {
            owner: &owner,
            repo: &repo,
            state: "all",
            page: cur.page,
            per_page: 100,
            since: cur.watermark.as_deref(),
            if_none_match: cached_etag.as_deref(),
        })
        .await
        .map_err(|e| ps_core::Error::Internal(format!("GitHub API error: {e}")))?;

    // If 304 Not Modified, skip this repo entirely
    if result.not_modified {
        debug!(owner, repo, "skipped (304 Not Modified)");
        cur.repo_index += 1;
        cur.page = 1;
        return Ok(FetchResult {
            items: vec![],
            next_cursor: Some(serialise_cursor(&cur)?),
            rate_limit: Some(result.rate_limit),
            etag: None,
        });
    }

    // Update ETag cache if we got a new one on page 1
    if let Some(new_etag) = &result.etag
        && cur.page == 1
        && let Err(e) = ctx
            .repos
            .activity
            .set_cached_etag(&ctx.source_config.name, &endpoint_key, new_etag)
            .await
    {
        warn!("failed to cache ETag: {e}");
    }

    // Convert PRs to ContributionInput and fetch reviews
    let items = map_prs_to_contributions(&client, &owner, &repo, &result.items, &mut cur).await?;

    // Determine next cursor
    let next_cursor = if let Some(next_page) = result.next_page {
        cur.page = next_page;
        Some(serialise_cursor(&cur)?)
    } else if cur.repo_index + 1 < cur.repos.len() {
        cur.repo_index += 1;
        cur.page = 1;
        Some(serialise_cursor(&cur)?)
    } else {
        None
    };

    debug!(
        owner,
        repo,
        page = cur.page,
        items = items.len(),
        "fetched batch"
    );

    Ok(FetchResult {
        items,
        next_cursor,
        rate_limit: Some(result.rate_limit),
        etag: result.etag,
    })
}

async fn map_prs_to_contributions(
    client: &GitHubClient,
    owner: &str,
    repo: &str,
    prs: &[super::types::GitHubPr],
    cur: &mut Cursor,
) -> Result<Vec<ContributionInput>, ps_core::Error> {
    let mut items = Vec::new();

    for pr in prs {
        // Track the highest updated_at for watermark advancement
        match cur.max_updated_at {
            Some(ref max) if pr.updated_at > *max => {
                cur.max_updated_at = Some(pr.updated_at.clone());
            }
            None => {
                cur.max_updated_at = Some(pr.updated_at.clone());
            }
            _ => {}
        }

        let pr_state = if pr.merged_at.is_some() {
            "merged"
        } else {
            &pr.state
        };

        // Fetch reviews for this PR
        let reviews = match client.list_reviews(owner, repo, pr.number).await {
            Ok(r) => r,
            Err(e) => {
                warn!(owner, repo, pr = pr.number, "failed to fetch reviews: {e}");
                vec![]
            }
        };

        let review_count = reviews.len();

        // Build state_history
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

        items.push(ContributionInput {
            platform: "github".into(),
            contribution_type: "pull_request".into(),
            platform_id: format!("{owner}/{repo}/pull/{}", pr.number),
            platform_username: pr.user.login.clone(),
            title: Some(pr.title.clone()),
            url: Some(pr.html_url.clone()),
            state: Some(pr_state.to_string()),
            created_at: parse_datetime(&pr.created_at)?,
            updated_at: Some(parse_datetime(&pr.updated_at)?),
            closed_at: pr
                .closed_at
                .as_deref()
                .map(parse_datetime)
                .transpose()?,
            metrics: serde_json::json!({
                "additions": pr.additions,
                "deletions": pr.deletions,
                "changed_files": pr.changed_files,
                "review_count": review_count,
                "draft": pr.draft.unwrap_or(false),
            }),
            metadata: serde_json::json!({
                "head_ref": pr.head.as_ref().map(|r| &r.ref_name),
                "base_ref": pr.base.as_ref().map(|r| &r.ref_name),
                "labels": pr.labels.as_ref().map(|ls| ls.iter().map(|l| &l.name).collect::<Vec<_>>()),
            }),
            content: None,
            state_history: Some(serde_json::Value::Array(state_history)),
        });

        // Map each review to a separate ContributionInput
        for review in &reviews {
            let submitted_at = review
                .submitted_at
                .as_deref()
                .map(parse_datetime)
                .transpose()?;

            items.push(ContributionInput {
                platform: "github".into(),
                contribution_type: "pr_review".into(),
                platform_id: format!("{owner}/{repo}/review/{}", review.id),
                platform_username: review.user.login.clone(),
                title: Some(format!("Review on #{}", pr.number)),
                url: Some(format!("{}/reviews/{}", pr.html_url, review.id)),
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

    // Collect unique usernames for batch identity resolution
    let usernames: Vec<String> = items
        .iter()
        .map(|i| i.platform_username.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let person_map = identity::batch_resolve_person_ids(ctx.repos.org.pool(), &usernames)
        .await
        .map_err(|e| ps_core::Error::Database(e.to_string()))?;

    let mut stored = 0usize;
    for item in items {
        let person_id = person_map.get(&item.platform_username).copied();
        let id = Uuid::now_v7();

        ctx.repos
            .activity
            .upsert_contribution(id, person_id, item)
            .await?;

        stored += 1;
    }

    debug!(stored, "stored batch");
    Ok(stored)
}

async fn advance_watermark_impl(
    ctx: &IngestionContext,
    new_watermark: &str,
    items_collected: i32,
) -> Result<(), ps_core::Error> {
    ctx.repos
        .activity
        .upsert_watermark(&ctx.source_config.name, new_watermark, items_collected)
        .await?;

    info!(
        source = ctx.source_config.name,
        watermark = new_watermark,
        items_collected,
        "advanced watermark"
    );
    Ok(())
}

/// Build a `GitHubClient` from the ingestion context and a decrypted token.
fn build_client(ctx: &IngestionContext, token: &str) -> GitHubClient {
    let base_url = ctx
        .source_config
        .settings
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("https://api.github.com");
    GitHubClient::new(ctx.http_client.clone(), base_url, token)
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
