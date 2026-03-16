use crate::Error;
use crate::models::{ContributionState, ContributionType, PeriodType};
use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

/// Repository for the `metrics` schema: pre-computed team snapshots.
#[derive(Clone)]
pub struct MetricsRepo {
    pool: PgPool,
}

/// A row from `metrics.team_snapshots` joined with team name and member count.
pub struct TeamSnapshotRow {
    pub id: Uuid,
    pub team_id: Uuid,
    pub team_name: String,
    pub member_count: i32,
    pub period_start: Date,
    pub period_end: Date,
    pub period_type: PeriodType,
    pub throughput: Option<i32>,
    pub avg_review_turnaround_hours: Option<f32>,
    pub avg_cycle_time_hours: Option<f32>,
    pub wip_avg: Option<f32>,
    pub flow_efficiency: Option<f32>,
    pub lead_time_hours: Option<f32>,
    pub raw_metrics: serde_json::Value,
}

/// Raw contribution data needed for metrics computation.
pub struct ContributionMetricRow {
    pub person_id: Option<Uuid>,
    pub platform_id: String,
    pub contribution_type: ContributionType,
    pub state: Option<ContributionState>,
    pub created_at: time::OffsetDateTime,
    pub closed_at: Option<time::OffsetDateTime>,
    pub metrics: serde_json::Value,
    pub metadata: serde_json::Value,
}

