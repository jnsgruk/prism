pub mod client;
pub mod etag;
pub mod graphql;
pub mod handler;
pub mod repos;
pub mod source;
pub mod team_sync;
pub mod types;

pub use graphql::GitHubGraphQLClient;
pub use source::GitHubSource;

// REST client re-exported for team sync handler and other consumers.
pub use client::GitHubClient;

use ps_core::models::RateLimitInfo;
use reqwest::header::HeaderMap;
use time::OffsetDateTime;
use tracing::warn;

/// Parse GitHub rate limit info from HTTP response headers.
///
/// Used by both the REST and GraphQL clients.
pub(crate) fn parse_rate_limit_headers(headers: &HeaderMap) -> RateLimitInfo {
    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let limit = headers
        .get("x-ratelimit-limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let reset_epoch: i64 = headers
        .get("x-ratelimit-reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let reset_at = OffsetDateTime::from_unix_timestamp(reset_epoch).unwrap_or_else(|e| {
        warn!("invalid rate limit reset timestamp {reset_epoch}: {e}");
        OffsetDateTime::now_utc()
    });

    RateLimitInfo {
        remaining,
        limit,
        reset_at,
    }
}
