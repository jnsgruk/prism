use serde::Deserialize;

// ---------------------------------------------------------------------------
// GraphQL response types
// ---------------------------------------------------------------------------

/// Top-level data wrapper for the PR fetch query.
#[derive(Debug, Deserialize)]
pub struct GraphQLRepoData {
    pub repository: GraphQLRepository,
}

#[derive(Debug, Deserialize)]
pub struct GraphQLRepository {
    #[serde(rename = "pullRequests")]
    pub pull_requests: GraphQLPrConnection,
}

#[derive(Debug, Deserialize)]
pub struct GraphQLPrConnection {
    #[serde(rename = "pageInfo")]
    pub page_info: GraphQLPageInfo,
    pub nodes: Vec<GraphQLPr>,
}

/// A pull request as returned by the GraphQL API.
#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLPr {
    pub number: u32,
    pub title: String,
    pub state: String,
    pub url: String,
    #[serde(rename = "isDraft")]
    pub is_draft: bool,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    #[serde(rename = "closedAt")]
    pub closed_at: Option<String>,
    #[serde(rename = "mergedAt")]
    pub merged_at: Option<String>,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    #[serde(rename = "changedFiles")]
    pub changed_files: Option<u32>,
    pub author: Option<GraphQLActor>,
    #[serde(rename = "bodyText")]
    pub body_text: Option<String>,
    pub labels: Option<GraphQLLabelConnection>,
    #[serde(rename = "headRefName")]
    pub head_ref_name: Option<String>,
    #[serde(rename = "baseRefName")]
    pub base_ref_name: Option<String>,
    pub reviews: GraphQLReviewConnection,
}

/// A review as returned by the GraphQL API.
#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLReview {
    #[serde(rename = "databaseId")]
    pub database_id: Option<u64>,
    pub state: String,
    pub body: Option<String>,
    #[serde(rename = "submittedAt")]
    pub submitted_at: Option<String>,
    pub author: Option<GraphQLActor>,
    pub comments: Option<GraphQLReviewCommentConnection>,
}

/// Connection for inline review comments.
#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLReviewCommentConnection {
    pub nodes: Vec<GraphQLReviewComment>,
}

/// An inline review comment (file-level feedback).
#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLReviewComment {
    pub body: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLReviewConnection {
    #[serde(rename = "pageInfo")]
    pub page_info: GraphQLPageInfo,
    pub nodes: Vec<GraphQLReview>,
}

/// An actor (user) in the GraphQL API — used for PR author and review author.
#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLActor {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLPageInfo {
    #[serde(rename = "hasNextPage")]
    pub has_next_page: bool,
    #[serde(rename = "endCursor")]
    pub end_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLLabelConnection {
    pub nodes: Vec<GraphQLLabel>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLLabel {
    pub name: String,
}

/// Top-level data wrapper for the search query.
#[derive(Debug, Deserialize)]
pub struct GraphQLSearchData {
    pub search: GraphQLSearchConnection,
}

#[derive(Debug, Deserialize)]
pub struct GraphQLSearchConnection {
    #[serde(rename = "pageInfo")]
    pub page_info: GraphQLPageInfo,
    #[serde(rename = "issueCount")]
    pub issue_count: Option<u32>,
    pub nodes: Vec<GraphQLSearchPr>,
}

/// A PR node from a GraphQL search query. Fields are optional because search
/// nodes can be non-PR types (filtered out by the `... on PullRequest` fragment,
/// which produces empty objects for non-matches).
#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLSearchPr {
    pub number: Option<u32>,
    pub title: Option<String>,
    pub state: Option<String>,
    pub url: Option<String>,
    #[serde(rename = "isDraft")]
    pub is_draft: Option<bool>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
    #[serde(rename = "closedAt")]
    pub closed_at: Option<String>,
    #[serde(rename = "mergedAt")]
    pub merged_at: Option<String>,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    #[serde(rename = "changedFiles")]
    pub changed_files: Option<u32>,
    pub author: Option<GraphQLActor>,
    #[serde(rename = "bodyText")]
    pub body_text: Option<String>,
    pub repository: Option<GraphQLSearchRepo>,
    pub labels: Option<GraphQLLabelConnection>,
    #[serde(rename = "headRefName")]
    pub head_ref_name: Option<String>,
    #[serde(rename = "baseRefName")]
    pub base_ref_name: Option<String>,
    pub reviews: Option<GraphQLReviewConnection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLSearchRepo {
    pub name: String,
    pub owner: GraphQLSearchRepoOwner,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphQLSearchRepoOwner {
    pub login: String,
}

// ---------------------------------------------------------------------------
// REST API response types (used by team sync handler)
// ---------------------------------------------------------------------------

/// A GitHub pull request as returned by the REST API.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubPr {
    pub number: u32,
    pub title: String,
    pub state: String,
    pub user: GitHubUser,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    pub merged_at: Option<String>,
    pub draft: Option<bool>,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub changed_files: Option<u32>,
    pub labels: Option<Vec<GitHubLabel>>,
    pub head: Option<GitHubRef>,
    pub base: Option<GitHubRef>,
}

/// A GitHub pull request review.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubReview {
    pub id: u64,
    pub user: GitHubUser,
    pub state: String,
    pub submitted_at: Option<String>,
    pub body: Option<String>,
}

/// A GitHub user (embedded in PR/review responses).
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubUser {
    pub login: String,
    pub id: u64,
}

/// A GitHub label.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubLabel {
    pub name: String,
}

/// A Git reference (head/base branch) on a PR.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRef {
    #[serde(rename = "ref")]
    pub ref_name: String,
}

/// A GitHub repository as returned by the org repos endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRepo {
    pub name: String,
    pub full_name: String,
    pub owner: GitHubRepoOwner,
    pub archived: Option<bool>,
    pub default_branch: Option<String>,
    pub language: Option<String>,
}

/// Repository owner info.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRepoOwner {
    pub login: String,
}

/// A GitHub team as returned by the org teams endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubTeam {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
}

/// A GitHub repository as returned by the team repos endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubTeamRepo {
    pub name: String,
    pub owner: GitHubRepoOwner,
    pub archived: Option<bool>,
}
