use serde::Deserialize;

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
