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

    let client = GitHubGraphQLClient::new(reqwest::Client::new(), &mock_server.uri(), "test-token");

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

    let client = GitHubGraphQLClient::new(reqwest::Client::new(), &mock_server.uri(), "test-token");

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

    let client = GitHubGraphQLClient::new(reqwest::Client::new(), &mock_server.uri(), "test-token");

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
