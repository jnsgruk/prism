use std::collections::HashMap;

use futures::stream::TryStreamExt;
use ps_core::repo::Repos;
use ps_core::repo::metrics::ListContributionsParams;
use ps_proto::prism::v1::metrics_service_server::MetricsService;
use ps_proto::prism::v1::{
    CompareTeamsRequest, CompareTeamsResponse, Contribution, GetFlowMetricsRequest,
    GetFlowMetricsResponse, GetIndividualProfileRequest, GetIndividualProfileResponse,
    GetTeamMetricsRequest, GetTeamMetricsResponse, ListPeriodsRequest, ListPeriodsResponse,
    ListPersonContributionsRequest, ListPersonContributionsResponse, ListTeamContributionsRequest,
    ListTeamContributionsResponse, Period, PeriodType, TeamMetrics,
};
use time::macros::format_description;
use time::{Date, OffsetDateTime};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::common::{db_err, require_auth, to_timestamp};

/// ISO 8601 date-only format (YYYY-MM-DD).
const DATE_FMT: &[time::format_description::BorrowedFormatItem<'_>] =
    format_description!("[year]-[month]-[day]");

pub struct MetricsServiceImpl {
    repos: Repos,
}

impl MetricsServiceImpl {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }
}

#[allow(clippy::result_large_err)]
fn parse_period_type(pt: i32) -> Result<ps_core::models::PeriodType, Status> {
    match PeriodType::try_from(pt) {
        Ok(PeriodType::Week) => Ok(ps_core::models::PeriodType::Week),
        Ok(PeriodType::Month) => Ok(ps_core::models::PeriodType::Month),
        Ok(PeriodType::Quarter) => Ok(ps_core::models::PeriodType::Quarter),
        _ => Err(Status::invalid_argument("invalid period_type")),
    }
}

fn period_type_to_proto(pt: ps_core::models::PeriodType) -> PeriodType {
    match pt {
        ps_core::models::PeriodType::Week => PeriodType::Week,
        ps_core::models::PeriodType::Month => PeriodType::Month,
        ps_core::models::PeriodType::Quarter => PeriodType::Quarter,
    }
}

#[allow(clippy::result_large_err)]
fn parse_date(s: &str) -> Result<Date, Status> {
    Date::parse(s, DATE_FMT).map_err(|_| Status::invalid_argument(format!("invalid date: {s}")))
}

fn format_date(d: Date) -> String {
    d.format(DATE_FMT).unwrap_or_else(|_| d.to_string())
}

fn json_f32(v: &serde_json::Value, key: &str) -> f32 {
    v.get(key)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0) as f32
}

fn json_i32(v: &serde_json::Value, key: &str) -> i32 {
    v.get(key).and_then(serde_json::Value::as_i64).unwrap_or(0) as i32
}

