use std::fmt::Write as _;

use ps_core::models::RateLimitInfo;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, LINK, USER_AGENT};
use time::OffsetDateTime;
use tracing::{debug, warn};

use super::types::{GitHubPr, GitHubRepo, GitHubReview};

/// Result of a paginated API call: items, optional next page, rate limit info.
pub struct PageResult<T> {
    pub items: Vec<T>,
    pub next_page: Option<u32>,
    pub rate_limit: RateLimitInfo,
    pub etag: Option<String>,
    /// True if the server returned 304 Not Modified.
    pub not_modified: bool,
}

/// Parameters for listing pull requests.
pub struct ListPullsParams<'a> {
    pub owner: &'a str,
    pub repo: &'a str,
    pub state: &'a str,
    pub page: u32,
    pub per_page: u32,
    pub since: Option<&'a str>,
    pub if_none_match: Option<&'a str>,
}

/// Low-level GitHub REST API client.
pub struct GitHubClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl GitHubClient {
    pub fn new(http: reqwest::Client, base_url: &str, token: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    /// List pull requests for a repository, sorted by `updated` ascending.
    ///
    /// Supports the `since` parameter (ISO 8601 datetime) for incremental fetching
    /// and `ETag` conditional requests via `if_none_match`.
    pub async fn list_pulls(
        &self,
        params: &ListPullsParams<'_>,
    ) -> Result<PageResult<GitHubPr>, GitHubError> {
        let mut url = format!(
            "{}/repos/{}/{}/pulls?state={}&sort=updated&direction=asc&per_page={}&page={}",
            self.base_url, params.owner, params.repo, params.state, params.per_page, params.page,
        );
        if let Some(since) = params.since {
            let _ = write!(url, "&since={since}");
        }

        let mut req = self.http.get(&url).headers(self.default_headers());

        if let Some(etag) = params.if_none_match {
            req = req.header("If-None-Match", etag);
        }

        let resp = req.send().await.map_err(GitHubError::Http)?;
        let rate_limit = parse_rate_limit(resp.headers());

        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(PageResult {
                items: vec![],
                next_page: None,
                rate_limit,
                etag: None,
                not_modified: true,
            });
        }

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api { status, body });
        }

        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let next_page = resp
            .headers()
            .get(LINK)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_next_page);
        let items: Vec<GitHubPr> = resp.json().await.map_err(GitHubError::Http)?;

        debug!(
            owner = params.owner,
            repo = params.repo,
            page = params.page,
            count = items.len(),
            ?next_page,
            "fetched pull requests"
        );

        Ok(PageResult {
            items,
            next_page,
            rate_limit,
            etag,
            not_modified: false,
        })
    }

    /// List reviews for a specific pull request.
    pub async fn list_reviews(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u32,
    ) -> Result<Vec<GitHubReview>, GitHubError> {
        let url = format!(
            "{}/repos/{owner}/{repo}/pulls/{pr_number}/reviews?per_page=100",
            self.base_url,
        );

        let resp = self
            .http
            .get(&url)
            .headers(self.default_headers())
            .send()
            .await
            .map_err(GitHubError::Http)?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api { status, body });
        }

        let reviews: Vec<GitHubReview> = resp.json().await.map_err(GitHubError::Http)?;
        Ok(reviews)
    }

    /// List repositories for a GitHub organisation.
    pub async fn list_org_repos(
        &self,
        org: &str,
        page: u32,
        per_page: u32,
    ) -> Result<PageResult<GitHubRepo>, GitHubError> {
        let url = format!(
            "{}/orgs/{org}/repos?type=all&sort=updated&per_page={per_page}&page={page}",
            self.base_url,
        );

        let resp = self
            .http
            .get(&url)
            .headers(self.default_headers())
            .send()
            .await
            .map_err(GitHubError::Http)?;

        let rate_limit = parse_rate_limit(resp.headers());

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api { status, body });
        }

        let next_page = resp
            .headers()
            .get(LINK)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_next_page);
        let items: Vec<GitHubRepo> = resp.json().await.map_err(GitHubError::Http)?;

        debug!(
            org,
            page,
            count = items.len(),
            ?next_page,
            "fetched org repos"
        );

        Ok(PageResult {
            items,
            next_page,
            rate_limit,
            etag: None,
            not_modified: false,
        })
    }

    fn default_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", self.token)) {
            headers.insert(AUTHORIZATION, val);
        }
        headers.insert(USER_AGENT, HeaderValue::from_static("prism-ingestion/0.1"));
        headers.insert(
            "Accept",
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );
        headers
    }
}

/// Parse the `Link` header to extract the next page number.
///
/// GitHub's Link header looks like:
/// `<https://api.github.com/...?page=2>; rel="next", <...>; rel="last"`
fn parse_next_page(link: &str) -> Option<u32> {
    for part in link.split(',') {
        if part.contains("rel=\"next\"") {
            // Extract URL between < and >
            let url_start = part.find('<')? + 1;
            let url_end = part.find('>')?;
            let url = &part[url_start..url_end];
            // Extract page param
            for param in url.split('&') {
                if let Some(val) = param.strip_prefix("page=") {
                    return val.parse().ok();
                }
                // Also handle ?page= at the start of query string
                if let Some(rest) = param.split('?').next_back()
                    && let Some(val) = rest.strip_prefix("page=")
                {
                    return val.parse().ok();
                }
            }
        }
    }
    None
}

/// Parse GitHub rate limit headers into a `RateLimitInfo`.
fn parse_rate_limit(headers: &HeaderMap) -> RateLimitInfo {
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

/// Errors from the GitHub API client.
#[derive(Debug, thiserror::Error)]
pub enum GitHubError {
    #[error("HTTP error: {0}")]
    Http(reqwest::Error),
    #[error("GitHub API error (status {status}): {body}")]
    Api {
        status: reqwest::StatusCode,
        body: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_next_page() {
        let link = r#"<https://api.github.com/repos/org/repo/pulls?page=2>; rel="next", <https://api.github.com/repos/org/repo/pulls?page=5>; rel="last""#;
        assert_eq!(parse_next_page(link), Some(2));
    }

    #[test]
    fn test_parse_next_page_no_next() {
        let link = r#"<https://api.github.com/repos/org/repo/pulls?page=1>; rel="first""#;
        assert_eq!(parse_next_page(link), None);
    }

    #[test]
    fn test_parse_next_page_with_other_params() {
        let link = r#"<https://api.github.com/repos/org/repo/pulls?state=all&per_page=100&page=3>; rel="next""#;
        assert_eq!(parse_next_page(link), Some(3));
    }
}
