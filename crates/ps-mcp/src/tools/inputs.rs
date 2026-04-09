use rmcp::schemars;

/// Input for querying team metrics.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct QueryTeamMetricsInput {
    /// Team name (resolved to ID automatically).
    pub team_name: String,
    /// Period: "`last_week`", "`last_month`", "`last_quarter`", or "`last_year`".
    pub period: String,
}

/// Input for querying contributions.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct QueryContributionsInput {
    /// Team name to list contributions for.
    pub team_name: String,
    /// Start date in YYYY-MM-DD format.
    pub period_start: String,
    /// End date in YYYY-MM-DD format.
    pub period_end: String,
    /// Filter by platform (github, jira, discourse).
    pub platform: Option<String>,
    /// Filter by contribution type (`pull_request`, `pr_review`, `jira_ticket`, `discourse_topic`).
    pub contribution_type: Option<String>,
    /// Filter by state (open, merged, closed).
    pub state: Option<String>,
    /// Free-text search across title, author, and repo.
    pub search: Option<String>,
    /// Maximum results to return (default 50, max 100).
    pub limit: Option<i32>,
    /// Page offset for pagination (0-based, default 0). Each page returns `limit` results.
    pub offset: Option<i32>,
}

/// Input for querying a person's contributions directly.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetPersonContributionsInput {
    /// Person name (resolved to ID automatically).
    pub person_name: String,
    /// Start date in YYYY-MM-DD format.
    pub period_start: Option<String>,
    /// End date in YYYY-MM-DD format.
    pub period_end: Option<String>,
    /// Filter by platform (github, jira, discourse).
    pub platform: Option<String>,
    /// Filter by contribution type (`pull_request`, `pr_review`, `jira_ticket`, `discourse_topic`).
    pub contribution_type: Option<String>,
    /// Filter by state (open, merged, closed, `in_progress`, approved, done).
    pub state: Option<String>,
    /// Free-text search across title and repo.
    pub search: Option<String>,
    /// Maximum results to return (default 50, max 100).
    pub limit: Option<i32>,
}

/// Input for comparing teams.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CompareTeamsInput {
    /// Team names to compare.
    pub team_names: Vec<String>,
    /// Period: "`last_week`", "`last_month`", "`last_quarter`", or "`last_year`".
    pub period: String,
}

/// Input for getting a person profile.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetPersonProfileInput {
    /// Person name.
    pub person_name: String,
    /// Period: "`last_week`", "`last_month`", "`last_quarter`", or "`last_year`".
    pub period: Option<String>,
}

/// Input for finding similar contributions.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchSimilarInput {
    /// Contribution ID to find similar items for.
    pub contribution_id: String,
    /// Maximum results (default 50).
    pub limit: Option<i32>,
    /// Filter by platform.
    pub platform: Option<String>,
}

/// Input for semantic text search.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchByTextInput {
    /// Free-text search query.
    pub query: String,
    /// Maximum results (default 50).
    pub limit: Option<i32>,
    /// Filter by platform.
    pub platform: Option<String>,
}

/// Input for querying enrichments.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct QueryEnrichmentsInput {
    /// Contribution ID to get enrichments for.
    pub contribution_id: String,
}

/// Input for listing teams.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListTeamsInput {
    /// Optional parent team ID to list children of.
    pub parent_team_id: Option<String>,
}

/// Input for listing people.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListPeopleInput {
    /// Filter by team name.
    pub team_name: Option<String>,
    /// Optional search filter.
    pub search: Option<String>,
}

/// Input for generating an image.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GenerateImageInput {
    /// The user's image prompt — pass through exactly as given.
    pub prompt: String,
    /// Model ID (e.g. `"google/imagen-3"`).
    /// Optional — uses the configured default image model if omitted.
    pub model: Option<String>,
    /// Deprecated — ignored. Kept for schema backwards compatibility.
    pub provider: Option<String>,
    /// Aspect ratio (e.g. `"1:1"`, `"16:9"`). Defaults to `"1:1"`.
    pub aspect_ratio: Option<String>,
}
