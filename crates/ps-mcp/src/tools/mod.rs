mod convert;
mod format;
pub mod generate_image;
mod inputs;

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool_handler, tool_router,
};

use crate::prism_client::PrismClient;

use ps_proto::canonical::prism::v1 as proto;

use convert::{
    contribution_type_str_to_proto, parse_insight_period, platform_str_to_proto, state_str_to_proto,
};
use format::{
    format_contribution, format_person_insights, format_similar_item, format_team_insights,
};
use inputs::{
    CompareTeamsInput, GenerateImageInput, GetPersonProfileInput, ListPeopleInput, ListTeamsInput,
    QueryContributionsInput, QueryEnrichmentsInput, QueryTeamMetricsInput, SearchByTextInput,
    SearchSimilarInput,
};

/// MCP tool server providing Prism data query tools and image generation.
#[derive(Clone)]
pub struct PrismTools {
    client: PrismClient,
    http: reqwest::Client,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for PrismTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrismTools").finish_non_exhaustive()
    }
}

impl PrismTools {
    pub fn new(client: PrismClient) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();
        Self {
            client,
            http,
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

    /// Generate an image using an AI image generation model.
    /// The image is saved to /workspace and visible in the workspace sidebar.
    #[rmcp::tool(name = "generate_image")]
    async fn generate_image(
        &self,
        Parameters(input): Parameters<GenerateImageInput>,
    ) -> Result<String, String> {
        generate_image::generate_and_save(
            &self.http,
            &input.prompt,
            input.model.as_deref(),
            input.provider.as_deref(),
            input.aspect_ratio.as_deref(),
        )
        .await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_router_registers_all_tools() {
        let router = PrismTools::tool_router();
        let tools = router.list_all();
        assert_eq!(
            tools.len(),
            10,
            "Expected 10 MCP tools, got {}",
            tools.len()
        );

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"query_team_metrics"));
        assert!(names.contains(&"query_contributions"));
        assert!(names.contains(&"compare_teams"));
        assert!(names.contains(&"generate_image"));
        assert!(names.contains(&"get_person_profile"));
        assert!(names.contains(&"search_similar"));
        assert!(names.contains(&"search_by_text"));
        assert!(names.contains(&"query_enrichments"));
        assert!(names.contains(&"list_teams"));
        assert!(names.contains(&"list_people"));
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
}
