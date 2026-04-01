use crate::Error;
use crate::models::Pipeline;
use uuid::Uuid;

use super::ActivityRepo;

impl ActivityRepo {
    /// Create a new pipeline record with status 'running'.
    pub async fn create_pipeline(
        &self,
        id: Uuid,
        invocation_id: Option<&str>,
    ) -> Result<Pipeline, Error> {
        let row = sqlx::query!(
            r#"
            INSERT INTO activity.pipelines (id, status, current_invocation_id)
            VALUES ($1, 'running', $2)
            RETURNING id, status, current_stage, started_at, completed_at, stages, current_invocation_id, error
            "#,
            id,
            invocation_id,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(Pipeline {
            id: row.id,
            status: row.status,
            current_stage: row.current_stage,
            started_at: row.started_at,
            completed_at: row.completed_at,
            stages: row.stages,
            current_invocation_id: row.current_invocation_id,
            error: row.error,
        })
    }

    /// Update the current stage and stages JSONB for a running pipeline.
    pub async fn update_pipeline_stage(
        &self,
        id: Uuid,
        stage: &str,
        stages: &serde_json::Value,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.pipelines
            SET current_stage = $2, stages = $3
            WHERE id = $1
            "#,
            id,
            stage,
            stages,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Mark a pipeline as completed (or `failed/completed_with_warnings`).
    pub async fn complete_pipeline(
        &self,
        id: Uuid,
        status: &str,
        stages: &serde_json::Value,
        error: Option<&str>,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.pipelines
            SET completed_at = now(), status = $2, stages = $3, current_invocation_id = NULL, error = $4
            WHERE id = $1
            "#,
            id,
            status,
            stages,
            error,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Get the most recent pipeline (by `started_at`).
    pub async fn get_latest_pipeline(&self) -> Result<Option<Pipeline>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT id, status, current_stage, started_at, completed_at, stages, current_invocation_id, error
            FROM activity.pipelines
            ORDER BY started_at DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row.map(|r| Pipeline {
            id: r.id,
            status: r.status,
            current_stage: r.current_stage,
            started_at: r.started_at,
            completed_at: r.completed_at,
            stages: r.stages,
            current_invocation_id: r.current_invocation_id,
            error: r.error,
        }))
    }

    /// List recent pipelines, ordered by `started_at` DESC.
    pub async fn list_recent_pipelines(&self, limit: i64) -> Result<Vec<Pipeline>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, status, current_stage, started_at, completed_at, stages, current_invocation_id, error
            FROM activity.pipelines
            ORDER BY started_at DESC
            LIMIT $1
            "#,
            limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| Pipeline {
                id: r.id,
                status: r.status,
                current_stage: r.current_stage,
                started_at: r.started_at,
                completed_at: r.completed_at,
                stages: r.stages,
                current_invocation_id: r.current_invocation_id,
                error: r.error,
            })
            .collect())
    }

    /// Check if there's a currently running pipeline.
    pub async fn has_active_pipeline(&self) -> Result<bool, Error> {
        let row = sqlx::query_scalar!(
            r#"
            SELECT EXISTS(SELECT 1 FROM activity.pipelines WHERE status = 'running') AS "exists!"
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row)
    }
}
