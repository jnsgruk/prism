use crate::Error;
use uuid::Uuid;

use super::ActivityRepo;

impl ActivityRepo {
    pub async fn record_run_coverage(
        &self,
        id: Uuid,
        coverage: &serde_json::Value,
    ) -> Result<(), Error> {
        sqlx::query!(
            "UPDATE activity.ingestion_runs SET metadata = COALESCE(metadata, '{}'::jsonb) || $2, progress = COALESCE(progress, '{}'::jsonb) || $2 WHERE id = $1",
            id, coverage,
        ).execute(&self.pool).await?;
        Ok(())
    }
    /// Update the progress of a running ingestion (items collected so far).
    pub async fn update_run_progress(&self, id: Uuid, items_collected: i32) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET items_collected = $2
            WHERE id = $1
            "#,
            id,
            items_collected,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Update the progress of a running ingestion with structured detail.
    ///
    /// The update is monotonic: progress is only written when `items_collected`
    /// is >= the current value in the database. This prevents Restate handler
    /// replays from overwriting forward progress with stale replay data.
    pub async fn update_run_progress_detail(
        &self,
        id: Uuid,
        items_collected: i32,
        progress: &serde_json::Value,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET items_collected = $2, progress = $3
            WHERE id = $1
              AND (items_collected IS NULL OR items_collected <= $2)
            "#,
            id,
            items_collected,
            progress,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }
}
