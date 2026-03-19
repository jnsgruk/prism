use ps_core::repo::Repos;
use ps_core::repo::insights;
use ps_proto::prism::v1::insights_service_server::InsightsService;
use ps_proto::prism::v1::{
    DepthBySignificance, EnrichmentCoverage, GetOrgInsightsRequest, GetOrgInsightsResponse,
    GetPersonInsightsRequest, GetPersonInsightsResponse, GetTeamInsightsRequest,
    GetTeamInsightsResponse, InsightTrend, NotableContribution, OrgDeliverySummary, OrgInsights,
    PersonInsights, ReviewQualitySummary, ReviewerDepth, ReviewerProfile, ReviewsReceivedSummary,
    SignificanceSummary, TeamInsights, TeamReviewComparison, TopicCategoryCount,
    TopicCategorySummary, TypeCoverage,
};
use time::OffsetDateTime;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::common::{db_err, require_auth};

pub struct InsightsServiceImpl {
    repos: Repos,
}

impl InsightsServiceImpl {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }
}

/// Parse a period string ("`last_week`", "`last_month`", "`last_quarter`") into a
/// `since` timestamp.
#[allow(clippy::result_large_err)]
fn parse_period_since(period: &str) -> Result<OffsetDateTime, Status> {
    let now = OffsetDateTime::now_utc();
    let duration = match period {
        "last_week" => time::Duration::weeks(1),
        "last_month" => time::Duration::days(30),
        "last_quarter" => time::Duration::days(90),
        "last_year" => time::Duration::days(365),
        _ => {
            return Err(Status::invalid_argument(format!(
                "invalid period: {period}"
            )));
        }
    };
    Ok(now - duration)
}

/// Map a period string to the snapshot `period_type` and approximate `period_start` date.
#[allow(clippy::result_large_err)]
fn parse_period_snapshot_params(period: &str) -> Result<(String, time::Date), Status> {
    let now = OffsetDateTime::now_utc();
    match period {
        "last_week" => Ok(("week".to_string(), (now - time::Duration::weeks(1)).date())),
        "last_month" => Ok(("month".to_string(), (now - time::Duration::days(30)).date())),
        "last_quarter" => Ok((
            "quarter".to_string(),
            (now - time::Duration::days(90)).date(),
        )),
        "last_year" => Ok((
            "quarter".to_string(),
            (now - time::Duration::days(365)).date(),
        )),
        _ => Err(Status::invalid_argument(format!(
            "invalid period: {period}"
        ))),
    }
}

/// Build an `InsightTrend` from a current review-quality row + significance row
/// and a previous snapshot.
#[allow(clippy::cast_lossless)]
fn build_trend(
    rq: &insights::ReviewQualityRow,
    sig: &insights::SignificanceRow,
    prev: &insights::SnapshotRow,
) -> InsightTrend {
    let total = rq.total_reviews;
    let current_rubber = if total > 0 {
        f64::from(rq.depth_1) / f64::from(total) * 100.0
    } else {
        0.0
    };
    let current_deep = if total > 0 {
        f64::from(rq.depth_4 + rq.depth_5) / f64::from(total) * 100.0
    } else {
        0.0
    };

    InsightTrend {
        has_previous: true,
        avg_depth_delta: rq.avg_depth - prev.avg_review_depth.unwrap_or(0.0) as f64,
        rubber_stamp_pct_delta: current_rubber - prev.rubber_stamp_pct.unwrap_or(0.0) as f64,
        deep_review_pct_delta: current_deep - prev.deep_review_pct.unwrap_or(0.0) as f64,
        review_count_delta: total - prev.review_count,
        significant_count_delta: sig.significant - prev.significant_count,
    }
}

