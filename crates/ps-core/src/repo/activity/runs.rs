use crate::Error;
use crate::models::{HandlerMethod, HandlerName, SourceName};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{ActivityRepo, BackupRunRow, IngestionRunRow};

impl ActivityRepo {
    /// Create a new run record with status 'running'.
    pub async fn create_run(
        &self,
        id: Uuid,
        source_name: &SourceName,
        handler_name: &HandlerName,
        handler_method: &HandlerMethod,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO activity.ingestion_runs (id, source_name, started_at, status, handler_name, handler_method)
            VALUES ($1, $2, now(), 'running', $3, $4)
            "#,
            id,
            source_name.as_str(),
            handler_name.as_str(),
            handler_method.as_str(),
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

    /// Mark an ingestion run as completed with warnings (partial success).
    pub async fn complete_run_with_warnings(
        &self,
        id: Uuid,
        items_collected: i32,
        error_message: &str,
        metadata: serde_json::Value,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET completed_at = now(),
                status = 'completed_with_warnings',
                items_collected = $2,
                error_message = $3,
                metadata = $4
            WHERE id = $1
            "#,
            id,
            items_collected,
            error_message,
            metadata,
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
                   items_collected, error_message, handler_name, handler_method,
                   pipeline_id
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
                pipeline_id: r.pipeline_id,
            })
            .collect())
    }

    /// Get a single run by ID.
    pub async fn get_run(&self, id: Uuid) -> Result<Option<IngestionRunRow>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT id, source_name, started_at, completed_at, status,
                   items_collected, error_message, handler_name, handler_method,
                   pipeline_id
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
            pipeline_id: r.pipeline_id,
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
                   items_collected, error_message, handler_name, handler_method,
                   pipeline_id
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
                pipeline_id: r.pipeline_id,
            })
            .collect())
    }

    /// List runs that are not yet linked to a pipeline but started after the
    /// given timestamp.  Used to show in-progress runs before `link_runs_to_pipeline`
    /// has been called by the workflow.
    pub async fn list_unlinked_runs_since(
        &self,
        since: OffsetDateTime,
    ) -> Result<Vec<IngestionRunRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, source_name, started_at, completed_at, status,
                   items_collected, error_message, handler_name, handler_method,
                   pipeline_id
            FROM activity.ingestion_runs
            WHERE pipeline_id IS NULL
              AND source_name != '_pipeline'
              AND started_at >= $1
            ORDER BY started_at ASC
            "#,
            since,
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
                pipeline_id: r.pipeline_id,
            })
            .collect())
    }

    /// Link all unlinked runs that started after the pipeline to this pipeline.
    /// Used by the pipeline workflow after each stage to associate handler runs.
    pub async fn link_runs_to_pipeline(&self, pipeline_id: Uuid) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET pipeline_id = $1
            WHERE pipeline_id IS NULL
              AND source_name != '_pipeline'
              AND started_at >= (SELECT started_at FROM activity.pipelines WHERE id = $1)
            "#,
            pipeline_id,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

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

    /// List runs associated with a set of pipeline IDs.
    pub async fn list_runs_for_pipelines(
        &self,
        pipeline_ids: &[Uuid],
    ) -> Result<Vec<IngestionRunRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, source_name, started_at, completed_at, status,
                   items_collected, error_message, handler_name, handler_method,
                   pipeline_id
            FROM activity.ingestion_runs
            WHERE pipeline_id = ANY($1)
              AND source_name != '_pipeline'
            ORDER BY started_at ASC
            "#,
            pipeline_ids,
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
                pipeline_id: r.pipeline_id,
            })
            .collect())
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

    /// Find the latest run for a given `source_name` and `handler_name`.
    /// Used by ps-server to poll backup progress.
    pub async fn find_latest_run_by_source_and_handler(
        &self,
        source_name: &str,
        handler_name: &str,
    ) -> Result<Option<BackupRunRow>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                status AS "status!",
                items_collected,
                error_message,
                progress
            FROM activity.ingestion_runs
            WHERE source_name = $1 AND handler_name = $2
            ORDER BY started_at DESC
            LIMIT 1
            "#,
            source_name,
            handler_name,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row.map(|r| BackupRunRow {
            id: r.id,
            status: r
                .status
                .parse()
                .unwrap_or(crate::models::IngestionStatus::Failed),
            items_collected: r.items_collected,
            error_message: r.error_message,
            progress: r.progress,
        }))
    }

    /// Find any active (running) run for a given `handler_name`.
    /// Used by ps-server to prevent concurrent backup invocations.
    pub async fn find_active_run_by_handler(
        &self,
        handler_name: &str,
    ) -> Result<Option<IngestionRunRow>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                source_name,
                started_at,
                completed_at,
                status AS "status!",
                items_collected,
                error_message,
                handler_name AS "handler_name!",
                handler_method AS "handler_method!",
                pipeline_id
            FROM activity.ingestion_runs
            WHERE handler_name = $1 AND status = 'running'
            ORDER BY started_at DESC
            LIMIT 1
            "#,
            handler_name,
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
            pipeline_id: r.pipeline_id,
        }))
    }

    /// Mark all active (running) runs for a handler as failed.
    /// Used by the `force` flag on backup creation to clear orphaned runs
    /// left behind when a Restate invocation is killed.
    pub async fn fail_active_runs_by_handler(
        &self,
        handler_name: &str,
        error_message: &str,
    ) -> Result<u64, Error> {
        let result = sqlx::query!(
            r#"
            UPDATE activity.ingestion_runs
            SET completed_at = now(), status = 'failed', error_message = $2
            WHERE handler_name = $1 AND status = 'running'
            "#,
            handler_name,
            error_message,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected())
    }

    /// Check if a run has been cancelled. Used by `export_backup` for
    /// cooperative cancellation between tables/pages.
    pub async fn is_run_cancelled(&self, id: Uuid) -> Result<bool, Error> {
        let status = sqlx::query_scalar!(
            r#"
            SELECT status AS "status!"
            FROM activity.ingestion_runs
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(status
            .and_then(|s| s.parse::<crate::models::IngestionStatus>().ok())
            .is_some_and(|s| s == crate::models::IngestionStatus::Cancelled))
    }
}
