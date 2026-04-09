use std::collections::HashMap;

use futures::stream::TryStreamExt;
use ps_core::repo::metrics::{ListContributionsParams, ListPersonContributionsParams};
use ps_proto::canonical::prism::v1::metrics_service_server::MetricsService;
use ps_proto::canonical::prism::v1::{
    CategoryCount, CompareTeamsRequest, CompareTeamsResponse, Contribution,
    DiscourseActivityDataPoint, GetContributionRequest, GetContributionResponse,
    GetDiscourseActivityRequest, GetDiscourseActivityResponse, GetFlowMetricsRequest,
    GetFlowMetricsResponse, GetIndividualProfileRequest, GetIndividualProfileResponse,
    GetTeamMetricsRequest, GetTeamMetricsResponse, ListPeriodsRequest, ListPeriodsResponse,
    ListPersonContributionsRequest, ListPersonContributionsResponse, ListTeamContributionsRequest,
    ListTeamContributionsResponse, PeerComparison, Percentile, Period, PlatformActivitySummary,
    PlatformIdentityInfo, TeamMetrics, ThroughputDataPoint, TopContributor, WipDataPoint,
};
use time::OffsetDateTime;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::{
    MetricsServiceImpl, contribution_detail_to_proto, contribution_full_to_proto, format_date,
    parse_date, parse_period_type, period_type_to_proto, snapshot_to_proto,
};
use crate::common::{
    db_err, platform_to_proto, proto_to_contribution_state_str, proto_to_contribution_type_str,
    proto_to_platform_str, require_auth,
};

#[tonic::async_trait]
impl MetricsService for MetricsServiceImpl {
    async fn get_team_metrics(
        &self,
        request: Request<GetTeamMetricsRequest>,
    ) -> Result<Response<GetTeamMetricsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_id: Uuid = req
            .team_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;

        let period = req
            .period
            .ok_or_else(|| Status::invalid_argument("period required"))?;
        let period_type_val = parse_period_type(period.r#type)?;
        let period_start = parse_date(&period.start)?;
        let period_end = parse_date(&period.end)?;
        let today = OffsetDateTime::now_utc().date();
        let is_open = period_end >= today;

        // For open periods, always recompute to pick up fresh data.
        // For closed periods, use cached snapshot if available.
        let snapshot = self
            .repos
            .metrics
            .get_team_snapshot(team_id, period_start, period_type_val)
            .await
            .map_err(db_err)?;

        let needs_compute = snapshot.is_none() || is_open;

        let metrics = if needs_compute {
            if let Err(e) = ps_metrics::compute_team_snapshot(
                &self.repos,
                team_id,
                period_start,
                period_end,
                period_type_val,
            )
            .await
            {
                tracing::warn!(%team_id, error = %e, "failed to compute snapshot on-the-fly");
            }

            let snapshot = self
                .repos
                .metrics
                .get_team_snapshot(team_id, period_start, period_type_val)
                .await
                .map_err(db_err)?;

            if let Some(s) = snapshot {
                snapshot_to_proto(s)
            } else {
                // Team exists but has no data yet — return zeros
                let team = self
                    .repos
                    .org
                    .get_team(team_id)
                    .await
                    .map_err(db_err)?
                    .ok_or_else(|| Status::not_found("team not found"))?;

                TeamMetrics {
                    team_id: team.id.to_string(),
                    team_name: team.name,
                    period: Some(period),
                    ..TeamMetrics::default()
                }
            }
        } else if let Some(s) = snapshot {
            snapshot_to_proto(s)
        } else {
            unreachable!("needs_compute is false only when snapshot is Some")
        };

        Ok(Response::new(GetTeamMetricsResponse {
            metrics: Some(metrics),
        }))
    }

    async fn compare_teams(
        &self,
        request: Request<CompareTeamsRequest>,
    ) -> Result<Response<CompareTeamsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_ids: Vec<Uuid> = req
            .team_ids
            .iter()
            .map(|id| id.parse::<Uuid>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Status::invalid_argument("invalid team_id in list"))?;

