use crate::Error;
use uuid::Uuid;

use super::{ActivityRepo, IngestionRunRow};

impl ActivityRepo {
    /// Create a new run record with status 'running'.
    pub async fn create_run(
        &self,
        id: Uuid,
        source_name: &str,
        handler_name: &str,
        handler_method: &str,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO activity.ingestion_runs (id, source_name, started_at, status, handler_name, handler_method)
            VALUES ($1, $2, now(), 'running', $3, $4)
            "#,
            id,
            source_name,
            handler_name,
            handler_method,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Mark an ingestion run as completed.
    pub async fn complete_run(&self, id: Uuid, items_collected: i32) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET completed_at = now(), status = 'completed', items_collected = $2
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

    /// Mark an ingestion run as failed with an error message.
    pub async fn fail_run(&self, id: Uuid, error_message: &str) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET completed_at = now(), status = 'failed', error_message = $2
            WHERE id = $1
            "#,
            id,
            error_message,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// List recent runs, optionally filtered by source name and/or handler name.
    /// When `ingestion_only` is true, restricts to data-ingestion runs only
    /// (`handler_method` = `run_ingestion`, `backfill`, or `run_cycle`),
    /// excluding team sync, metrics, and other system handler runs.
    pub async fn list_runs(
        &self,
        source_name: Option<&str>,
        handler_name: Option<&str>,
        ingestion_only: bool,
    ) -> Result<Vec<IngestionRunRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, source_name, started_at, completed_at, status,
                   items_collected, error_message, handler_name, handler_method
            FROM activity.ingestion_runs
            WHERE ($1::text IS NULL OR source_name = $1)
              AND ($2::text IS NULL OR handler_name = $2)
              AND (NOT $3::bool OR handler_method IN ('run_ingestion', 'backfill', 'run_cycle'))
            ORDER BY started_at DESC
            LIMIT 100
            "#,
            source_name,
            handler_name,
            ingestion_only,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| IngestionRunRow {
                id: r.id,
                source_name: r.source_name,
                started_at: r.started_at,
                completed_at: r.completed_at,
                status: r
                    .status
                    .parse()
                    .unwrap_or(crate::models::IngestionStatus::Failed),
                items_collected: r.items_collected,
                error_message: r.error_message,
                handler_name: r.handler_name,
                handler_method: r.handler_method,
            })
            .collect())
    }

    /// Get a single run by ID.
    pub async fn get_run(&self, id: Uuid) -> Result<Option<IngestionRunRow>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT id, source_name, started_at, completed_at, status,
                   items_collected, error_message, handler_name, handler_method
            FROM activity.ingestion_runs
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row.map(|r| IngestionRunRow {
            id: r.id,
            source_name: r.source_name,
            started_at: r.started_at,
            completed_at: r.completed_at,
            status: r
                .status
                .parse()
                .unwrap_or(crate::models::IngestionStatus::Failed),
            items_collected: r.items_collected,
            error_message: r.error_message,
            handler_name: r.handler_name,
            handler_method: r.handler_method,
        }))
    }

    /// Cancel a single run by ID.
    pub async fn cancel_run_by_id(&self, id: Uuid) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET completed_at = now(), status = 'cancelled', error_message = 'Cancelled by user'
            WHERE id = $1
              AND completed_at IS NULL
            "#,
            id,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Get all currently-running handler runs.
    pub async fn get_active_handler_runs(&self) -> Result<Vec<IngestionRunRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, source_name, started_at, completed_at, status,
                   items_collected, error_message, handler_name, handler_method
            FROM activity.ingestion_runs
            WHERE status = 'running'
            ORDER BY started_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| IngestionRunRow {
                id: r.id,
                source_name: r.source_name,
                started_at: r.started_at,
                completed_at: r.completed_at,
                status: r
                    .status
                    .parse()
                    .unwrap_or(crate::models::IngestionStatus::Failed),
                items_collected: r.items_collected,
                error_message: r.error_message,
                handler_name: r.handler_name,
                handler_method: r.handler_method,
            })
            .collect())
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
