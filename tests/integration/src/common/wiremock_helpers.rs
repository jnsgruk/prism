use ps_core::ingestion::IngestionContext;
use ps_core::models::{Platform, SourceConfig};
use ps_core::repo::Repos;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Context provided to `define_source_test!` tests.
pub struct SourceTestContext {
    pub mock_server: wiremock::MockServer,
    pub repos: Repos,
    pub pool: PgPool,
}

impl SourceTestContext {
    /// Build an `IngestionContext` that points HTTP calls at the mock server.
    pub async fn build_ingestion_ctx(
        &self,
        source_name: &str,
        platform: Platform,
        settings: serde_json::Value,
        token: Option<String>,
        email: Option<String>,
        api_username: Option<String>,
    ) -> IngestionContext {
        let source_id = Uuid::now_v7();

        // Insert source config into DB so plan() can find it.
        self.repos
            .config
            .create_source(
                source_id,
                &platform.to_string(),
                source_name,
                &settings,
                None,
            )
            .await
            .expect("create source config");

        let source_config = SourceConfig {
            id: source_id,
            source_type: platform,
            name: source_name.to_string(),
            enabled: true,
            settings,
            schedule_cron: None,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("build http client");

        IngestionContext {
            repos: self.repos.clone(),
            source_config,
            http_client,
            token,
            email,
            api_username,
        }
    }
}

// ---------------------------------------------------------------------------
// GitHub GraphQL response builders
// ---------------------------------------------------------------------------

/// Build a GraphQL search response with the given PR nodes.
pub fn graphql_search_response(
    nodes: &[serde_json::Value],
    has_next_page: bool,
    end_cursor: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "data": {
            "search": {
                "pageInfo": {
                    "hasNextPage": has_next_page,
                    "endCursor": end_cursor,
                },
                "issueCount": nodes.len(),
                "nodes": nodes,
            }
        },
        "extensions": {
            "rateLimit": {
                "remaining": 4900,
                "limit": 5000,
                "resetAt": "2099-01-01T00:00:00Z",
            }
        }
    })
}

/// Build a single PR node for a GraphQL search response.
#[allow(clippy::too_many_arguments)]
pub fn graphql_pr_node(
    owner: &str,
    repo: &str,
    number: u32,
    author: &str,
    title: &str,
    state: &str,
    created_at: &str,
    updated_at: &str,
    additions: u32,
    deletions: u32,
    reviews: &[serde_json::Value],
) -> serde_json::Value {
    serde_json::json!({
        "number": number,
        "title": title,
        "state": state,
        "url": format!("https://github.com/{owner}/{repo}/pull/{number}"),
        "isDraft": false,
        "createdAt": created_at,
        "updatedAt": updated_at,
        "closedAt": null,
        "mergedAt": null,
        "additions": additions,
        "deletions": deletions,
        "changedFiles": 3,
        "author": { "login": author },
        "bodyText": format!("PR body for {title}"),
        "repository": {
            "name": repo,
            "owner": { "login": owner }
        },
        "labels": { "nodes": [] },
        "headRefName": "feature-branch",
        "baseRefName": "main",
        "reviews": {
            "pageInfo": { "hasNextPage": false },
            "nodes": reviews,
        }
    })
}

/// Build a review node for inclusion in a PR's reviews.
pub fn graphql_review_node(
    reviewer: &str,
    state: &str,
    submitted_at: &str,
    database_id: u64,
) -> serde_json::Value {
    serde_json::json!({
        "databaseId": database_id,
        "state": state,
        "body": format!("Review by {reviewer}"),
        "submittedAt": submitted_at,
        "author": { "login": reviewer },
        "comments": { "nodes": [] }
    })
}

/// Build a GitHub REST PR files response (for diff fetching).
pub fn github_pr_files_response(files: &[(&str, &str)]) -> serde_json::Value {
    let items: Vec<serde_json::Value> = files
        .iter()
        .map(|(filename, patch)| {
            serde_json::json!({
                "filename": filename,
                "status": "modified",
                "patch": patch,
            })
        })
        .collect();
    serde_json::Value::Array(items)
}

