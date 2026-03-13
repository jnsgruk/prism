use std::collections::HashMap;

use ps_core::repo::Repos;
use ps_proto::prism::v1::metrics_service_server::MetricsService;
use ps_proto::prism::v1::{
    CompareTeamsRequest, CompareTeamsResponse, GetTeamMetricsRequest, GetTeamMetricsResponse,
    ListPeriodsRequest, ListPeriodsResponse, Period, PeriodType, TeamMetrics,
};
use time::Date;
use time::macros::format_description;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::common::{db_err, require_auth};

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
fn parse_period_type(pt: i32) -> Result<&'static str, Status> {
    match PeriodType::try_from(pt) {
        Ok(PeriodType::Week) => Ok("week"),
        Ok(PeriodType::Month) => Ok("month"),
        Ok(PeriodType::Quarter) => Ok("quarter"),
        _ => Err(Status::invalid_argument("invalid period_type")),
    }
}

fn period_type_to_proto(pt: &str) -> PeriodType {
    match pt {
        "week" => PeriodType::Week,
        "month" => PeriodType::Month,
        "quarter" => PeriodType::Quarter,
        _ => PeriodType::Unspecified,
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

fn snapshot_to_proto(s: ps_core::repo::metrics::TeamSnapshotRow) -> TeamMetrics {
    TeamMetrics {
        team_id: s.team_id.to_string(),
        team_name: s.team_name,
        period: Some(Period {
            r#type: period_type_to_proto(&s.period_type).into(),
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
        let period_type_str = parse_period_type(period.r#type)?;
        let period_start = parse_date(&period.start)?;

        // Try to get existing snapshot
        let snapshot = self
            .repos
            .metrics
            .get_team_snapshot(team_id, period_start, period_type_str)
            .await
            .map_err(db_err)?;

        let metrics = if let Some(s) = snapshot {
            snapshot_to_proto(s)
        } else {
            // No snapshot exists — compute on the fly
            let period_end = parse_date(&period.end)?;
            if let Err(e) = ps_metrics::compute_team_snapshot(
                &self.repos,
                team_id,
                period_start,
                period_end,
                period_type_str,
            )
            .await
            {
                tracing::warn!(%team_id, error = %e, "failed to compute snapshot on-the-fly");
            }

            // Try again after computation
            let snapshot = self
                .repos
                .metrics
                .get_team_snapshot(team_id, period_start, period_type_str)
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
                }
            }
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
        let period_type_str = parse_period_type(period.r#type)?;
        let period_start = parse_date(&period.start)?;
        let period_end = parse_date(&period.end)?;

        // Compute any missing snapshots on the fly
        for &team_id in &team_ids {
            let existing = self
                .repos
                .metrics
                .get_team_snapshot(team_id, period_start, period_type_str)
                .await
                .map_err(db_err)?;

            if existing.is_none() {
                let _ = ps_metrics::compute_team_snapshot(
                    &self.repos,
                    team_id,
                    period_start,
                    period_end,
                    period_type_str,
                )
                .await;
            }
        }

        let snapshots = self
            .repos
            .metrics
            .compare_team_snapshots(&team_ids, period_start, period_type_str)
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
                r#type: period_type_to_proto(&r.period_type).into(),
                start: format_date(r.start),
                end: format_date(r.end),
            })
            .collect();

        Ok(Response::new(ListPeriodsResponse { periods }))
    }
}