fn review_quality_to_proto(
    rq: insights::ReviewQualityRow,
    top_reviewers: Vec<insights::ReviewerDepthRow>,
) -> ReviewQualitySummary {
    let total = rq.total_reviews;
    let rubber_stamp_pct = if total > 0 {
        f64::from(rq.depth_1) / f64::from(total) * 100.0
    } else {
        0.0
    };
    let deep_review_pct = if total > 0 {
        f64::from(rq.depth_4 + rq.depth_5) / f64::from(total) * 100.0
    } else {
        0.0
    };

    ReviewQualitySummary {
        avg_depth: rq.avg_depth,
        depth_distribution: vec![rq.depth_1, rq.depth_2, rq.depth_3, rq.depth_4, rq.depth_5],
        total_reviews: total,
        rubber_stamp_pct,
        deep_review_pct,
        constructive_count: rq.constructive,
        neutral_count: rq.neutral,
        critical_count: rq.critical,
        hostile_count: rq.hostile,
        top_reviewers: top_reviewers
            .into_iter()
            .map(|r| ReviewerDepth {
                person_id: r.person_id.to_string(),
                person_name: r.person_name,
                review_count: r.review_count,
                avg_depth: r.avg_depth,
            })
            .collect(),
    }
}

fn significance_to_proto(s: insights::SignificanceRow) -> SignificanceSummary {
    SignificanceSummary {
        significant_count: s.significant,
        notable_count: s.notable,
        routine_count: s.routine,
        avg_confidence: s.avg_confidence,
    }
}

fn topics_to_proto(rows: Vec<insights::TopicCategoryRow>) -> TopicCategorySummary {
    let total: i32 = rows.iter().map(|r| r.count).sum();
    TopicCategorySummary {
        categories: rows
            .into_iter()
            .map(|r| TopicCategoryCount {
                category: r.category,
                count: r.count,
            })
            .collect(),
        total_classified: total,
    }
}

fn notable_to_proto(rows: Vec<insights::NotableContributionRow>) -> Vec<NotableContribution> {
    rows.into_iter()
        .map(|r| NotableContribution {
            contribution_id: r.contribution_id.to_string(),
            title: r.title,
            url: r.url,
            person_name: r.person_name,
            platform: r.platform,
            contribution_type: r.contribution_type,
            enrichment_type: r.enrichment_type,
            value_summary: r.value_summary,
            rationale: r.rationale,
            confidence: r.confidence,
        })
        .collect()
}

fn coverage_to_proto(
    total: i32,
    enriched: i32,
    by_type: Vec<insights::TypeCoverageRow>,
) -> EnrichmentCoverage {
    EnrichmentCoverage {
        total_contributions: total,
        enriched_contributions: enriched,
        by_type: by_type
            .into_iter()
            .map(|r| TypeCoverage {
                enrichment_type: r.enrichment_type,
                eligible: r.eligible,
                enriched: r.enriched,
            })
            .collect(),
    }
}

fn depth_by_sig_to_proto(d: insights::DepthBySignificanceRow) -> DepthBySignificance {
    DepthBySignificance {
        avg_depth_significant: d.avg_depth_significant,
        avg_depth_notable: d.avg_depth_notable,
        avg_depth_routine: d.avg_depth_routine,
        significant_review_count: d.significant_review_count,
        notable_review_count: d.notable_review_count,
        routine_review_count: d.routine_review_count,
    }
}

#[tonic::async_trait]
impl InsightsService for InsightsServiceImpl {
    async fn get_team_insights(
        &self,
        request: Request<GetTeamInsightsRequest>,
    ) -> Result<Response<GetTeamInsightsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_id: Uuid = req
            .team_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;
        let since = parse_period_since(&req.period)?;
        let descendants = req.include_descendants;

        let (review_quality, top_reviewers, significance, topics, notable, coverage, depth_by_sig) =
            tokio::try_join!(
                self.repos
                    .insights
                    .get_review_quality_for_team(team_id, descendants, since),
                self.repos
                    .insights
                    .get_top_reviewers(team_id, descendants, since, 10, 5),
                self.repos
                    .insights
                    .get_significance_for_team(team_id, descendants, since),
                self.repos
                    .insights
                    .get_topic_categories_for_team(team_id, descendants, since),
                self.repos.insights.get_notable_contributions_for_team(
                    team_id,
                    descendants,
                    since,
                    5
                ),
                self.repos
                    .insights
                    .get_coverage_for_team(team_id, descendants, since),
                self.repos
                    .insights
                    .get_depth_by_significance_for_team(team_id, descendants, since),
            )
            .map_err(db_err)?;

