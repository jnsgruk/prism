use super::*;

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
    pub source_platforms: Vec<String>,
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

impl MetricsRepo {
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

        let Some(snapshot) = row else {
            return Ok(None);
        };

        let source_platforms = self.get_snapshot_source_platforms(snapshot.id).await?;

        Ok(Some(TeamSnapshotRow {
            id: snapshot.id,
            team_id: snapshot.team_id,
            team_name: snapshot.team_name,
            member_count: snapshot.member_count,
            period_start: snapshot.period_start,
            period_end: snapshot.period_end,
            period_type: PeriodType::from_str_opt(&snapshot.period_type).unwrap_or(period_type),
            throughput: snapshot.throughput,
            avg_review_turnaround_hours: snapshot.avg_review_turnaround_hours,
            avg_cycle_time_hours: snapshot.avg_cycle_time_hours,
            wip_avg: snapshot.wip_avg,
            flow_efficiency: snapshot.flow_efficiency,
            lead_time_hours: snapshot.lead_time_hours,
            raw_metrics: snapshot.raw_metrics,
            source_platforms,
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

        let snapshot_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
        let source_map = self
            .get_bulk_snapshot_source_platforms(&snapshot_ids)
            .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let source_platforms = source_map.get(&r.id).cloned().unwrap_or_default();
                TeamSnapshotRow {
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
                    source_platforms,
                }
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
    ///
    /// Returns the actual snapshot ID (which may differ from `snap.id` if an
    /// existing row was updated via the ON CONFLICT clause).
    pub async fn upsert_snapshot(&self, snap: &SnapshotInput) -> Result<Uuid, Error> {
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
        let row = sqlx::query_scalar!(
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
            RETURNING id
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
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row)
    }

    /// Get the last N snapshots for a team and period type (for trend data).
    pub async fn get_snapshot_history(
        &self,
        team_id: Uuid,
        period_type: PeriodType,
        limit: i32,
    ) -> Result<Vec<TeamSnapshotRow>, Error> {
        let period_type_str = period_type.as_str();
        let rows = sqlx::query!(
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
              AND ts.period_type = $2
            ORDER BY ts.period_start DESC
            LIMIT $3
            "#,
            team_id,
            period_type_str,
            i64::from(limit),
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
                source_platforms: Vec::new(), // not needed for trend data
            })
            .collect())
    }
}
