use super::super::client::GitHubClient;
use crate::infra::retry::retry_transient;
use ps_core::ingestion::{ContributionInput, IngestionContext};
use ps_core::models::{ContributionType, RateLimitInfo};
use std::fmt::Write as _;
use tracing::{debug, warn};
const MAX_DIFF_SIZE: usize = 20_000;
const REST_RATE_LIMIT_FLOOR: i32 = 50;

/// Outcome of a `fetch_pr_diffs()` call.
pub(super) struct DiffFetchOutcome {
    /// Rate limit info if we hit the REST limit (for durable sleep).
    pub(super) rate_limit: Option<RateLimitInfo>,
    /// PRs that were skipped due to rate limiting.
    pub(super) skipped: Vec<ps_core::ingestion::SkippedDiff>,
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
pub(super) async fn fetch_pr_diffs(
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
