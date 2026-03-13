use crate::Error;
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
    pub period_type: String,
    pub throughput: Option<i32>,
    pub avg_review_turnaround_hours: Option<f32>,
}

/// Raw contribution data needed for metrics computation.
pub struct ContributionMetricRow {
    pub person_id: Option<Uuid>,
    pub contribution_type: String,
    pub state: Option<String>,
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
        period_type: &str,
    ) -> Result<Option<TeamSnapshotRow>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT ts.id, ts.team_id, t.name AS team_name,
                   (SELECT COUNT(*)::int FROM org.team_memberships tm
                    WHERE tm.team_id = t.id
                      AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)) AS "member_count!",
                   ts.period_start, ts.period_end, ts.period_type,
                   ts.throughput, ts.avg_review_turnaround_hours
            FROM metrics.team_snapshots ts
            JOIN org.teams t ON t.id = ts.team_id
            WHERE ts.team_id = $1
              AND ts.period_start = $2
              AND ts.period_type = $3
            "#,
            team_id,
            period_start,
            period_type,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(row.map(|r| TeamSnapshotRow {
            id: r.id,
            team_id: r.team_id,
            team_name: r.team_name,
            member_count: r.member_count,
            period_start: r.period_start,
            period_end: r.period_end,
            period_type: r.period_type,
            throughput: r.throughput,
            avg_review_turnaround_hours: r.avg_review_turnaround_hours,
        }))
    }

    /// Get snapshots for multiple teams for a specific period.
    pub async fn compare_team_snapshots(
        &self,
        team_ids: &[Uuid],
        period_start: Date,
        period_type: &str,
    ) -> Result<Vec<TeamSnapshotRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT ts.id, ts.team_id, t.name AS team_name,
                   (SELECT COUNT(*)::int FROM org.team_memberships tm
                    WHERE tm.team_id = t.id
                      AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)) AS "member_count!",
                   ts.period_start, ts.period_end, ts.period_type,
                   ts.throughput, ts.avg_review_turnaround_hours
            FROM metrics.team_snapshots ts
            JOIN org.teams t ON t.id = ts.team_id
            WHERE ts.team_id = ANY($1)
              AND ts.period_start = $2
              AND ts.period_type = $3
            ORDER BY t.name
            "#,
            team_ids,
            period_start,
            period_type,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| TeamSnapshotRow {
                id: r.id,
                team_id: r.team_id,
                team_name: r.team_name,
                member_count: r.member_count,
                period_start: r.period_start,
                period_end: r.period_end,
                period_type: r.period_type,
                throughput: r.throughput,
                avg_review_turnaround_hours: r.avg_review_turnaround_hours,
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
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| PeriodRow {
                start: r.period_start,
                end: r.period_end,
                period_type: r.period_type,
            })
            .collect())
    }

    /// Upsert a team snapshot (used by metrics computation).
    pub async fn upsert_snapshot(&self, snap: &SnapshotInput) -> Result<(), Error> {
        let id = snap.id;
        let team_id = snap.team_id;
        let period_start = snap.period_start;
        let period_end = snap.period_end;
        let period_type = &snap.period_type;
        let throughput = snap.throughput;
        let avg_review_turnaround_hours = snap.avg_review_turnaround_hours;
        sqlx::query!(
            r#"
            INSERT INTO metrics.team_snapshots (
                id, team_id, period_start, period_end, period_type,
                throughput, avg_review_turnaround_hours, computed_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, now())
            ON CONFLICT (team_id, period_start, period_type)
            DO UPDATE SET
                period_end = EXCLUDED.period_end,
                throughput = EXCLUDED.throughput,
                avg_review_turnaround_hours = EXCLUDED.avg_review_turnaround_hours,
                computed_at = now()
            "#,
            id,
            team_id,
            period_start,
            period_end,
            period_type,
            throughput,
            avg_review_turnaround_hours,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Get contributions for a team's members within a date range.
    /// Used by metrics computation to calculate throughput and review turnaround.
    pub async fn get_team_contributions(
        &self,
        team_id: Uuid,
        period_start: Date,
        period_end: Date,
    ) -> Result<Vec<ContributionMetricRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT c.person_id, c.contribution_type, c.state,
                   c.created_at, c.closed_at,
                   c.metrics, c.metadata
            FROM activity.contributions c
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
            WHERE tm.team_id = $1
              AND (tm.end_date IS NULL OR tm.end_date > $3::date)
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
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| ContributionMetricRow {
                person_id: r.person_id,
                contribution_type: r.contribution_type,
                state: r.state,
                created_at: r.created_at,
                closed_at: r.closed_at,
                metrics: r.metrics,
                metadata: r.metadata,
            })
            .collect())
    }
}

/// A distinct period that has snapshot data.
pub struct PeriodRow {
    pub start: Date,
    pub end: Date,
    pub period_type: String,
}

/// Input for upserting a team snapshot.
pub struct SnapshotInput {
    pub id: Uuid,
    pub team_id: Uuid,
    pub period_start: Date,
    pub period_end: Date,
    pub period_type: String,
    pub throughput: i32,
    pub avg_review_turnaround_hours: Option<f32>,
}
