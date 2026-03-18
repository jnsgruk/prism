use ps_core::models::RateLimitInfo;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use zeroize::Zeroizing;

use super::types::{GraphQLSearchData, GraphQLSearchPr};

/// GitHub GraphQL API client.
///
/// Replaces the REST-based ingestion client. A single GraphQL query fetches
/// a page of PRs with inline reviews, eliminating the N+1 calls that the
/// REST approach required (1 list-pulls + N list-reviews per page).
pub struct GitHubGraphQLClient {
    http: reqwest::Client,
    endpoint: String,
    token: Zeroizing<String>,
}

/// A page of results from a GraphQL query.
#[derive(Debug, Clone)]
pub struct GraphQLPage<T> {
    pub items: Vec<T>,
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
    pub rate_limit: RateLimitInfo,
}

/// Raw GraphQL response envelope.
#[derive(Debug, Deserialize)]
struct GraphQLResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQLError>>,
    #[serde(default)]
    extensions: Option<GraphQLExtensions>,
}

/// A GraphQL error.
#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: String,
}

/// Response extensions containing rate limit info.
#[derive(Debug, Deserialize)]
struct GraphQLExtensions {
    #[serde(rename = "rateLimit")]
    rate_limit: Option<GraphQLRateLimit>,
}

#[derive(Debug, Deserialize)]
struct GraphQLRateLimit {
    remaining: Option<i32>,
    limit: Option<i32>,
    #[serde(rename = "resetAt")]
    reset_at: Option<String>,
}

/// Request body sent to the GraphQL endpoint.
#[derive(Serialize)]
struct GraphQLRequest<'a> {
    query: &'a str,
    variables: serde_json::Value,
}

/// Errors from the GitHub GraphQL client.
#[derive(Debug, thiserror::Error)]
pub enum GraphQLClientError {
    #[error("HTTP error: {0}")]
    Http(reqwest::Error),
    #[error("GitHub API error (status {status}): {body}")]
    Api {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("GraphQL errors: {messages}")]
    GraphQL { messages: String },
    #[error("response parse error: {message}")]
    Parse { message: String, body: String },
}

impl GitHubGraphQLClient {
    pub fn new(http: reqwest::Client, base_url: &str, token: &str) -> Self {
        // Derive GraphQL endpoint from REST base URL.
        // https://api.github.com → https://api.github.com/graphql
        // https://github.example.com/api/v3 → https://github.example.com/api/graphql
        let base = base_url.trim_end_matches('/');
        let endpoint = if base.ends_with("/api/v3") {
            format!("{}/graphql", base.strip_suffix("/v3").unwrap_or(base))
        } else {
            format!("{base}/graphql")
        };

        Self {
            http,
            endpoint,
            token: Zeroizing::new(token.to_string()),
        }
    }

    /// Fetch a page of pull requests with inline reviews for a repository.
    ///
    /// Returns up to 100 PRs sorted by `UPDATED_AT ASC`, with up to 100
    /// reviews per PR included inline. Currently used only in tests —
    /// production ingestion uses `search_pull_requests` for server-side
    /// `updated:>` filtering.
    #[cfg(test)]
    pub async fn fetch_pull_requests(
        &self,
        owner: &str,
        repo: &str,
        cursor: Option<&str>,
    ) -> Result<GraphQLPage<super::types::GraphQLPr>, GraphQLClientError> {
        use super::types::GraphQLRepoData;

        let variables = serde_json::json!({
            "owner": owner,
            "repo": repo,
            "cursor": cursor,
        });

        let (data, rate_limit): (GraphQLRepoData, _) =
            self.execute(FETCH_PRS_QUERY, variables).await?;

        let connection = data.repository.pull_requests;

        Ok(GraphQLPage {
            items: connection.nodes,
            has_next_page: connection.page_info.has_next_page,
            end_cursor: connection.page_info.end_cursor,
            rate_limit,
        })
    }

    /// Search for pull requests by team members across an org.
    ///
    /// The `query` parameter is a GitHub search query string, e.g.:
    /// `"author:user1 type:pr org:myorg updated:>2024-01-01"`
    pub async fn search_pull_requests(
        &self,
        query: &str,
        cursor: Option<&str>,
    ) -> Result<GraphQLPage<GraphQLSearchPr>, GraphQLClientError> {
        let variables = serde_json::json!({
            "query": query,
            "cursor": cursor,
        });

        let (data, rate_limit): (GraphQLSearchData, _) =
            self.execute(SEARCH_PRS_QUERY, variables).await?;

        let search = data.search;

        Ok(GraphQLPage {
            items: search.nodes,
            has_next_page: search.page_info.has_next_page,
            end_cursor: search.page_info.end_cursor,
            rate_limit,
        })
    }