impl MetricsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get a team snapshot for a specific period.
    pub async fn get_team_snapshot(
        &self,
        team_id: Uuid,
        period_start: Date,
        period_type: PeriodType,
    ) -> Result<Option<TeamSnapshotRow>, Error> {
        let period_type_str = period_type.as_str();
        let row = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT ch.id FROM org.teams ch
                JOIN team_tree tt ON ch.parent_team_id = tt.id
            )
            SELECT ts.id, ts.team_id, t.name AS team_name,
                   (SELECT COUNT(DISTINCT tm.person_id)::int
                    FROM org.team_memberships tm
                    JOIN team_tree tt ON tm.team_id = tt.id
                    WHERE tm.end_date IS NULL OR tm.end_date > CURRENT_DATE) AS "member_count!",
                   ts.period_start, ts.period_end, ts.period_type,
                   ts.throughput, ts.avg_review_turnaround_hours,
                   ts.avg_cycle_time_hours, ts.wip_avg,
                   ts.flow_efficiency, ts.lead_time_hours,
                   ts.raw_metrics AS "raw_metrics!"
            FROM metrics.team_snapshots ts
            JOIN org.teams t ON t.id = ts.team_id
            WHERE ts.team_id = $1
              AND ts.period_start = $2
              AND ts.period_type = $3
            "#,
            team_id,
            period_start,
            period_type_str,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row.map(|r| TeamSnapshotRow {
            id: r.id,
            team_id: r.team_id,
            team_name: r.team_name,
            member_count: r.member_count,
            period_start: r.period_start,
            period_end: r.period_end,
            period_type: PeriodType::from_str_opt(&r.period_type).unwrap_or(period_type),
            throughput: r.throughput,
            avg_review_turnaround_hours: r.avg_review_turnaround_hours,
            avg_cycle_time_hours: r.avg_cycle_time_hours,
            wip_avg: r.wip_avg,
            flow_efficiency: r.flow_efficiency,
            lead_time_hours: r.lead_time_hours,
            raw_metrics: r.raw_metrics,
        }))
    }

    /// Get snapshots for multiple teams for a specific period.
    pub async fn compare_team_snapshots(
        &self,
        team_ids: &[Uuid],
        period_start: Date,
        period_type: PeriodType,
    ) -> Result<Vec<TeamSnapshotRow>, Error> {
        let period_type_str = period_type.as_str();
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id, id AS root_id FROM org.teams WHERE id = ANY($1)
                UNION ALL
                SELECT ch.id, tt.root_id
                FROM org.teams ch
                JOIN team_tree tt ON ch.parent_team_id = tt.id
            ),
            member_counts AS (
                SELECT tt.root_id AS team_id,
                       COUNT(DISTINCT tm.person_id)::int AS count
                FROM team_tree tt
                JOIN org.team_memberships tm ON tm.team_id = tt.id
                WHERE tm.end_date IS NULL OR tm.end_date > CURRENT_DATE
                GROUP BY tt.root_id
            )
            SELECT ts.id, ts.team_id, t.name AS team_name,
                   COALESCE(mc.count, 0) AS "member_count!",
                   ts.period_start, ts.period_end, ts.period_type,
                   ts.throughput, ts.avg_review_turnaround_hours,
                   ts.avg_cycle_time_hours, ts.wip_avg,
                   ts.flow_efficiency, ts.lead_time_hours,
                   ts.raw_metrics AS "raw_metrics!"
            FROM metrics.team_snapshots ts
            JOIN org.teams t ON t.id = ts.team_id
            LEFT JOIN member_counts mc ON mc.team_id = ts.team_id
            WHERE ts.team_id = ANY($1)
              AND ts.period_start = $2
              AND ts.period_type = $3
            ORDER BY t.name
            "#,
            team_ids,
            period_start,
            period_type_str,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| TeamSnapshotRow {
                id: r.id,
                team_id: r.team_id,
                team_name: r.team_name,
                member_count: r.member_count,
                period_start: r.period_start,
                period_end: r.period_end,
                period_type: PeriodType::from_str_opt(&r.period_type).unwrap_or(period_type),
                throughput: r.throughput,
                avg_review_turnaround_hours: r.avg_review_turnaround_hours,
                avg_cycle_time_hours: r.avg_cycle_time_hours,
                wip_avg: r.wip_avg,
                flow_efficiency: r.flow_efficiency,
                lead_time_hours: r.lead_time_hours,
                raw_metrics: r.raw_metrics,
            })
            .collect())
    }

    /// List periods that have snapshot data.
    pub async fn list_periods(&self) -> Result<Vec<PeriodRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT period_start, period_end, period_type
            FROM metrics.team_snapshots
            ORDER BY period_type, period_start DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                Some(PeriodRow {
                    start: r.period_start,
                    end: r.period_end,
                    period_type: PeriodType::from_str_opt(&r.period_type)?,
                })
            })
            .collect())
    }

    /// Upsert a team snapshot (used by metrics computation).
    pub async fn upsert_snapshot(&self, snap: &SnapshotInput) -> Result<(), Error> {
        let id = snap.id;
        let team_id = snap.team_id;
        let period_start = snap.period_start;
        let period_end = snap.period_end;
        let period_type = snap.period_type.as_str();
        let throughput = snap.throughput;
        let avg_review_turnaround_hours = snap.avg_review_turnaround_hours;
        let avg_cycle_time_hours = snap.avg_cycle_time_hours;
        let wip_avg = snap.wip_avg;
        let flow_efficiency = snap.flow_efficiency;
        let lead_time_hours = snap.lead_time_hours;
        let raw_metrics = &snap.raw_metrics;
        sqlx::query!(
            r#"
            INSERT INTO metrics.team_snapshots (
                id, team_id, period_start, period_end, period_type,
                throughput, avg_review_turnaround_hours,
                avg_cycle_time_hours, wip_avg, flow_efficiency, lead_time_hours,
                raw_metrics, computed_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now())
            ON CONFLICT (team_id, period_start, period_type)
            DO UPDATE SET
                period_end = EXCLUDED.period_end,
                throughput = EXCLUDED.throughput,
                avg_review_turnaround_hours = EXCLUDED.avg_review_turnaround_hours,
                avg_cycle_time_hours = EXCLUDED.avg_cycle_time_hours,
                wip_avg = EXCLUDED.wip_avg,
                flow_efficiency = EXCLUDED.flow_efficiency,
                lead_time_hours = EXCLUDED.lead_time_hours,
                raw_metrics = EXCLUDED.raw_metrics,
                computed_at = now()
            "#,
            id,
            team_id,
            period_start,
            period_end,
            period_type,
            throughput,
            avg_review_turnaround_hours,
            avg_cycle_time_hours,
            wip_avg,
            flow_efficiency,
            lead_time_hours,
            raw_metrics,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Get contributions for a team's members within a date range.
    ///
    /// Uses a recursive CTE to include members of all descendant teams so that
    /// parent/group-level metrics aggregate correctly.
    pub async fn get_team_contributions(
        &self,
        team_id: Uuid,
        period_start: Date,
        period_end: Date,
    ) -> Result<Vec<ContributionMetricRow>, Error> {
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
            )
            SELECT DISTINCT c.person_id, c.platform_id, c.contribution_type, c.state,
                   c.created_at, c.closed_at,
                   c.metrics, c.metadata
            FROM activity.contributions c
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
            JOIN team_tree tt ON tm.team_id = tt.id
            WHERE (tm.end_date IS NULL OR tm.end_date > $3::date)
              AND tm.start_date <= $3::date
              AND c.created_at >= $2::date::timestamptz
              AND c.created_at < ($3::date + INTERVAL '1 day')::timestamptz
            "#,
            team_id,
            period_start,
            period_end,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                Some(ContributionMetricRow {
                    person_id: r.person_id,
                    platform_id: r.platform_id,
                    contribution_type: match r.contribution_type.as_str() {
                        "pull_request" => ContributionType::PullRequest,
                        "pr_review" => ContributionType::PrReview,
                        _ => return None,
                    },
                    state: r.state.as_deref().and_then(ContributionState::from_str_opt),
                    created_at: r.created_at,
                    closed_at: r.closed_at,
                    metrics: r.metrics,
                    metadata: r.metadata,
                })
            })
            .collect())
    }
}

/// Parameters for listing team contributions with filtering and pagination.
pub struct ListContributionsParams<'a> {
    pub team_id: Uuid,
    pub period_start: Date,
    pub period_end: Date,
    pub contribution_type: Option<&'a str>,
    pub state: Option<&'a str>,
    pub search: Option<&'a str>,
    pub sort_field: Option<&'a str>,
    pub sort_desc: bool,
    pub page_size: i32,
    pub offset: i32,
}