// ---------------------------------------------------------------------------
// Jira response builders
// ---------------------------------------------------------------------------

/// Build a Jira search response.
pub fn jira_search_response(
    issues: &[serde_json::Value],
    is_last: bool,
    next_page_token: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "startAt": 0,
        "maxResults": 50,
        "total": issues.len(),
        "issues": issues,
        "isLast": is_last,
        "nextPageToken": next_page_token,
    })
}

/// Build a single Jira issue node.
pub fn jira_issue_node(
    key: &str,
    summary: &str,
    status_category_key: &str,
    assignee_account_id: &str,
    created: &str,
    updated: &str,
) -> serde_json::Value {
    serde_json::json!({
        "key": key,
        "self": format!("https://jira.example.com/rest/api/3/issue/{key}"),
        "fields": {
            "summary": summary,
            "description": null,
            "status": {
                "name": "In Progress",
                "statusCategory": {
                    "key": status_category_key,
                    "name": "In Progress"
                }
            },
            "issuetype": {
                "name": "Story",
                "subtask": false
            },
            "priority": {
                "name": "Medium"
            },
            "labels": ["backend"],
            "assignee": {
                "accountId": assignee_account_id,
                "displayName": "Test User",
                "emailAddress": "test@example.com"
            },
            "reporter": {
                "accountId": "reporter-1",
                "displayName": "Reporter"
            },
            "created": created,
            "updated": updated,
            "resolutiondate": null,
            "parent": null
        },
        "changelog": {
            "histories": []
        }
    })
}

// ---------------------------------------------------------------------------
// Discourse response builders
// ---------------------------------------------------------------------------

/// Build a Discourse /latest.json response.
pub fn discourse_latest_response(
    topics: &[serde_json::Value],
    has_more: bool,
) -> serde_json::Value {
    serde_json::json!({
        "topic_list": {
            "topics": topics,
            "more_topics_url": if has_more {
                Some("/latest.json?page=1")
            } else {
                None::<&str>
            },
        }
    })
}

/// Build a Discourse topic summary for /latest.json.
pub fn discourse_topic_summary(
    id: i64,
    title: &str,
    slug: &str,
    category_id: Option<i64>,
    posts_count: i32,
    created_at: &str,
    bumped_at: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "title": title,
        "slug": slug,
        "posts_count": posts_count,
        "views": 42,
        "category_id": category_id,
        "created_at": created_at,
        "bumped_at": bumped_at,
        "last_posted_at": bumped_at,
        "pinned": false,
        "has_accepted_answer": false,
        "tags": [],
    })
}

/// Build a Discourse topic detail response (/t/{id}.json).
pub fn discourse_topic_detail(
    id: i64,
    title: &str,
    slug: &str,
    posts: &[serde_json::Value],
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "title": title,
        "slug": slug,
        "posts_count": posts.len(),
        "views": 42,
        "category_id": 1,
        "created_at": "2025-03-01T10:00:00Z",
        "bumped_at": "2025-03-15T10:00:00Z",
        "has_accepted_answer": false,
        "tags": [],
        "post_stream": {
            "posts": posts,
        }
    })
}

/// Build a Discourse post node.
pub fn discourse_post(
    id: i64,
    topic_id: i64,
    username: &str,
    post_number: i32,
    created_at: &str,
    raw: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "topic_id": topic_id,
        "username": username,
        "name": username,
        "post_number": post_number,
        "reply_count": 0,
        "actions_summary": [],
        "created_at": created_at,
        "updated_at": created_at,
        "raw": raw,
        "reply_to_post_number": if post_number > 1 { Some(1) } else { None::<i32> },
    })
}

/// Build a Discourse categories response.
pub fn discourse_categories_response(categories: &[(i64, &str, &str)]) -> serde_json::Value {
    let cats: Vec<serde_json::Value> = categories
        .iter()
        .map(|(id, name, slug)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "slug": slug,
            })
        })
        .collect();
    serde_json::json!({
        "category_list": {
            "categories": cats,
        }
    })
}
