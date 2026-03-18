use std::fmt::Write as _;

use ps_core::models::RateLimitInfo;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, LINK, USER_AGENT};
use tracing::debug;

use super::types::{
    GitHubPr, GitHubPrFile, GitHubRepo, GitHubReview, GitHubTeam, GitHubTeamRepo, GitHubUser,
};

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

/// Validate that a URL path segment contains only safe characters.
///
/// GitHub org names, repo names, and team slugs consist of alphanumerics,
/// hyphens, underscores, and dots. Reject anything else to prevent path
/// traversal or injection.
fn validate_path_segment(segment: &str, label: &str) -> Result<(), GitHubError> {
    if segment.is_empty()
        || !segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(GitHubError::InvalidPathSegment {
            label: label.to_string(),
            value: segment.to_string(),
        });
    }
    Ok(())
}

/// Low-level GitHub REST API client.
pub struct GitHubClient {
    http: reqwest::Client,
    base_url: String,
    headers: HeaderMap,
}

impl GitHubClient {
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn new(http: reqwest::Client, base_url: &str, token: &str) -> Self {
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {token}")) {
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
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            headers,
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
        validate_path_segment(params.owner, "owner")?;
        validate_path_segment(params.repo, "repo")?;

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
            return Err(GitHubError::Api {
                status,
                body,
                rate_limit,
            });
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
        validate_path_segment(owner, "owner")?;
        validate_path_segment(repo, "repo")?;

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

        let rate_limit = parse_rate_limit(resp.headers());
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api {
                status,
                body,
                rate_limit,
            });
        }

        let reviews: Vec<GitHubReview> = resp.json().await.map_err(GitHubError::Http)?;
        Ok(reviews)
    }

    /// List files changed in a pull request.
    ///
    /// Returns up to 100 files per page. Use `next_page` from the result to
    /// paginate. The `patch` field on each file contains the unified diff (absent
    /// for binary files or very large diffs).
    pub async fn list_pr_files(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u32,
        page: u32,
    ) -> Result<PageResult<GitHubPrFile>, GitHubError> {
        validate_path_segment(owner, "owner")?;
        validate_path_segment(repo, "repo")?;

        let url = format!(
            "{}/repos/{owner}/{repo}/pulls/{pr_number}/files?per_page=100&page={page}",
            self.base_url,
        );
        let result: PageResult<GitHubPrFile> = self.paginated_get(&url).await?;
        debug!(
            owner,
            repo,
            pr_number,
            page,
            count = result.items.len(),
            next_page = ?result.next_page,
            "fetched PR files"
        );
        Ok(result)
    }

    /// Generic paginated GET: send request, parse rate limit, extract Link
    /// next-page, deserialize JSON body into `Vec<T>`.
    async fn paginated_get<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<PageResult<T>, GitHubError> {
        let resp = self
            .http
            .get(url)
            .headers(self.default_headers())
            .send()
            .await
            .map_err(GitHubError::Http)?;

        let rate_limit = parse_rate_limit(resp.headers());

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api {
                status,
                body,
                rate_limit,
            });
        }

        let next_page = resp
            .headers()
            .get(LINK)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_next_page);
        let items: Vec<T> = resp.json().await.map_err(GitHubError::Http)?;

        Ok(PageResult {
            items,
            next_page,
            rate_limit,
            etag: None,
            not_modified: false,
        })
    }

    /// List repositories for a GitHub organisation.
    pub async fn list_org_repos(
        &self,
        org: &str,
        page: u32,
        per_page: u32,
    ) -> Result<PageResult<GitHubRepo>, GitHubError> {
        validate_path_segment(org, "org")?;
        let url = format!(
            "{}/orgs/{org}/repos?type=all&sort=updated&per_page={per_page}&page={page}",
            self.base_url,
        );
        let result = self.paginated_get(&url).await?;
        debug!(org, page, count = result.items.len(), next_page = ?result.next_page, "fetched org repos");
        Ok(result)
    }

    /// List teams for a GitHub organisation.
    pub async fn list_org_teams(
        &self,
        org: &str,
        page: u32,
    ) -> Result<PageResult<GitHubTeam>, GitHubError> {
        validate_path_segment(org, "org")?;
        let url = format!(
            "{}/orgs/{org}/teams?per_page=100&page={page}",
            self.base_url,
        );
        let result = self.paginated_get(&url).await?;
        debug!(org, page, count = result.items.len(), next_page = ?result.next_page, "fetched org teams");
        Ok(result)
    }

    /// List members of a GitHub team.
    pub async fn list_team_members(
        &self,
        org: &str,
        team_slug: &str,
        page: u32,
    ) -> Result<PageResult<GitHubUser>, GitHubError> {
        validate_path_segment(org, "org")?;
        validate_path_segment(team_slug, "team_slug")?;
        let url = format!(
            "{}/orgs/{org}/teams/{team_slug}/members?per_page=100&page={page}",
            self.base_url,
        );
        let result = self.paginated_get(&url).await?;
        debug!(org, team_slug, page, count = result.items.len(), next_page = ?result.next_page, "fetched team members");
        Ok(result)
    }

    /// List repositories accessible to a GitHub team.
    pub async fn list_team_repos(
        &self,
        org: &str,
        team_slug: &str,
        page: u32,
    ) -> Result<PageResult<GitHubTeamRepo>, GitHubError> {
        validate_path_segment(org, "org")?;
        validate_path_segment(team_slug, "team_slug")?;
        let url = format!(
            "{}/orgs/{org}/teams/{team_slug}/repos?per_page=100&page={page}",
            self.base_url,
        );
        let result = self.paginated_get(&url).await?;
        debug!(org, team_slug, page, count = result.items.len(), next_page = ?result.next_page, "fetched team repos");
        Ok(result)
    }

    /// Header clone cost is negligible given GitHub rate limits (~5000 req/hour).
    fn default_headers(&self) -> HeaderMap {
        self.headers.clone()
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

use super::parse_rate_limit_headers;

fn parse_rate_limit(headers: &HeaderMap) -> RateLimitInfo {
    parse_rate_limit_headers(headers)
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
        rate_limit: RateLimitInfo,
    },
    #[error("invalid URL path segment for {label}: {value:?}")]
    InvalidPathSegment { label: String, value: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    #[tokio::test]
    async fn test_list_pulls_basic() {
        let mock_server = MockServer::start().await;

        let body = serde_json::json!([{
            "number": 42,
            "title": "Add feature",
            "state": "open",
            "user": { "login": "alice", "id": 1 },
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-02T00:00:00Z",
            "html_url": "https://github.com/org/repo/pull/42",
            "additions": 10,
            "deletions": 5,
            "changed_files": 3
        }]);

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&body)
                    .append_header("x-ratelimit-remaining", "99")
                    .append_header("x-ratelimit-limit", "100")
                    .append_header("x-ratelimit-reset", "1700000000"),
            )
            .mount(&mock_server)
            .await;

        let client = GitHubClient::new(reqwest::Client::new(), &mock_server.uri(), "test-token");

        let result = client
            .list_pulls(&ListPullsParams {
                owner: "org",
                repo: "repo",
                state: "all",
                page: 1,
                per_page: 100,
                since: None,
                if_none_match: None,
            })
            .await
            .expect("list_pulls");

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].number, 42);
        assert_eq!(result.items[0].title, "Add feature");
        assert_eq!(result.rate_limit.remaining, 99);
        assert!(!result.not_modified);
    }

    #[tokio::test]
    async fn test_list_pulls_304_not_modified() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls"))
            .and(header("if-none-match", "\"abc123\""))
            .respond_with(
                ResponseTemplate::new(304)
                    .append_header("x-ratelimit-remaining", "50")
                    .append_header("x-ratelimit-limit", "100")
                    .append_header("x-ratelimit-reset", "1700000000"),
            )
            .mount(&mock_server)
            .await;

        let client = GitHubClient::new(reqwest::Client::new(), &mock_server.uri(), "test-token");

        let result = client
            .list_pulls(&ListPullsParams {
                owner: "org",
                repo: "repo",
                state: "all",
                page: 1,
                per_page: 100,
                since: None,
                if_none_match: Some("\"abc123\""),
            })
            .await
            .expect("list_pulls");

        assert!(result.not_modified);
        assert!(result.items.is_empty());
    }

    #[tokio::test]
    async fn test_list_pulls_pagination() {
        let mock_server = MockServer::start().await;

        let body = serde_json::json!([{
            "number": 1,
            "title": "PR 1",
            "state": "open",
            "user": { "login": "bob", "id": 2 },
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-02T00:00:00Z",
            "html_url": "https://github.com/org/repo/pull/1",
            "additions": 1,
            "deletions": 0,
            "changed_files": 1
        }]);

        let link_header = format!(
            "<{}/repos/org/repo/pulls?page=2>; rel=\"next\"",
            mock_server.uri()
        );

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&body)
                    .append_header("link", &link_header)
                    .append_header("x-ratelimit-remaining", "98")
                    .append_header("x-ratelimit-limit", "100")
                    .append_header("x-ratelimit-reset", "1700000000"),
            )
            .mount(&mock_server)
            .await;

        let client = GitHubClient::new(reqwest::Client::new(), &mock_server.uri(), "test-token");

        let result = client
            .list_pulls(&ListPullsParams {
                owner: "org",
                repo: "repo",
                state: "all",
                page: 1,
                per_page: 100,
                since: None,
                if_none_match: None,
            })
            .await
            .expect("list_pulls");

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.next_page, Some(2));
    }

    #[tokio::test]
    async fn test_list_reviews() {
        let mock_server = MockServer::start().await;

        let body = serde_json::json!([
            {
                "id": 100,
                "user": { "login": "reviewer", "id": 3 },
                "state": "APPROVED",
                "submitted_at": "2024-01-05T00:00:00Z",
                "body": "LGTM"
            },
            {
                "id": 101,
                "user": { "login": "reviewer2", "id": 4 },
                "state": "CHANGES_REQUESTED",
                "submitted_at": "2024-01-04T00:00:00Z",
                "body": "Needs changes"
            }
        ]);

        Mock::given(method("GET"))
            .and(path("/repos/org/repo/pulls/42/reviews"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&mock_server)
            .await;

        let client = GitHubClient::new(reqwest::Client::new(), &mock_server.uri(), "test-token");

        let reviews = client
            .list_reviews("org", "repo", 42)
            .await
            .expect("list_reviews");

        assert_eq!(reviews.len(), 2);
        assert_eq!(reviews[0].state, "APPROVED");
        assert_eq!(reviews[1].state, "CHANGES_REQUESTED");
    }

    #[tokio::test]
    async fn test_list_org_repos() {
        let mock_server = MockServer::start().await;

        let body = serde_json::json!([
            {
                "id": 1000,
                "name": "repo-a",
                "full_name": "org/repo-a",
                "owner": { "login": "org" },
                "html_url": "https://github.com/org/repo-a",
                "description": "A repo",
                "archived": false,
                "fork": false,
                "default_branch": "main"
            }
        ]);

        Mock::given(method("GET"))
            .and(path("/orgs/org/repos"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&body)
                    .append_header("x-ratelimit-remaining", "95")
                    .append_header("x-ratelimit-limit", "100")
                    .append_header("x-ratelimit-reset", "1700000000"),
            )
            .mount(&mock_server)
            .await;

        let client = GitHubClient::new(reqwest::Client::new(), &mock_server.uri(), "test-token");

        let result = client
            .list_org_repos("org", 1, 100)
            .await
            .expect("list_org_repos");

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].name, "repo-a");
        assert_eq!(result.items[0].archived, Some(false));
    }
}
