//! Bounded review continuations shared by full-source and person ingestion.
use ps_core::ingestion::{ContributionInput, FetchResult, IngestionContext};
use ps_core::models::RateLimitInfo;
use serde::{Deserialize, Serialize};

use super::super::graphql::GraphQLClientError;
use super::super::types::GraphQLSearchPr;
use super::{Cursor, build_graphql_client, decrypt_token, serialise_cursor};
use crate::infra::retry::retry_transient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ReviewTarget {
    owner: String,
    repo: String,
    number: u32,
    title: String,
    url: String,
    created_at: String,
    cursor: String,
}

pub(super) fn queue(
    cur: &mut Cursor,
    owner: &str,
    repo: &str,
    pr: &GraphQLSearchPr,
) -> Result<(), ps_core::Error> {
    let Some(reviews) = &pr.reviews else {
        return Ok(());
    };
    if !reviews.page_info.has_next_page {
        return Ok(());
    }
    let cursor = reviews.page_info.end_cursor.clone().ok_or_else(|| {
        ps_core::Error::Internal("GitHub review page omitted its continuation cursor".into())
    })?;
    cur.pending_reviews.push_back(ReviewTarget {
        owner: owner.into(),
        repo: repo.into(),
        number: pr.number.unwrap_or(0),
        title: pr.title.clone().unwrap_or_default(),
        url: pr.url.clone().unwrap_or_default(),
        created_at: pr
            .created_at
            .clone()
            .unwrap_or_else(|| "1970-01-01T00:00:00Z".into()),
        cursor,
    });
    Ok(())
}

pub(super) fn result(
    cur: &Cursor,
    items: Vec<ContributionInput>,
    rate_limit: Option<RateLimitInfo>,
) -> Result<FetchResult, ps_core::Error> {
    Ok(FetchResult {
        items,
        next_cursor: Some(serialise_cursor(cur)?),
        display_rate_limit: rate_limit.clone(),
        rate_limit,
        etag: None,
        skipped_diffs: vec![],
    })
}

/// API rate limiting must preserve the exact cursor until a durable sleep ends.
pub(super) fn rate_limited(error: &GraphQLClientError) -> Option<RateLimitInfo> {
    let limit = match error {
        GraphQLClientError::Api {
            status,
            rate_limit,
            body,
        } if *status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || (*status == reqwest::StatusCode::FORBIDDEN
                && ((rate_limit.limit > 0 && rate_limit.remaining == 0)
                    || body.to_ascii_lowercase().contains("rate limit"))) =>
        {
            Some(rate_limit.clone())
        }
        GraphQLClientError::GraphQL {
            messages,
            rate_limit,
        } if messages.to_ascii_lowercase().contains("rate limit") => rate_limit.clone(),
        _ => None,
    };
    limit.map(|mut limit| {
        limit.remaining = 0;
        limit
    })
}

/// Provider response bodies may contain credentials or private diagnostics.
/// Durable Person state retains only an actionable category and HTTP status.
pub(super) fn person_error(error: &GraphQLClientError) -> String {
    tracing::warn!(error = ?error, "GitHub person collection request failed");
    match error {
        GraphQLClientError::Api { status, .. } => format!(
            "GitHub request failed (HTTP {}); check source permissions and configuration",
            status.as_u16()
        ),
        GraphQLClientError::GraphQL { .. } => {
            "GitHub query failed; check source permissions and configuration".into()
        }
        GraphQLClientError::Parse { .. } => {
            "GitHub returned an invalid response; collection is incomplete".into()
        }
        GraphQLClientError::Http(_) => "GitHub connection failed; retry collection".into(),
    }
}

pub(super) async fn fetch(
    ctx: &IngestionContext,
    cur: &mut Cursor,
) -> Result<FetchResult, ps_core::Error> {
    let target = cur
        .pending_reviews
        .front()
        .cloned()
        .ok_or_else(|| ps_core::Error::Internal("missing review target".into()))?;
    let token = decrypt_token(ctx)?;
    let client = build_graphql_client(ctx, &token);
    let page = match retry_transient(
        "GitHub review page",
        GraphQLClientError::is_transient,
        || {
            client.fetch_reviews(
                &target.owner,
                &target.repo,
                target.number,
                Some(&target.cursor),
            )
        },
    )
    .await
    {
        Ok(page) => page,
        Err(error) => {
            if let Some(limit) = rate_limited(&error) {
                return result(cur, vec![], Some(limit));
            }
            if error.is_transient() {
                return Err(ps_core::Error::Internal(format!(
                    "GitHub review fetch failed: {error}"
                )));
            }
            cur.failed_items.push(ps_core::ingestion::FailedItem {
                key: format!(
                    "{}/{}/pull/{}/reviews",
                    target.owner, target.repo, target.number
                ),
                error: if ctx.person_request()?.is_some() {
                    person_error(&error)
                } else {
                    error.to_string()
                },
            });
            cur.pending_reviews.pop_front();
            return result(cur, vec![], None);
        }
    };

    let mut items = Vec::new();
    for review in &page.items {
        if !super::convert::is_submitted_review(review) {
            continue;
        }
        let item = super::convert::search_review_to_contribution(
            &target.owner,
            &target.repo,
            target.number,
            &target.url,
            &target.title,
            &target.created_at,
            review,
        )?;
        if super::person::eligible(ctx, &item)? {
            items.push(item);
        }
    }

    if page.has_next_page {
        let cursor = page
            .end_cursor
            .filter(|cursor| cursor != &target.cursor)
            .ok_or_else(|| {
                ps_core::Error::Internal("GitHub reviews did not advance pagination".into())
            })?;
        if let Some(target) = cur.pending_reviews.front_mut() {
            target.cursor = cursor;
        }
    } else {
        cur.pending_reviews.pop_front();
    }
    result(cur, items, Some(page.rate_limit))
}
