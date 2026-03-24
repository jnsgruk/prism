use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool_handler, tool_router,
};

use crate::artifact_store::ArtifactStore;
use crate::prism_client::PrismClient;

use ps_proto::canonical::prism::v1 as proto;

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
    /// Maximum results to return (default 25, max 100).
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
    /// Maximum results (default 10).
    pub limit: Option<i32>,
    /// Filter by platform.
    pub platform: Option<String>,
}

/// Input for semantic text search.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchByTextInput {
    /// Free-text search query.
    pub query: String,
    /// Maximum results (default 10).
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

/// Input for uploading an artifact.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UploadArtifactInput {
    /// Path to the file in /workspace to upload.
    pub file_path: String,
    /// Human-readable display name (defaults to filename).
    pub display_name: Option<String>,
}

/// MCP tool server providing Prism data tools and S3 artifact management.
#[derive(Clone)]
pub struct PrismTools {
    client: PrismClient,
    artifacts: ArtifactStore,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for PrismTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrismTools").finish_non_exhaustive()
    }
}

impl PrismTools {
    pub fn new(client: PrismClient, artifacts: ArtifactStore) -> Self {
        Self {
            client,
            artifacts,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl PrismTools {
    /// Get enrichment-based insights for a team: review quality, PR significance,
    /// sentiment breakdown, notable contributions, and trends.
    #[rmcp::tool(name = "query_team_metrics")]
    async fn query_team_metrics(
        &self,
        Parameters(input): Parameters<QueryTeamMetricsInput>,
    ) -> Result<String, String> {
        let team_id = self.resolve_team_id(&input.team_name).await?;

        let req = proto::GetTeamInsightsRequest {
            team_id,
            period: parse_insight_period(&input.period),
            include_descendants: true,
        };

        let resp = self
            .client
            .insights
            .clone()
            .get_team_insights(req)
            .await
            .map_err(|e| format!("gRPC error: {e}"))?;

        let inner = resp.into_inner();
        serde_json::to_string_pretty(&format_team_insights(&inner))
            .map_err(|e| format!("serialization error: {e}"))
    }

    /// Search and filter contributions for a team in a time period.
    /// Returns contribution titles, authors, states, URLs, and dates.
    #[rmcp::tool(name = "query_contributions")]
    async fn query_contributions(
        &self,
        Parameters(input): Parameters<QueryContributionsInput>,
    ) -> Result<String, String> {
        let team_id = self.resolve_team_id(&input.team_name).await?;

        let req = proto::ListTeamContributionsRequest {
            team_id,
            period: Some(proto::Period {
                r#type: proto::PeriodType::Unspecified.into(),
                start: input.period_start,
                end: input.period_end,
            }),
            contribution_type: contribution_type_str_to_proto(input.contribution_type.as_deref()),
            state: state_str_to_proto(input.state.as_deref()),
            platform: platform_str_to_proto(input.platform.as_deref()),
            page_size: input.limit.unwrap_or(25),
            search: input.search,
            ..Default::default()
        };

        let resp = self
            .client
            .metrics
            .clone()
            .list_team_contributions(req)
            .await
            .map_err(|e| format!("gRPC error: {e}"))?;

        let inner = resp.into_inner();
        let contributions: Vec<_> = inner
            .contributions
            .iter()
            .map(format_contribution)
            .collect();

        serde_json::to_string_pretty(&serde_json::json!({
            "total_count": inner.total_count,
            "contributions": contributions,
        }))
        .map_err(|e| format!("serialization error: {e}"))
    }

    /// Compare enrichment-based insights side-by-side for two or more teams.
    #[rmcp::tool(name = "compare_teams")]
    async fn compare_teams(
        &self,
        Parameters(input): Parameters<CompareTeamsInput>,
    ) -> Result<String, String> {
        let period = parse_insight_period(&input.period);
        let mut results = Vec::new();

        for name in &input.team_names {
            let team_id = self.resolve_team_id(name).await?;
            let req = proto::GetTeamInsightsRequest {
                team_id,
                period,
                include_descendants: true,
            };
            let resp = self
                .client
                .insights
                .clone()
                .get_team_insights(req)
                .await
                .map_err(|e| format!("gRPC error for team {name}: {e}"))?;
            let inner = resp.into_inner();
            results.push(serde_json::json!({
                "team": name,
                "insights": format_team_insights(&inner),
            }));
        }

        serde_json::to_string_pretty(&results).map_err(|e| format!("serialization error: {e}"))
    }

    /// Get a person's enrichment-based insights: review profile, PR impact,
    /// Discourse activity, and notable contributions.
    #[rmcp::tool(name = "get_person_profile")]
    async fn get_person_profile(
        &self,
        Parameters(input): Parameters<GetPersonProfileInput>,
    ) -> Result<String, String> {
        let person_id = self.resolve_person_id(&input.person_name).await?;
        let period = input
            .period
            .as_deref()
            .map_or(proto::InsightPeriod::LastMonth.into(), |p| {
                parse_insight_period(p)
            });

        let resp = self
            .client
            .insights
            .clone()
            .get_person_insights(proto::GetPersonInsightsRequest { person_id, period })
            .await
            .map_err(|e| format!("gRPC error: {e}"))?;

        let inner = resp.into_inner();
        serde_json::to_string_pretty(&format_person_insights(&inner))
            .map_err(|e| format!("serialization error: {e}"))
    }

    /// Find contributions semantically similar to a given contribution using vector embeddings.
    #[rmcp::tool(name = "search_similar")]
    async fn search_similar(
        &self,
        Parameters(input): Parameters<SearchSimilarInput>,
    ) -> Result<String, String> {
        let req = proto::FindSimilarRequest {
            contribution_id: input.contribution_id,
            limit: input.limit.unwrap_or(10),
            platform: platform_str_to_proto(input.platform.as_deref()),
            ..Default::default()
        };

        let resp = self
            .client
            .reasoning
            .clone()
            .find_similar(req)
            .await
            .map_err(|e| format!("gRPC error: {e}"))?;

        let items: Vec<_> = resp
            .into_inner()
            .items
            .iter()
            .map(format_similar_item)
            .collect();

        serde_json::to_string_pretty(&items).map_err(|e| format!("serialization error: {e}"))
    }

    /// Search contributions by semantic text query using vector embeddings.
    #[rmcp::tool(name = "search_by_text")]
    async fn search_by_text(
        &self,
        Parameters(input): Parameters<SearchByTextInput>,
    ) -> Result<String, String> {
        let req = proto::SearchByTextRequest {
            query_text: input.query,
            limit: input.limit.unwrap_or(10),
            platform: platform_str_to_proto(input.platform.as_deref()),
            ..Default::default()
        };

        let resp = self
            .client
            .reasoning
            .clone()
            .search_by_text(req)
            .await
            .map_err(|e| format!("gRPC error: {e}"))?;

        let items: Vec<_> = resp
            .into_inner()
            .items
            .iter()
            .map(format_similar_item)
            .collect();

        serde_json::to_string_pretty(&items).map_err(|e| format!("serialization error: {e}"))
    }

    /// Get AI enrichment scores (review depth, sentiment, significance) for a contribution.
    #[rmcp::tool(name = "query_enrichments")]
    async fn query_enrichments(
        &self,
        Parameters(input): Parameters<QueryEnrichmentsInput>,
    ) -> Result<String, String> {
        let req = proto::GetEnrichmentsRequest {
            contribution_id: input.contribution_id,
        };

        let resp = self
            .client
            .reasoning
            .clone()
            .get_enrichments(req)
            .await
            .map_err(|e| format!("gRPC error: {e}"))?;

        let enrichments: Vec<_> = resp
            .into_inner()
            .enrichments
            .iter()
            .map(|e| {
                serde_json::json!({
                    "type": e.enrichment_type,
                    "value": &e.value_json,
                    "model": &e.model_name,
                    "confidence": e.confidence,
                })
            })
            .collect();

        serde_json::to_string_pretty(&enrichments).map_err(|e| format!("serialization error: {e}"))
    }

    /// List all teams with their member counts and hierarchy.
    /// Use this to discover team names and IDs.
    #[rmcp::tool(name = "list_teams")]
    async fn list_teams(
        &self,
        Parameters(input): Parameters<ListTeamsInput>,
    ) -> Result<String, String> {
        let req = proto::ListTeamsRequest {
            parent_team_id: input.parent_team_id,
            ..Default::default()
        };

        let resp = self
            .client
            .org
            .clone()
            .list_teams(req)
            .await
            .map_err(|e| format!("gRPC error: {e}"))?;

        let teams: Vec<_> = resp
            .into_inner()
            .teams
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": &t.id,
                    "name": &t.name,
                    "org_name": &t.org_name,
                    "member_count": t.member_count,
                    "parent_team_id": &t.parent_team_id,
                })
            })
            .collect();