/// A detailed contribution row for drill-down display.
pub struct ContributionDetailRow {
    pub id: Uuid,
    pub person_name: String,
    pub platform: crate::models::Platform,
    pub contribution_type: crate::models::ContributionType,
    pub platform_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub state: Option<crate::models::ContributionState>,
    pub created_at: time::OffsetDateTime,
    pub closed_at: Option<time::OffsetDateTime>,
    pub metrics: serde_json::Value,
    pub metadata: serde_json::Value,
    pub total_count: i64,
}

impl MetricsRepo {
    /// List individual contributions for a team's members, with filtering and pagination.
    ///
    /// Uses the same recursive team-tree CTE as `get_team_contributions` but returns
    /// full contribution details for drill-down display.
    pub async fn list_team_contributions(
        &self,
        params: &ListContributionsParams<'_>,
    ) -> Result<(Vec<ContributionDetailRow>, i64), Error> {
        let team_id = params.team_id;
        let period_start = params.period_start;
        let period_end = params.period_end;
        let contribution_type = params.contribution_type;
        let state = params.state;
        let sort_field = params.sort_field;
        let sort_desc = params.sort_desc;
        let page_size = params.page_size;
        let offset = params.offset;
        let escaped_search = params.search.map(super::escape_like);
        let search = escaped_search.as_deref();
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
            )
            SELECT c.id, p.name AS person_name, c.platform, c.contribution_type,
                   c.platform_id, c.title, c.url, c.state,
                   c.created_at, c.closed_at,
                   c.metrics, c.metadata,
                   COUNT(*) OVER() AS "total_count!"
            FROM activity.contributions c
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
                AND tm.team_id IN (SELECT id FROM team_tree)
                AND (tm.end_date IS NULL OR tm.end_date > $3::date)
                AND tm.start_date <= $3::date
            JOIN org.people p ON p.id = c.person_id
            WHERE c.created_at >= $2::date::timestamptz
              AND c.created_at < ($3::date + INTERVAL '1 day')::timestamptz
              AND ($4::text IS NULL OR c.contribution_type = $4)
              AND ($5::text IS NULL OR c.state = $5)
              AND ($8::text IS NULL OR (
                  c.title ILIKE '%' || $8 || '%'
                  OR p.name ILIKE '%' || $8 || '%'
                  OR c.metadata->>'repo' ILIKE '%' || $8 || '%'
              ))
            ORDER BY
              CASE WHEN $9 = 'person_name' AND NOT $10 THEN p.name END ASC NULLS LAST,
              CASE WHEN $9 = 'person_name' AND $10 THEN p.name END DESC NULLS LAST,
              CASE WHEN $9 = 'state' AND NOT $10 THEN c.state END ASC NULLS LAST,
              CASE WHEN $9 = 'state' AND $10 THEN c.state END DESC NULLS LAST,
              CASE WHEN $9 = 'repo' AND NOT $10 THEN c.metadata->>'repo' END ASC NULLS LAST,
              CASE WHEN $9 = 'repo' AND $10 THEN c.metadata->>'repo' END DESC NULLS LAST,
              CASE WHEN COALESCE($9, 'created_at') = 'created_at' AND NOT $10 THEN c.created_at END ASC,
              CASE WHEN COALESCE($9, 'created_at') = 'created_at' AND $10 THEN c.created_at END DESC
            LIMIT $6 OFFSET $7
            "#,
            team_id,
            period_start,
            period_end,
            contribution_type,
            state,
            page_size as i64,
            offset as i64,
            search,
            sort_field,
            sort_desc,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let total_count = rows.first().map_or(0, |r| r.total_count);

        Ok((
            rows.into_iter()
                .map(|r| ContributionDetailRow {
                    id: r.id,
                    person_name: r.person_name,
                    platform: r
                        .platform
                        .parse()
                        .unwrap_or(crate::models::Platform::Github),
                    contribution_type: r
                        .contribution_type
                        .parse()
                        .unwrap_or(crate::models::ContributionType::PullRequest),
                    platform_id: r.platform_id,
                    title: r.title,
                    url: r.url,
                    state: r.state.and_then(|s| s.parse().ok()),
                    created_at: r.created_at,
                    closed_at: r.closed_at,
                    metrics: r.metrics,
                    metadata: r.metadata,
                    total_count,
                })
                .collect(),
            total_count,
        ))
    }
}

/// A distinct period that has snapshot data.
pub struct PeriodRow {
    pub start: Date,
    pub end: Date,
    pub period_type: PeriodType,
}

/// Input for upserting a team snapshot.
pub struct SnapshotInput {
    pub id: Uuid,
    pub team_id: Uuid,
    pub period_start: Date,
    pub period_end: Date,
    pub period_type: PeriodType,
    pub throughput: i32,
    pub avg_review_turnaround_hours: Option<f32>,
    pub avg_cycle_time_hours: Option<f32>,
    pub wip_avg: Option<f32>,
    pub flow_efficiency: Option<f32>,
    pub lead_time_hours: Option<f32>,
    pub raw_metrics: serde_json::Value,
}