        let (total, enriched, by_type) = coverage;

        // Fetch previous-period snapshot for trend deltas
        let trend =
            if let Ok((period_type, period_start)) = parse_period_snapshot_params(&req.period) {
                let prev = self
                    .repos
                    .insights
                    .get_previous_snapshot(team_id, period_start, &period_type)
                    .await
                    .ok()
                    .flatten();
                prev.map(|p| build_trend(&review_quality, &significance, &p))
            } else {
                None
            };

        Ok(Response::new(GetTeamInsightsResponse {
            insights: Some(TeamInsights {
                coverage: Some(coverage_to_proto(total, enriched, by_type)),
                review_quality: Some(review_quality_to_proto(review_quality, top_reviewers)),
                pr_significance: Some(significance_to_proto(significance)),
                discourse_topics: Some(topics_to_proto(topics)),
                notable_items: notable_to_proto(notable),
                depth_by_significance: Some(depth_by_sig_to_proto(depth_by_sig)),
                trend,
            }),
        }))
    }

    async fn get_person_insights(
        &self,
        request: Request<GetPersonInsightsRequest>,
    ) -> Result<Response<GetPersonInsightsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let person_id: Uuid = req
            .person_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid person_id"))?;
        let since = parse_period_since(&req.period)?;

        let (review_quality, reviews_received, significance, topics, notable, coverage) =
            tokio::try_join!(
                self.repos
                    .insights
                    .get_review_quality_for_person(person_id, since),
                self.repos
                    .insights
                    .get_reviews_received_for_person(person_id, since),
                self.repos
                    .insights
                    .get_significance_for_person(person_id, since),
                self.repos
                    .insights
                    .get_topic_categories_for_person(person_id, since),
                self.repos
                    .insights
                    .get_notable_contributions_for_person(person_id, since, 3),
                self.repos
                    .insights
                    .get_coverage_for_person(person_id, since),
            )
            .map_err(db_err)?;

        let (total, enriched, by_type) = coverage;
        let rq = &review_quality;
        let total_reviews = rq.total_reviews;
        let rubber_stamp_pct = if total_reviews > 0 {
            f64::from(rq.depth_1) / f64::from(total_reviews) * 100.0
        } else {
            0.0
        };

        Ok(Response::new(GetPersonInsightsResponse {
            insights: Some(PersonInsights {
                coverage: Some(coverage_to_proto(total, enriched, by_type)),
                reviewer_profile: Some(ReviewerProfile {
                    avg_depth: review_quality.avg_depth,
                    depth_distribution: vec![
                        review_quality.depth_1,
                        review_quality.depth_2,
                        review_quality.depth_3,
                        review_quality.depth_4,
                        review_quality.depth_5,
                    ],
                    total_reviews_given: total_reviews,
                    rubber_stamp_pct,
                    constructive_count: review_quality.constructive,
                    neutral_count: review_quality.neutral,
                    critical_count: review_quality.critical,
                }),
                reviews_received: Some(ReviewsReceivedSummary {
                    avg_depth_received: reviews_received.avg_depth_received,
                    total_reviews_received: reviews_received.total_reviews_received,
                    deep_review_pct: reviews_received.deep_review_pct,
                }),
                pr_impact: Some(significance_to_proto(significance)),
                discourse_topics: Some(topics_to_proto(topics)),
                highlights: notable_to_proto(notable),
            }),
        }))
    }

    async fn get_org_insights(
        &self,
        request: Request<GetOrgInsightsRequest>,
    ) -> Result<Response<GetOrgInsightsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let since = parse_period_since(&req.period)?;
        let team_id: Option<Uuid> = if req.team_id.is_empty() {
            None
        } else {
            Some(
                req.team_id
                    .parse()
                    .map_err(|_| Status::invalid_argument("invalid team_id"))?,
            )
        };

        // For org insights, we need a root team to scope queries.
        // If no team_id provided, find the first root team.
        let all_teams = self.repos.org.get_all_teams().await.map_err(db_err)?;

        let root_team_id = if let Some(tid) = team_id {
            tid
        } else {
            all_teams
                .iter()
                .find(|t| t.parent_team_id.is_none())
                .map(|t| t.id)
                .ok_or_else(|| Status::not_found("no teams found"))?
        };

        // Child teams of the selected root for comparison
        let child_ids: Vec<Uuid> = all_teams
            .iter()
            .filter(|t| t.parent_team_id == Some(root_team_id))
            .map(|t| t.id)
            .collect();

        let (
            review_quality,
            top_reviewers,
            significance,
            topics,
            notable,
            coverage,
            depth_by_sig,
            delivery,
            team_comparison,
        ) = tokio::try_join!(
            self.repos
                .insights
                .get_review_quality_for_team(root_team_id, true, since),
            self.repos
                .insights
                .get_top_reviewers(root_team_id, true, since, 10, 5),
            self.repos
                .insights
                .get_significance_for_team(root_team_id, true, since),
            self.repos
                .insights
                .get_topic_categories_for_team(root_team_id, true, since),
            self.repos
                .insights
                .get_notable_contributions_for_team(root_team_id, true, since, 5),
            self.repos
                .insights
                .get_coverage_for_team(root_team_id, true, since),
            self.repos
                .insights
                .get_depth_by_significance_for_team(root_team_id, true, since),
            self.repos
                .insights
                .get_delivery_summary(Some(root_team_id), since),
            async {
                if child_ids.is_empty() {
                    Ok(vec![])
                } else {
                    self.repos
                        .insights
                        .get_team_review_comparison(&child_ids, since)
                        .await
                }
            },
        )
        .map_err(db_err)?;

        let (total, enriched, by_type) = coverage;

        // Fetch previous-period snapshot for trend deltas
        let trend =
            if let Ok((period_type, period_start)) = parse_period_snapshot_params(&req.period) {
                let prev = self
                    .repos
                    .insights
                    .get_previous_snapshot(root_team_id, period_start, &period_type)
                    .await
                    .ok()
                    .flatten();
                prev.map(|p| build_trend(&review_quality, &significance, &p))
            } else {
                None
            };

        Ok(Response::new(GetOrgInsightsResponse {
            insights: Some(OrgInsights {
                coverage: Some(coverage_to_proto(total, enriched, by_type)),
                review_quality: Some(review_quality_to_proto(review_quality, top_reviewers)),
                team_comparison: team_comparison
                    .into_iter()
                    .map(|tc| TeamReviewComparison {
                        team_id: tc.team_id.to_string(),
                        team_name: tc.team_name,
                        review_count: tc.review_count,
                        avg_depth: tc.avg_depth,
                        rubber_stamp_pct: tc.rubber_stamp_pct,
                        constructive_count: tc.constructive,
                        neutral_count: tc.neutral,
                        critical_count: tc.critical,
                    })
                    .collect(),
                pr_significance: Some(significance_to_proto(significance)),
                discourse_topics: Some(topics_to_proto(topics)),
                org_highlights: notable_to_proto(notable),
                delivery: Some(OrgDeliverySummary {
                    total_prs_merged: delivery.total_prs_merged,
                    total_reviews: delivery.total_reviews,
                    total_jira_closed: delivery.total_jira_closed,
                    total_discourse_topics: delivery.total_discourse_topics,
                    total_discourse_posts: delivery.total_discourse_posts,
                    active_contributors: delivery.active_contributors,
                    active_teams: delivery.active_teams,
                }),
                depth_by_significance: Some(depth_by_sig_to_proto(depth_by_sig)),
                trend,
            }),
        }))
    }
}