        serde_json::to_string_pretty(&teams).map_err(|e| format!("serialization error: {e}"))
    }

    /// List people, optionally filtered by team or search term.
    #[rmcp::tool(name = "list_people")]
    async fn list_people(
        &self,
        Parameters(input): Parameters<ListPeopleInput>,
    ) -> Result<String, String> {
        let team_id = if let Some(ref name) = input.team_name {
            Some(self.resolve_team_id(name).await?)
        } else {
            None
        };

        let req = proto::ListPeopleRequest {
            team_id,
            search: input.search,
            ..Default::default()
        };

        let resp = self
            .client
            .org
            .clone()
            .list_people(req)
            .await
            .map_err(|e| format!("gRPC error: {e}"))?;

        let people: Vec<_> = resp
            .into_inner()
            .people
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": &p.id,
                    "name": &p.name,
                    "team_name": &p.team_name,
                    "active": p.active,
                })
            })
            .collect();

        serde_json::to_string_pretty(&people).map_err(|e| format!("serialization error: {e}"))
    }

    /// Upload a file from /workspace as a conversation artifact to S3.
    /// Returns the artifact key and confirmation.
    #[rmcp::tool(name = "upload_artifact")]
    async fn upload_artifact(
        &self,
        Parameters(input): Parameters<UploadArtifactInput>,
    ) -> Result<String, String> {
        let path = std::path::Path::new(&input.file_path);
        if !path.exists() {
            return Err(format!("File not found: {}", input.file_path));
        }

        let data = tokio::fs::read(&input.file_path)
            .await
            .map_err(|e| format!("Failed to read file: {e}"))?;

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("artifact")
            .to_string();
        let display_name = input.display_name.as_deref().unwrap_or(&filename);
        let content_type = guess_content_type(&filename);
        let size = data.len();

        let key = self
            .artifacts
            .upload(&filename, Some(content_type), data.into())
            .await
            .map_err(|e| format!("S3 upload failed: {e}"))?;

        serde_json::to_string_pretty(&serde_json::json!({
            "status": "uploaded",
            "artifact_key": key,
            "display_name": display_name,
            "content_type": content_type,
            "size_bytes": size,
        }))
        .map_err(|e| format!("serialization error: {e}"))
    }

    /// List all artifacts uploaded in the current conversation.
    #[rmcp::tool(name = "list_artifacts")]
    async fn list_artifacts(&self) -> Result<String, String> {
        let entries = self
            .artifacts
            .list()
            .await
            .map_err(|e| format!("S3 list failed: {e}"))?;

        let items: Vec<_> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "key": &e.key,
                    "filename": &e.filename,
                    "size_bytes": e.size_bytes,
                })
            })
            .collect();

        serde_json::to_string_pretty(&items).map_err(|e| format!("serialization error: {e}"))
    }
}

