//! Durable work committed atomically with scoped contribution changes.

use serde::{Deserialize, Serialize};
use time::Date;
use uuid::Uuid;

use super::ActivityRepo;
use crate::{Error, models::PeriodType};

/// A bounded period and the exact dirty generations that its computation covers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInvalidationPeriod {
    pub period_type: PeriodType,
    pub period_start: Date,
    pub invalidation_ids: Vec<Uuid>,
}

impl ActivityRepo {
    /// Select bounded unique periods. Recovery only reads terminal owners: it
    /// cannot race writes or downstream processing of an active person import.
    /// Insight acknowledgement waits until the changed inputs leave the AI queue.
    pub async fn pending_snapshot_invalidations(
        &self,
        pipeline_id: Option<Uuid>,
        insights: bool,
        limit: i64,
    ) -> Result<Vec<SnapshotInvalidationPeriod>, Error> {
        let rows = sqlx::query!(
            r#"
            WITH pending AS (
            SELECT dirty.id, dirty.period_type, dirty.period_start
            FROM activity.snapshot_invalidations dirty
            JOIN activity.contribution_changes changes ON changes.id = dirty.change_id
            JOIN activity.pipelines pipeline ON pipeline.id = changes.pipeline_id
            WHERE ($1::uuid IS NULL OR changes.pipeline_id = $1)
                AND ($1::uuid IS NOT NULL OR pipeline.status NOT IN ('pending', 'running', 'cancelling'))
                AND CASE WHEN $2 THEN dirty.insights_refreshed_at IS NULL
                    ELSE dirty.metrics_refreshed_at IS NULL END
                AND (NOT $2 OR (dirty.metrics_refreshed_at IS NOT NULL AND NOT EXISTS (
                    SELECT 1 FROM reasoning.enrichment_queue queue
                    WHERE queue.contribution_id = changes.contribution_id
                )))
            ORDER BY dirty.period_start, dirty.period_type, dirty.id
            LIMIT 1024
            )
            SELECT period_type, period_start,
                array_agg(id ORDER BY id) AS "invalidation_ids!"
            FROM pending
            GROUP BY period_type, period_start
            ORDER BY period_start, period_type
            LIMIT $3
            "#,
            pipeline_id,
            insights,
            limit.clamp(1, 8),
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(SnapshotInvalidationPeriod {
                    period_type: row.period_type.parse().map_err(Error::Internal)?,
                    period_start: row.period_start,
                    invalidation_ids: row.invalidation_ids,
                })
            })
            .collect()
    }

    /// Only acknowledge the selected generations after every team succeeds.
    /// A correction arriving during computation has a new UUID and stays dirty.
    pub async fn acknowledge_snapshot_invalidations(
        &self,
        invalidation_ids: &[Uuid],
        insights: bool,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.snapshot_invalidations
            SET metrics_refreshed_at = CASE WHEN $2 THEN metrics_refreshed_at ELSE now() END,
                insights_refreshed_at = CASE WHEN $2 THEN now() ELSE insights_refreshed_at END
            WHERE id = ANY($1)
            "#,
            invalidation_ids,
            insights,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Includes unavailable enrichment work so the pipeline cannot report it as
    /// complete merely because no currently computable insight periods exist.
    pub async fn count_pending_snapshot_invalidations(
        &self,
        pipeline_id: Uuid,
    ) -> Result<(i64, i64), Error> {
        let row = sqlx::query!(
            r#"
            SELECT COUNT(*) FILTER (WHERE dirty.metrics_refreshed_at IS NULL) AS "metrics!",
                COUNT(*) FILTER (WHERE dirty.insights_refreshed_at IS NULL) AS "insights!"
            FROM activity.snapshot_invalidations dirty
            JOIN activity.contribution_changes changes ON changes.id = dirty.change_id
            WHERE changes.pipeline_id = $1
            "#,
            pipeline_id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok((row.metrics, row.insights))
    }
}