    /// Execute a GraphQL query, returning the deserialized data and rate limit.
    async fn execute<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<(T, RateLimitInfo), GraphQLClientError> {
        let body = GraphQLRequest { query, variables };

        let resp = self
            .http
            .post(&self.endpoint)
            .headers(self.default_headers())
            .json(&body)
            .send()
            .await
            .map_err(GraphQLClientError::Http)?;

        let header_rate_limit = parse_rate_limit_headers(resp.headers());
        let status = resp.status();
        let response_text = resp.text().await.map_err(GraphQLClientError::Http)?;

        if !status.is_success() {
            return Err(GraphQLClientError::Api {
                status,
                body: response_text,
            });
        }

        let response: GraphQLResponse<T> =
            serde_json::from_str(&response_text).map_err(|e| GraphQLClientError::Parse {
                message: e.to_string(),
                body: response_text.clone(),
            })?;

        // Prefer rate limit from extensions (includes cost), fall back to headers.
        let rate_limit = response
            .extensions
            .as_ref()
            .and_then(|ext| ext.rate_limit.as_ref())
            .map(|rl| {
                let reset_at = rl
                    .reset_at
                    .as_deref()
                    .and_then(|s| {
                        OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
                            .ok()
                    })
                    .unwrap_or(header_rate_limit.reset_at);

                RateLimitInfo {
                    remaining: rl.remaining.unwrap_or(header_rate_limit.remaining),
                    limit: rl.limit.unwrap_or(header_rate_limit.limit),
                    reset_at,
                }
            })
            .unwrap_or(header_rate_limit);

        // Check for GraphQL-level errors.
        if let Some(errors) = response.errors
            && !errors.is_empty()
        {
            let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
            return Err(GraphQLClientError::GraphQL {
                messages: messages.join("; "),
            });
        }

        let data = response.data.ok_or(GraphQLClientError::GraphQL {
            messages: "response contained no data".into(),
        })?;

        Ok((data, rate_limit))
    }

    fn default_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", &*self.token)) {
            headers.insert(AUTHORIZATION, val);
        }
        headers.insert(USER_AGENT, HeaderValue::from_static("prism-ingestion/0.1"));
        headers
    }
}

use super::parse_rate_limit_headers;

// ---------------------------------------------------------------------------
// GraphQL query strings
// ---------------------------------------------------------------------------

#[cfg(test)]
const FETCH_PRS_QUERY: &str = r"
query($owner: String!, $repo: String!, $cursor: String) {
  repository(owner: $owner, name: $repo) {
    pullRequests(
      first: 100
      after: $cursor
      orderBy: { field: UPDATED_AT, direction: ASC }
    ) {
      pageInfo { hasNextPage endCursor }
      nodes {
        number
        title
        state
        url
        isDraft
        createdAt
        updatedAt
        closedAt
        mergedAt
        additions
        deletions
        changedFiles
        author { login }
        bodyText
        labels(first: 10) { nodes { name } }
        headRefName
        baseRefName
        reviews(first: 10) {
          pageInfo { hasNextPage }
          nodes {
            databaseId
            state
            body
            submittedAt
            author { login }
            comments(first: 20) {
              nodes { body path }
            }
          }
        }
      }
    }
  }
}
";

