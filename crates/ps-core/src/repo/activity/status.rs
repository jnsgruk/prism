use crate::Error;

use super::{ActivityRepo, SourceStatusRow};

impl ActivityRepo {
    /// Get the status of all enabled sources (cross-schema join with
    /// `config.source_configs`).
    pub async fn get_source_statuses(&self) -> Result<Vec<SourceStatusRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                sc.name,
                sc.source_type,
                iw.watermark_value as "watermark_value?",
                lr.completed_at as "last_successful_run?",
                iw.last_attempt,
                iw.last_error,
                lr.items_collected as "items_collected_last_run?",
                ar.id IS NOT NULL as "has_active_run!",
                ar.items_collected as "active_run_items?",
                ar.started_at as "active_run_started_at?",
                iw.current_invocation_id as "current_invocation_id?",
                ar.progress as "active_run_progress?"
            FROM config.source_configs sc
            LEFT JOIN activity.ingestion_watermarks iw
                ON sc.name = iw.source_name
            LEFT JOIN LATERAL (
                SELECT id, items_collected, started_at, progress
                FROM activity.ingestion_runs ir
                WHERE ir.source_name = sc.name
                  AND ir.completed_at IS NULL
                ORDER BY ir.started_at DESC
                LIMIT 1
            ) ar ON true
            LEFT JOIN LATERAL (
                SELECT completed_at, items_collected
                FROM activity.ingestion_runs ir
                WHERE ir.source_name = sc.name
                  AND ir.status IN ('completed', 'completed_with_warnings')
                ORDER BY ir.completed_at DESC
                LIMIT 1
            ) lr ON true
            WHERE sc.enabled = true
            ORDER BY sc.name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| SourceStatusRow {
                name: r.name,
                source_type: r
                    .source_type
                    .parse()
                    .unwrap_or(crate::models::Platform::Github),
                watermark_value: r.watermark_value,
                last_successful_run: r.last_successful_run,
                last_attempt: r.last_attempt,
                last_error: r.last_error,
                items_collected_last_run: r.items_collected_last_run,
                has_active_run: r.has_active_run,
                active_run_items: r.active_run_items,
                active_run_started_at: r.active_run_started_at,
                current_invocation_id: r.current_invocation_id,
                active_run_progress: r.active_run_progress,
            })
            .collect())
    }

    /// Cancel all active (incomplete) runs for a source and clear the invocation ID.
    pub async fn cancel_active_runs(&self, source_name: &str) -> Result<(), Error> {
        self.cancel_active_runs_with_reason(source_name, "Cancelled by user")
            .await
    }

    /// Cancel all active (incomplete) runs for a source with a custom reason.
    pub async fn cancel_active_runs_with_reason(
        &self,
        source_name: &str,
        reason: &str,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET completed_at = now(), status = 'cancelled', error_message = $2
            WHERE source_name = $1
              AND completed_at IS NULL
            "#,
            source_name,
            reason,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        self.clear_current_invocation_id(source_name).await?;

        Ok(())
    }

    /// Delete all activity data: contributions, watermarks, runs, etag cache, metric snapshots.
    /// Returns the number of contributions deleted.
    pub async fn reset_all(&self) -> Result<i64, Error> {
        let mut tx = self.pool.begin().await.map_err(Error::from)?;

        sqlx::query!("DELETE FROM metrics.team_snapshots")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        let contribs = sqlx::query!("DELETE FROM activity.contributions")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        sqlx::query!("DELETE FROM activity.ingestion_runs")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        sqlx::query!("DELETE FROM activity.ingestion_watermarks")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        sqlx::query!("DELETE FROM activity.etag_cache")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        tx.commit().await.map_err(Error::from)?;

        Ok(contribs.rows_affected().cast_signed())
    }
}
