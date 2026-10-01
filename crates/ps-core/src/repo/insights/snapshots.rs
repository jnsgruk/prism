use super::*;

impl InsightsRepo {
    pub async fn upsert_snapshot(&self, p: &UpsertSnapshotParams) -> Result<Uuid, Error> {
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO reasoning.insight_snapshots (
                team_id, period_start, period_end, period_type,
                avg_review_depth, review_count, rubber_stamp_pct, deep_review_pct,
                depth_distribution,
                constructive_count, neutral_count, critical_count, hostile_count,
                significant_count, notable_count, routine_count,
                avg_depth_on_significant, avg_depth_on_notable, avg_depth_on_routine,
                enrichment_coverage, raw_insights,
                computed_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8,
                $9,
                $10, $11, $12, $13,
                $14, $15, $16,
                $17, $18, $19,
                $20, $21,
                now()
            )
            ON CONFLICT (team_id, period_start, period_type) DO UPDATE SET
                period_end = EXCLUDED.period_end,
                avg_review_depth = EXCLUDED.avg_review_depth,
                review_count = EXCLUDED.review_count,
                rubber_stamp_pct = EXCLUDED.rubber_stamp_pct,
                deep_review_pct = EXCLUDED.deep_review_pct,
                depth_distribution = EXCLUDED.depth_distribution,
                constructive_count = EXCLUDED.constructive_count,
                neutral_count = EXCLUDED.neutral_count,
                critical_count = EXCLUDED.critical_count,
                hostile_count = EXCLUDED.hostile_count,
                significant_count = EXCLUDED.significant_count,
                notable_count = EXCLUDED.notable_count,
                routine_count = EXCLUDED.routine_count,
                avg_depth_on_significant = EXCLUDED.avg_depth_on_significant,
                avg_depth_on_notable = EXCLUDED.avg_depth_on_notable,
                avg_depth_on_routine = EXCLUDED.avg_depth_on_routine,
                enrichment_coverage = EXCLUDED.enrichment_coverage,
                raw_insights = EXCLUDED.raw_insights,
                computed_at = now()
            RETURNING id
            "#,
            p.team_id,
            p.period_start,
            p.period_end,
            &p.period_type,
            p.avg_review_depth,
            p.review_count,
            p.rubber_stamp_pct,
            p.deep_review_pct,
            &p.depth_distribution,
            p.constructive_count,
            p.neutral_count,
            p.critical_count,
            p.hostile_count,
            p.significant_count,
            p.notable_count,
            p.routine_count,
            p.avg_depth_on_significant,
            p.avg_depth_on_notable,
            p.avg_depth_on_routine,
            &p.enrichment_coverage,
            &p.raw_insights,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(id)
    }

    pub async fn get_previous_snapshot(
        &self,
        team_id: Uuid,
        period_start: Date,
        period_type: &str,
    ) -> Result<Option<SnapshotRow>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT
                avg_review_depth AS "avg_review_depth: f32",
                review_count AS "review_count!",
                rubber_stamp_pct AS "rubber_stamp_pct: f32",
                deep_review_pct AS "deep_review_pct: f32",
                significant_count AS "significant_count!",
                notable_count AS "notable_count!",
                routine_count AS "routine_count!"
            FROM reasoning.insight_snapshots
            WHERE team_id = $1
              AND period_type = $2
              AND period_start < $3
            ORDER BY period_start DESC
            LIMIT 1
            "#,
            team_id,
            period_type,
            period_start,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row.map(|r| SnapshotRow {
            avg_review_depth: r.avg_review_depth,
            review_count: r.review_count,
            rubber_stamp_pct: r.rubber_stamp_pct,
            deep_review_pct: r.deep_review_pct,
            significant_count: r.significant_count,
            notable_count: r.notable_count,
            routine_count: r.routine_count,
        }))
    }
}