fn snapshot_to_proto(s: ps_core::repo::metrics::TeamSnapshotRow) -> TeamMetrics {
    TeamMetrics {
        team_id: s.team_id.to_string(),
        team_name: s.team_name,
        period: Some(Period {
            r#type: period_type_to_proto(s.period_type).into(),
            start: format_date(s.period_start),
            end: format_date(s.period_end),
        }),
        throughput: s.throughput.unwrap_or(0),
        avg_review_turnaround_hours: s.avg_review_turnaround_hours.unwrap_or(0.0),
        member_count: s.member_count,
        review_turnaround_p75_hours: json_f32(&s.raw_metrics, "review_turnaround_p75_hours"),
        review_turnaround_p90_hours: json_f32(&s.raw_metrics, "review_turnaround_p90_hours"),
        review_turnaround_p99_hours: json_f32(&s.raw_metrics, "review_turnaround_p99_hours"),
        raw_metrics: HashMap::default(),
        avg_cycle_time_hours: s.avg_cycle_time_hours.unwrap_or(0.0),
        wip_avg: s.wip_avg.unwrap_or(0.0),
        flow_efficiency: s.flow_efficiency.unwrap_or(0.0),
        lead_time_hours: s.lead_time_hours.unwrap_or(0.0),
        source_platforms: s.source_platforms.clone(),
    }
}

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
                    throughput: 0,
                    avg_review_turnaround_hours: 0.0,
                    member_count: team.member_count,
                    review_turnaround_p75_hours: 0.0,
                    review_turnaround_p90_hours: 0.0,
                    review_turnaround_p99_hours: 0.0,
                    raw_metrics: HashMap::default(),
                    avg_cycle_time_hours: 0.0,
                    wip_avg: 0.0,
                    flow_efficiency: 0.0,
                    lead_time_hours: 0.0,
                    source_platforms: Vec::new(),
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
                    throughput: 0,
                    avg_review_turnaround_hours: 0.0,
                    member_count: team.member_count,
                    review_turnaround_p75_hours: 0.0,
                    review_turnaround_p90_hours: 0.0,
                    review_turnaround_p99_hours: 0.0,
                    raw_metrics: HashMap::default(),
                    avg_cycle_time_hours: 0.0,
                    wip_avg: 0.0,
                    flow_efficiency: 0.0,
                    lead_time_hours: 0.0,
                    source_platforms: Vec::new(),
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

        let contribution_type = req.contribution_type.as_deref();
        let state = req.state.as_deref();
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
                contribution_type,
                state,
                search,
                sort_field,
                sort_desc,
                page_size,
                offset,
            })
            .await
            .map_err(db_err)?;

        let contributions = rows
            .into_iter()
            .map(|r| {
                let repo = if let Some(s) = r.metadata.get("repo").and_then(|v| v.as_str()) {
                    s.to_string()
                } else {
                    // Fallback: extract "owner/repo" from platform_id (e.g. "owner/repo/pull/123")
                    match r.platform_id.splitn(3, '/').collect::<Vec<_>>().as_slice() {
                        [owner, repo, ..] => format!("{owner}/{repo}"),
                        _ => String::new(),
                    }
                };
                Contribution {
                    id: r.id.to_string(),
                    person_name: r.person_name,
                    platform: r.platform.to_string(),
                    contribution_type: r.contribution_type.to_string(),
                    platform_id: r.platform_id,
                    title: r.title.unwrap_or_default(),
                    url: r.url.unwrap_or_default(),
                    state: r.state.map(|s| s.to_string()).unwrap_or_default(),
                    created_at: Some(to_timestamp(r.created_at)),
                    closed_at: r.closed_at.map(to_timestamp),
                    additions: json_i32(&r.metrics, "additions"),
                    deletions: json_i32(&r.metrics, "deletions"),
                    changed_files: json_i32(&r.metrics, "changed_files"),
                    review_count: json_i32(&r.metrics, "review_count"),
                    review_hours: json_f32(&r.metrics, "review_hours"),
                    repo,
                }
            })
            .collect();

        Ok(Response::new(ListTeamContributionsResponse {
            contributions,
            total_count: total_count as i32,
        }))
    }

    async fn get_individual_profile(
        &self,
        request: Request<GetIndividualProfileRequest>,
    ) -> Result<Response<GetIndividualProfileResponse>, Status> {
        let _ctx = require_auth(&request)?;
        Err(Status::unimplemented(
            "GetIndividualProfile not yet implemented",
        ))
    }

    async fn list_person_contributions(
        &self,
        request: Request<ListPersonContributionsRequest>,
    ) -> Result<Response<ListPersonContributionsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        Err(Status::unimplemented(
            "ListPersonContributions not yet implemented",
        ))
    }

    async fn get_flow_metrics(
        &self,
        request: Request<GetFlowMetricsRequest>,
    ) -> Result<Response<GetFlowMetricsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        Err(Status::unimplemented("GetFlowMetrics not yet implemented"))
    }
}
