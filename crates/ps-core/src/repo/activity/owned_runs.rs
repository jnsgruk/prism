use uuid::Uuid;

use crate::Error;

use super::ActivityRepo;

impl ActivityRepo {
    /// Terminal run updates cannot resurrect a cancellation or affect another owner.
    pub async fn complete_pipeline_run(&self, id: Uuid, items: i32) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET completed_at = now(), status = 'completed', items_collected = $2
            WHERE id = $1 AND completed_at IS NULL AND pipeline_id IS NOT NULL
            "#,
            id,
            items,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn fail_pipeline_run(&self, id: Uuid, error: &str) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET completed_at = now(), status = 'failed', error_message = $2
            WHERE id = $1 AND completed_at IS NULL AND pipeline_id IS NOT NULL
            "#,
            id,
            error,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn complete_pipeline_run_with_warnings(
        &self,
        id: Uuid,
        items: i32,
        error: &str,
        metadata: serde_json::Value,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET completed_at = now(), status = 'completed_with_warnings',
                items_collected = $2, error_message = $3, metadata = $4
            WHERE id = $1 AND completed_at IS NULL AND pipeline_id IS NOT NULL
            "#,
            id,
            items,
            error,
            metadata,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