const SEARCH_PRS_QUERY: &str = r"
query($query: String!, $cursor: String) {
  search(query: $query, type: ISSUE, first: 50, after: $cursor) {
    pageInfo { hasNextPage endCursor }
    issueCount
    nodes {
      ... on PullRequest {
        number
        title
        state
        url
        isDraft
        createdAt
        updatedAt
        closedAt
        mergedAt
        additions
        deletions
        changedFiles
        author { login }
        bodyText
        repository {
          name
          owner { login }
        }
        labels(first: 10) { nodes { name } }
        headRefName
        baseRefName
        reviews(first: 10) {
          pageInfo { hasNextPage }
          nodes {
            databaseId
            state
            body
            submittedAt
            author { login }
            comments(first: 20) {
              nodes { body path }
            }
          }
        }
      }
    }
  }
}
";

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn graphql_pr_response() -> serde_json::Value {
        serde_json::json!({
            "data": {
                "repository": {
                    "pullRequests": {
                        "pageInfo": { "hasNextPage": false, "endCursor": "cursor123" },
                        "nodes": [{
                            "number": 42,
                            "title": "Add feature",
                            "state": "OPEN",
                            "url": "https://github.com/org/repo/pull/42",
                            "isDraft": false,
                            "createdAt": "2024-01-01T00:00:00Z",
                            "updatedAt": "2024-01-02T00:00:00Z",
                            "closedAt": null,
                            "mergedAt": null,
                            "additions": 10,
                            "deletions": 5,
                            "changedFiles": 3,
                            "author": { "login": "alice" },
                            "bodyText": "This PR adds a feature",
                            "labels": { "nodes": [{ "name": "bug" }] },
                            "headRefName": "feature-branch",
                            "baseRefName": "main",
                            "reviews": {
                                "pageInfo": { "hasNextPage": false },
                                "nodes": [{
                                    "databaseId": 100,
                                    "state": "APPROVED",
                                    "body": "LGTM",
                                    "submittedAt": "2024-01-05T00:00:00Z",
                                    "author": { "login": "bob" },
                                    "comments": {
                                        "nodes": [{ "body": "Nice work", "path": "src/main.rs" }]
                                    }
                                }]
                            }
                        }]
                    }
                }
            },
            "extensions": {
                "rateLimit": {
                    "cost": 1,
                    "remaining": 4999,
                    "limit": 5000,
                    "resetAt": "2024-01-01T01:00:00Z"
                }
            }
        })
    }

    #[tokio::test]
    async fn test_fetch_pull_requests() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(graphql_pr_response()))
            .mount(&mock_server)
            .await;

        let client =
            GitHubGraphQLClient::new(reqwest::Client::new(), &mock_server.uri(), "test-token");

        let page = client
            .fetch_pull_requests("org", "repo", None)
            .await
            .expect("fetch_pull_requests");

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].number, 42);
        assert_eq!(page.items[0].title, "Add feature");
        assert_eq!(page.items[0].author.as_ref().unwrap().login, "alice");
        assert_eq!(page.items[0].reviews.nodes.len(), 1);
        assert_eq!(page.items[0].reviews.nodes[0].state, "APPROVED");
        assert!(!page.has_next_page);
        assert_eq!(page.end_cursor, Some("cursor123".into()));
        assert_eq!(page.rate_limit.remaining, 4999);
    }

    #[tokio::test]
    async fn test_graphql_error_response() {
        let mock_server = MockServer::start().await;

        let error_body = serde_json::json!({
            "data": null,
            "errors": [{
                "message": "Could not resolve to a Repository"
            }]
        });

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(error_body))
            .mount(&mock_server)
            .await;

        let client =
            GitHubGraphQLClient::new(reqwest::Client::new(), &mock_server.uri(), "test-token");

        let result = client.fetch_pull_requests("org", "nonexistent", None).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Could not resolve")
        );
    }

    #[tokio::test]
    async fn test_search_pull_requests() {
        let mock_server = MockServer::start().await;

        let body = serde_json::json!({
            "data": {
                "search": {
                    "pageInfo": { "hasNextPage": true, "endCursor": "search_cursor" },
                    "issueCount": 150,
                    "nodes": [{
                        "number": 99,
                        "title": "Fix upstream bug",
                        "state": "MERGED",
                        "url": "https://github.com/org/other-repo/pull/99",
                        "isDraft": false,
                        "createdAt": "2024-02-01T00:00:00Z",
                        "updatedAt": "2024-02-05T00:00:00Z",
                        "closedAt": "2024-02-05T00:00:00Z",
                        "mergedAt": "2024-02-05T00:00:00Z",
                        "additions": 20,
                        "deletions": 3,
                        "changedFiles": 2,
                        "author": { "login": "alice" },
                        "bodyText": "Fixes an upstream bug",
                        "repository": {
                            "name": "other-repo",
                            "owner": { "login": "org" }
                        },
                        "labels": { "nodes": [] },
                        "headRefName": "fix-bug",
                        "baseRefName": "main",
                        "reviews": {
                            "pageInfo": { "hasNextPage": false },
                            "nodes": []
                        }
                    }]
                }
            }
        });

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&mock_server)
            .await;

        let client =
            GitHubGraphQLClient::new(reqwest::Client::new(), &mock_server.uri(), "test-token");

        let page = client
            .search_pull_requests("author:alice type:pr org:org", None)
            .await
            .expect("search_pull_requests");

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].number, Some(99));
        assert_eq!(
            page.items[0].repository.as_ref().unwrap().name,
            "other-repo"
        );
        assert!(page.has_next_page);
    }

    #[test]
    fn test_graphql_endpoint_derivation() {
        let client =
            GitHubGraphQLClient::new(reqwest::Client::new(), "https://api.github.com", "token");
        assert_eq!(client.endpoint, "https://api.github.com/graphql");

        let client = GitHubGraphQLClient::new(
            reqwest::Client::new(),
            "https://github.example.com/api/v3",
            "token",
        );
        assert_eq!(client.endpoint, "https://github.example.com/api/graphql");
    }
}