#[tool_handler]
impl ServerHandler for PrismTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Prism engineering insights tools. Query team metrics, search contributions, \
                 compare teams, get person profiles, and manage analysis artifacts.",
        )
    }
}

// ---------------------------------------------------------------------------
// Helper methods
// ---------------------------------------------------------------------------

impl PrismTools {
    /// Resolve a team name to its UUID by listing teams and matching.
    async fn resolve_team_id(&self, name: &str) -> Result<String, String> {
        let resp = self
            .client
            .org
            .clone()
            .list_teams(proto::ListTeamsRequest::default())
            .await
            .map_err(|e| format!("gRPC error resolving team: {e}"))?;

        let teams = &resp.into_inner().teams;
        let lower = name.to_lowercase();

        // Exact match first, then substring match.
        let team = teams
            .iter()
            .find(|t| t.name.to_lowercase() == lower)
            .or_else(|| {
                teams
                    .iter()
                    .find(|t| t.name.to_lowercase().contains(&lower))
            })
            .ok_or_else(|| format!("Team not found: {name}"))?;

        Ok(team.id.clone())
    }

    /// Resolve a person name to their UUID.
    async fn resolve_person_id(&self, name: &str) -> Result<String, String> {
        let resp = self
            .client
            .org
            .clone()
            .list_people(proto::ListPeopleRequest {
                search: Some(name.to_string()),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("gRPC error resolving person: {e}"))?;

        let people = &resp.into_inner().people;
        let lower = name.to_lowercase();

        let person = people
            .iter()
            .find(|p| p.name.to_lowercase() == lower)
            .or_else(|| {
                people
                    .iter()
                    .find(|p| p.name.to_lowercase().contains(&lower))
            })
            .ok_or_else(|| format!("Person not found: {name}"))?;

        Ok(person.id.clone())
    }
}

// ---------------------------------------------------------------------------
// Proto → JSON formatting helpers
// ---------------------------------------------------------------------------

fn format_team_insights(resp: &proto::GetTeamInsightsResponse) -> serde_json::Value {
    resp.insights.as_ref().map_or(
        serde_json::json!({"message": "No insights available for this period"}),
        |ins| {
            serde_json::json!({
                "review_quality": ins.review_quality.as_ref().map(|r| serde_json::json!({
                    "avg_depth": r.avg_depth,
                    "total_reviews": r.total_reviews,
                    "rubber_stamp_pct": r.rubber_stamp_pct,
                    "deep_review_pct": r.deep_review_pct,
                    "constructive_count": r.constructive_count,
                    "neutral_count": r.neutral_count,
                    "critical_count": r.critical_count,
                })),
                "pr_significance": ins.pr_significance.as_ref().map(|s| serde_json::json!({
                    "significant_count": s.significant_count,
                    "notable_count": s.notable_count,
                    "routine_count": s.routine_count,
                })),
                "notable_items": ins.notable_items.iter().map(|n| serde_json::json!({
                    "contribution_id": &n.contribution_id,
                    "title": &n.title,
                    "rationale": &n.rationale,
                })).collect::<Vec<_>>(),
                "coverage": ins.coverage.as_ref().map(|c| serde_json::json!({
                    "total_contributions": c.total_contributions,
                    "enriched_count": c.enriched_contributions,
                })),
            })
        },
    )
}

fn format_contribution(c: &proto::Contribution) -> serde_json::Value {
    serde_json::json!({
        "id": &c.id,
        "title": &c.title,
        "person_name": &c.person_name,
        "platform": c.platform,
        "contribution_type": c.contribution_type,
        "state": c.state,
        "url": &c.url,
        "repo": &c.repo,
        "created_at": c.created_at.as_ref().map(|t| t.seconds),
    })
}

fn format_similar_item(s: &proto::SimilarItem) -> serde_json::Value {
    serde_json::json!({
        "contribution_id": &s.contribution_id,
        "title": &s.title,
        "platform": s.platform,
        "contribution_type": s.contribution_type,
        "author_name": &s.author_name,
        "external_url": &s.external_url,
        "distance": s.distance,
    })
}

fn format_person_insights(resp: &proto::GetPersonInsightsResponse) -> serde_json::Value {
    resp.insights.as_ref().map_or(
        serde_json::json!({"message": "No insights available"}),
        |ins| {
            serde_json::json!({
                "reviewer_profile": ins.reviewer_profile.as_ref().map(|r| serde_json::json!({
                    "avg_depth": r.avg_depth,
                    "total_reviews_given": r.total_reviews_given,
                    "rubber_stamp_pct": r.rubber_stamp_pct,
                    "constructive_count": r.constructive_count,
                    "neutral_count": r.neutral_count,
                })),
                "pr_impact": ins.pr_impact.as_ref().map(|s| serde_json::json!({
                    "significant_count": s.significant_count,
                    "notable_count": s.notable_count,
                    "routine_count": s.routine_count,
                })),
                "highlights": ins.highlights.iter().map(|n| serde_json::json!({
                    "contribution_id": &n.contribution_id,
                    "title": &n.title,
                    "rationale": &n.rationale,
                })).collect::<Vec<_>>(),
                "coverage": ins.coverage.as_ref().map(|c| serde_json::json!({
                    "total_contributions": c.total_contributions,
                    "enriched_count": c.enriched_contributions,
                })),
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Proto enum conversions (string → i32)
//
// Delegates to canonical implementations in ps_proto::convert.
// ---------------------------------------------------------------------------

fn parse_insight_period(s: &str) -> i32 {
    match s.to_lowercase().as_str() {
        "last_week" | "week" => proto::InsightPeriod::LastWeek.into(),
        "last_quarter" | "quarter" => proto::InsightPeriod::LastQuarter.into(),
        "last_year" | "year" => proto::InsightPeriod::LastYear.into(),
        // Default to last_month for unrecognised periods.
        _ => proto::InsightPeriod::LastMonth.into(),
    }
}

fn platform_str_to_proto(s: Option<&str>) -> i32 {
    s.map_or(proto::Platform::Unspecified.into(), |v| {
        proto::Platform::from_user_str(v).into()
    })
}

fn contribution_type_str_to_proto(s: Option<&str>) -> i32 {
    s.map_or(proto::ContributionType::Unspecified.into(), |v| {
        proto::ContributionType::from_user_str(v).into()
    })
}

fn state_str_to_proto(s: Option<&str>) -> i32 {
    s.map_or(proto::ContributionState::Unspecified.into(), |v| {
        proto::ContributionState::from_user_str(v).into()
    })
}

fn guess_content_type(filename: &str) -> &'static str {
    match filename.rsplit('.').next() {
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("md") => "text/markdown",
        Some("txt") => "text/plain",
        Some("html") => "text/html",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Tool registration
    // -----------------------------------------------------------------------

    #[test]
    fn tool_router_registers_all_11_tools() {
        let router = PrismTools::tool_router();
        let tools = router.list_all();
        assert_eq!(
            tools.len(),
            11,
            "Expected 11 MCP tools, got {}",
            tools.len()
        );

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"query_team_metrics"));
        assert!(names.contains(&"query_contributions"));
        assert!(names.contains(&"compare_teams"));
        assert!(names.contains(&"get_person_profile"));
        assert!(names.contains(&"search_similar"));
        assert!(names.contains(&"search_by_text"));
        assert!(names.contains(&"query_enrichments"));
        assert!(names.contains(&"list_teams"));
        assert!(names.contains(&"list_people"));
        assert!(names.contains(&"upload_artifact"));
        assert!(names.contains(&"list_artifacts"));
    }

    #[test]
    fn tools_are_sorted_alphabetically() {
        let router = PrismTools::tool_router();
        let tools = router.list_all();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn all_tools_have_descriptions() {
        let router = PrismTools::tool_router();
        for tool in router.list_all() {
            assert!(
                tool.description.is_some(),
                "Tool '{}' has no description",
                tool.name
            );
        }
    }

    // -----------------------------------------------------------------------
    // Enum conversions (delegating to ps_proto::convert)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_insight_period_valid() {
        assert_eq!(
            parse_insight_period("last_week"),
            i32::from(proto::InsightPeriod::LastWeek)
        );
        assert_eq!(
            parse_insight_period("week"),
            i32::from(proto::InsightPeriod::LastWeek)
        );
        assert_eq!(
            parse_insight_period("LAST_QUARTER"),
            i32::from(proto::InsightPeriod::LastQuarter)
        );
    }

    #[test]
    fn parse_insight_period_defaults_to_month() {
        assert_eq!(
            parse_insight_period("unknown"),
            i32::from(proto::InsightPeriod::LastMonth)
        );
        assert_eq!(
            parse_insight_period(""),
            i32::from(proto::InsightPeriod::LastMonth)
        );
    }

    #[test]
    fn platform_str_to_proto_values() {
        assert_eq!(
            platform_str_to_proto(Some("github")),
            i32::from(proto::Platform::Github)
        );
        assert_eq!(
            platform_str_to_proto(Some("GitHub")),
            i32::from(proto::Platform::Github)
        );
        assert_eq!(
            platform_str_to_proto(Some("JIRA")),
            i32::from(proto::Platform::Jira)
        );
        assert_eq!(
            platform_str_to_proto(None),
            i32::from(proto::Platform::Unspecified)
        );
    }

    #[test]
    fn contribution_type_str_to_proto_all_variants() {
        assert_eq!(
            contribution_type_str_to_proto(Some("pull_request")),
            i32::from(proto::ContributionType::PullRequest)
        );
        assert_eq!(
            contribution_type_str_to_proto(Some("pr_review")),
            i32::from(proto::ContributionType::PrReview)
        );
        assert_eq!(
            contribution_type_str_to_proto(Some("review")),
            i32::from(proto::ContributionType::PrReview)
        );
        assert_eq!(
            contribution_type_str_to_proto(Some("discourse_like")),
            i32::from(proto::ContributionType::DiscourseLike)
        );
        assert_eq!(
            contribution_type_str_to_proto(None),
            i32::from(proto::ContributionType::Unspecified)
        );
    }

    #[test]
    fn state_str_to_proto_all_variants() {
        assert_eq!(
            state_str_to_proto(Some("open")),
            i32::from(proto::ContributionState::Open)
        );
        assert_eq!(
            state_str_to_proto(Some("MERGED")),
            i32::from(proto::ContributionState::Merged)
        );
        assert_eq!(
            state_str_to_proto(Some("in_progress")),
            i32::from(proto::ContributionState::InProgress)
        );
        assert_eq!(
            state_str_to_proto(Some("approved")),
            i32::from(proto::ContributionState::Approved)
        );
        assert_eq!(
            state_str_to_proto(Some("done")),
            i32::from(proto::ContributionState::Done)
        );
        assert_eq!(
            state_str_to_proto(None),
            i32::from(proto::ContributionState::Unspecified)
        );
    }

    // -----------------------------------------------------------------------
    // Content type guessing
    // -----------------------------------------------------------------------

    #[test]
    fn guess_content_type_known_extensions() {
        assert_eq!(guess_content_type("report.csv"), "text/csv");
        assert_eq!(guess_content_type("data.json"), "application/json");
        assert_eq!(guess_content_type("readme.md"), "text/markdown");
        assert_eq!(guess_content_type("notes.txt"), "text/plain");
        assert_eq!(guess_content_type("chart.png"), "image/png");
        assert_eq!(guess_content_type("doc.pdf"), "application/pdf");
    }

    #[test]
    fn guess_content_type_unknown_defaults_to_octet_stream() {
        assert_eq!(
            guess_content_type("archive.tar.gz"),
            "application/octet-stream"
        );
        assert_eq!(guess_content_type("noext"), "application/octet-stream");
    }
}