        let period = req
            .period
            .ok_or_else(|| Status::invalid_argument("period required"))?;
        let period_type_val = parse_period_type(period.r#type)?;
        let period_start = parse_date(&period.start)?;
        let period_end = parse_date(&period.end)?;
        let today = OffsetDateTime::now_utc().date();
        let is_open = period_end >= today;

        // Compute missing/stale snapshots in parallel (capped at 4 concurrent).
        futures::stream::iter(team_ids.iter().map(Ok::<_, Status>))
            .try_for_each_concurrent(4, |&team_id| async move {
                let needs_compute = if is_open {
                    true
                } else {
                    self.repos
                        .metrics
                        .get_team_snapshot(team_id, period_start, period_type_val)
                        .await
                        .map_err(db_err)?
                        .is_none()
                };

                if needs_compute {
                    let _ = ps_metrics::compute_team_snapshot(
                        &self.repos,
                        team_id,
                        period_start,
                        period_end,
                        period_type_val,
                    )
                    .await;
                }
                Ok(())
            })
            .await?;

        let snapshots = self
            .repos
            .metrics
            .compare_team_snapshots(&team_ids, period_start, period_type_val)
            .await
            .map_err(db_err)?;

        // Include teams with no data as zeros
        let mut metrics: Vec<TeamMetrics> = snapshots.into_iter().map(snapshot_to_proto).collect();

        // Add zero entries for teams that had no snapshots
        let found_ids: std::collections::HashSet<String> =
            metrics.iter().map(|m| m.team_id.clone()).collect();

        for team_id in &team_ids {
            if !found_ids.contains(&team_id.to_string())
                && let Ok(Some(team)) = self.repos.org.get_team(*team_id).await
            {
                metrics.push(TeamMetrics {
                    team_id: team.id.to_string(),
                    team_name: team.name,
                    period: Some(period.clone()),
                    member_count: team.member_count,
                    ..TeamMetrics::default()
                });
            }
        }

        Ok(Response::new(CompareTeamsResponse { metrics }))
    }

    async fn list_periods(
        &self,
        request: Request<ListPeriodsRequest>,
    ) -> Result<Response<ListPeriodsResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let rows = self.repos.metrics.list_periods().await.map_err(db_err)?;

        let periods = rows
            .into_iter()
            .map(|r| Period {
                r#type: period_type_to_proto(r.period_type).into(),
                start: format_date(r.start),
                end: format_date(r.end),
            })
            .collect();

        Ok(Response::new(ListPeriodsResponse { periods }))
    }

    async fn list_team_contributions(
        &self,
        request: Request<ListTeamContributionsRequest>,
    ) -> Result<Response<ListTeamContributionsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_id: Uuid = req
            .team_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;

        let period = req
            .period
            .ok_or_else(|| Status::invalid_argument("period required"))?;
        let period_start = parse_date(&period.start)?;
        let period_end = parse_date(&period.end)?;

        let page_size = if req.page_size > 0 { req.page_size } else { 25 };
        let offset = req.page_index * page_size;

        let contribution_type_str = proto_to_contribution_type_str(req.contribution_type);
        let state_str = proto_to_contribution_state_str(req.state);
        let platform_str = proto_to_platform_str(req.platform, req.platform_instance.as_deref());
        let search = req.search.as_deref().filter(|s| !s.is_empty());
        let sort_field = req.sort_field.as_deref().filter(|s| !s.is_empty());
        let sort_desc = req.sort_desc.unwrap_or(true);

        let (rows, total_count) = self
            .repos
            .metrics
            .list_team_contributions(&ListContributionsParams {
                team_id,
                period_start,
                period_end,
                contribution_type: contribution_type_str.as_deref(),
                state: state_str.as_deref(),
                search,
                sort_field,
                sort_desc,
                page_size,
                offset,
                platform: platform_str.as_deref(),
            })
            .await
            .map_err(db_err)?;

        let contributions: Vec<Contribution> =
            rows.into_iter().map(contribution_detail_to_proto).collect();

        Ok(Response::new(ListTeamContributionsResponse {
            contributions,
            total_count: total_count as i32,
        }))
    }

    async fn get_contribution(
        &self,
        request: Request<GetContributionRequest>,
    ) -> Result<Response<GetContributionResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let contribution_id: Uuid = req
            .contribution_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid contribution_id"))?;

        let row = self
            .repos
            .metrics
            .get_contribution_by_id(contribution_id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("contribution not found"))?;

        let contribution = contribution_full_to_proto(row);

        Ok(Response::new(GetContributionResponse {
            contribution: Some(contribution),
        }))
    }

    async fn get_individual_profile(
        &self,
        request: Request<GetIndividualProfileRequest>,
    ) -> Result<Response<GetIndividualProfileResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let person_id: Uuid = req
            .person_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid person_id"))?;

        let period = req
            .period
            .ok_or_else(|| Status::invalid_argument("period required"))?;
        let period_start = parse_date(&period.start)?;
        let period_end = parse_date(&period.end)?;

        let person = self
            .repos
            .org
            .get_person(person_id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("person not found"))?;

        let identities = self
            .repos
            .org
            .get_identities_for_people(&[person_id])
            .await
            .map_err(db_err)?;

        let activity = self
            .repos
            .metrics
            .get_person_activity_summary(person_id, period_start, period_end)
            .await
            .map_err(db_err)?;

        let activity_by_platform: Vec<PlatformActivitySummary> = activity
            .into_iter()
            .map(|a| {
                let mut metrics = HashMap::new();
                metrics.insert(
                    "pull_request_count".to_string(),
                    f64::from(a.pull_request_count),
                );
                metrics.insert("pr_review_count".to_string(), f64::from(a.pr_review_count));
                if let Some(v) = a.avg_review_hours {
                    metrics.insert("avg_review_hours".to_string(), v);
                }
                if let Some(v) = a.avg_cycle_time_hours {
                    metrics.insert("avg_cycle_time_hours".to_string(), v);
                }
                let (platform, platform_instance) = platform_to_proto(&a.platform);
                PlatformActivitySummary {
                    platform,
                    platform_instance,
                    contribution_count: a.contribution_count,
                    metrics,
                }
            })
            .collect();

        // Compute peer context if the person has a level set
        let peer_context = if let Some(ref level) = person.level {
            let since = period_start.midnight().assume_utc();
            let (throughput_result, enrichment_result) = tokio::join!(
                self.repos.metrics.compute_peer_percentiles(
                    person_id,
                    level,
                    period_start,
                    period_end
                ),
                self.repos
                    .insights
                    .compute_enrichment_peer_percentiles(person_id, level, since),
            );

            let mut metrics = HashMap::new();
            let mut max_peer_count = 0i32;

            if let Ok(Some((count, percentile, peer_count))) = throughput_result {
                metrics.insert(
                    "throughput".to_string(),
                    Percentile {
                        #[allow(clippy::cast_precision_loss)]
                        value: count as f64,
                        percentile,
                    },
                );
                max_peer_count = max_peer_count.max(peer_count);
            }

            if let Ok(enrichment_percentiles) = enrichment_result {
                for ep in enrichment_percentiles {
                    max_peer_count = max_peer_count.max(ep.peer_count);
                    metrics.insert(
                        ep.metric_name,
                        Percentile {
                            value: ep.value,
                            percentile: ep.percentile,
                        },
                    );
                }
            }

            if metrics.is_empty() {
                None
            } else {
                Some(PeerComparison {
                    level: level.clone(),
                    peer_count: max_peer_count,
                    metrics,
                })
            }
        } else {
            None
        };

        Ok(Response::new(GetIndividualProfileResponse {
            person_id: person.id.to_string(),
            name: person.name,
            team_name: person.team_name.unwrap_or_default(),
            level: person.level.unwrap_or_default(),
            identities: identities
                .into_iter()
                .map(|i| {
                    let (platform, platform_instance) = platform_to_proto(&i.platform);
                    PlatformIdentityInfo {
                        platform,
                        username: i.platform_username,
                        platform_instance,
                    }
                })
                .collect(),
            activity_by_platform,
            peer_context,
        }))
    }

    async fn list_person_contributions(
        &self,
        request: Request<ListPersonContributionsRequest>,
    ) -> Result<Response<ListPersonContributionsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let person_id: Uuid = req
            .person_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid person_id"))?;

        let page_size = if req.page_size > 0 { req.page_size } else { 25 };
        let offset = req.page_index * page_size;

        let platform_str = proto_to_platform_str(req.platform, req.platform_instance.as_deref());
        let contribution_type_str = proto_to_contribution_type_str(req.contribution_type);
        let since = req
            .since
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(parse_date)
            .transpose()?;
        let until = req
            .until
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(parse_date)
            .transpose()?;
        let sort_field = req.sort_field.as_deref().filter(|s| !s.is_empty());
        let sort_desc = req.sort_desc.unwrap_or(true);
        let state_str = proto_to_contribution_state_str(req.state);
        let search = req.search.as_deref().filter(|s| !s.is_empty());

        let (rows, total_count) = self
            .repos
            .metrics
            .list_person_contributions(&ListPersonContributionsParams {
                person_id,
                platform: platform_str.as_deref(),
                contribution_type: contribution_type_str.as_deref(),
                since,
                until,
                sort_field,
                sort_desc,
                page_size,
                offset,
                state: state_str.as_deref(),
                search,
            })
            .await
            .map_err(db_err)?;

        let contributions: Vec<Contribution> =
            rows.into_iter().map(contribution_detail_to_proto).collect();

        Ok(Response::new(ListPersonContributionsResponse {
            contributions,
            total_count: total_count as i32,
        }))
    }

    async fn get_discourse_activity(
        &self,
        request: Request<GetDiscourseActivityRequest>,
    ) -> Result<Response<GetDiscourseActivityResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_id: Uuid = req
            .team_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;

        let period = req
            .period
            .ok_or_else(|| Status::invalid_argument("period required"))?;
        let period_start = parse_date(&period.start)?;
        let period_end = parse_date(&period.end)?;

        let instance = req.instance.map(|i| {
            if i.starts_with("discourse") {
                i
            } else {
                format!("discourse-{i}")
            }
        });
        let instance = instance.as_deref();

        let (categories, trend, contributors) = tokio::try_join!(
            self.repos.metrics.get_discourse_category_distribution(
                team_id,
                period_start,
                period_end,
                instance,
            ),
            self.repos.metrics.get_discourse_activity_trend(
                team_id,
                period_start,
                period_end,
                instance
            ),
            self.repos.metrics.get_discourse_top_contributors(
                team_id,
                period_start,
                period_end,
                instance
            ),
        )
        .map_err(db_err)?;

        Ok(Response::new(GetDiscourseActivityResponse {
            category_distribution: categories
                .into_iter()
                .map(|c| CategoryCount {
                    category: c.category,
                    topics: c.topic_count,
                    posts: c.post_count,
                })
                .collect(),
            activity_trend: trend
                .into_iter()
                .map(|t| DiscourseActivityDataPoint {
                    date: format_date(t.date),
                    topics: t.topics,
                    posts: t.posts,
                    likes: t.likes,
                    instance: String::new(),
                })
                .collect(),
            top_contributors: contributors
                .into_iter()
                .map(|c| TopContributor {
                    person_id: c.person_id.to_string(),
                    name: c.name,
                    topics: c.topics,
                    posts: c.posts,
                    likes_received: c.likes_received,
                    solved: 0,
                })
                .collect(),
        }))
    }

    async fn get_flow_metrics(
        &self,
        request: Request<GetFlowMetricsRequest>,
    ) -> Result<Response<GetFlowMetricsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_id: Uuid = req
            .team_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;

        let period = req
            .period
            .ok_or_else(|| Status::invalid_argument("period required"))?;
        let period_type_val = parse_period_type(period.r#type)?;
        let period_start = parse_date(&period.start)?;
        let period_end = parse_date(&period.end)?;
        let today = OffsetDateTime::now_utc().date();
        let is_open = period_end >= today;

        // Recompute if open period or no snapshot exists
        let snapshot = self
            .repos
            .metrics
            .get_team_snapshot(team_id, period_start, period_type_val)
            .await
            .map_err(db_err)?;

        if snapshot.is_none() || is_open {
            let _ = ps_metrics::compute_team_snapshot(
                &self.repos,
                team_id,
                period_start,
                period_end,
                period_type_val,
            )
            .await;
        }

        let current = self
            .repos
            .metrics
            .get_team_snapshot(team_id, period_start, period_type_val)
            .await
            .map_err(db_err)?;

        // Fetch historical snapshots for trend data.
        // Use sub-period granularity: month → weekly bars, quarter → monthly bars.
        // Week has no sub-period so we show weekly history for context.
        let (trend_period_type, history_limit) = match period_type_val {
            ps_core::models::PeriodType::Week | ps_core::models::PeriodType::Month => {
                (ps_core::models::PeriodType::Week, 8)
            }
            ps_core::models::PeriodType::Quarter => (ps_core::models::PeriodType::Month, 6),
        };

        let history = self
            .repos
            .metrics
            .get_snapshot_history(team_id, trend_period_type, history_limit)
            .await
            .map_err(db_err)?;

        let throughput_trend: Vec<ThroughputDataPoint> = history
            .iter()
            .rev() // oldest first
            .map(|s| {
                // Parse per-source breakdown from raw_metrics JSON
                let by_source = s
                    .raw_metrics
                    .get("throughput_by_source")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_i64().map(|n| (k.clone(), n as i32)))
                            .collect()
                    })
                    .unwrap_or_default();

                ThroughputDataPoint {
                    date: format_date(s.period_start),
                    count: s.throughput.unwrap_or(0),
                    source: String::new(),
                    by_source,
                }
            })
            .collect();

        let wip_trend: Vec<WipDataPoint> = history
            .iter()
            .rev()
            .map(|s| WipDataPoint {
                date: format_date(s.period_start),
                wip: f64::from(s.wip_avg.unwrap_or(0.0)),
            })
            .collect();

        let response = if let Some(s) = current {
            GetFlowMetricsResponse {
                avg_cycle_time_hours: f64::from(s.avg_cycle_time_hours.unwrap_or(0.0)),
                wip_average: f64::from(s.wip_avg.unwrap_or(0.0)),
                throughput: s.throughput.unwrap_or(0),
                flow_efficiency: f64::from(s.flow_efficiency.unwrap_or(0.0)),
                lead_time_hours: f64::from(s.lead_time_hours.unwrap_or(0.0)),
                throughput_trend,
                wip_trend,
            }
        } else {
            GetFlowMetricsResponse {
                avg_cycle_time_hours: 0.0,
                wip_average: 0.0,
                throughput: 0,
                flow_efficiency: 0.0,
                lead_time_hours: 0.0,
                throughput_trend,
                wip_trend,
            }
        };

        Ok(Response::new(response))
    }
}
