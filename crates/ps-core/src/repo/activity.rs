use crate::Error;
use crate::ingestion::ContributionInput;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Repository for the `activity` schema: contributions, ingestion watermarks,
/// `ETag` cache, and ingestion runs.
#[derive(Clone)]
pub struct ActivityRepo {
    pool: PgPool,
}

/// A row from `activity.ingestion_runs`.
pub struct IngestionRunRow {
    pub id: Uuid,
    pub source_name: String,
    pub started_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
    pub status: String,
    pub items_collected: Option<i32>,
    pub error_message: Option<String>,
}

/// A joined row from `config.source_configs` + `activity.ingestion_watermarks`.
pub struct SourceStatusRow {
    pub name: String,
    pub source_type: String,
    pub watermark_value: Option<String>,
    pub last_successful_run: Option<OffsetDateTime>,
    pub last_attempt: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    pub items_collected_last_run: Option<i32>,
    /// Whether this source has a currently running ingestion (no `completed_at`).
    pub has_active_run: bool,
    /// Items collected so far in the active run (from `ingestion_runs`).
    pub active_run_items: Option<i32>,
    /// When the active run started.
    pub active_run_started_at: Option<OffsetDateTime>,
    /// Current Restate invocation ID (for reconciliation).
    pub current_invocation_id: Option<String>,
}

impl ActivityRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Upsert a contribution into `activity.contributions`.
    pub async fn upsert_contribution(
        &self,
        id: Uuid,
        person_id: Option<Uuid>,
        item: &ContributionInput,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO activity.contributions (
                id, person_id, platform, contribution_type, platform_id,
                title, url, state, created_at, updated_at, closed_at,
                metrics, metadata, content, state_history, ingested_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, now())
            ON CONFLICT (platform, platform_id)
            DO UPDATE SET
                person_id = COALESCE(EXCLUDED.person_id, activity.contributions.person_id),
                title = EXCLUDED.title,
                url = EXCLUDED.url,
                state = EXCLUDED.state,
                updated_at = EXCLUDED.updated_at,
                closed_at = EXCLUDED.closed_at,
                metrics = EXCLUDED.metrics,
                metadata = EXCLUDED.metadata,
                content = EXCLUDED.content,
                state_history = EXCLUDED.state_history,
                ingested_at = now()
            "#,
            id,
            person_id,
            item.platform,
            item.contribution_type,
            item.platform_id,
            item.title,
            item.url,
            item.state,
            item.created_at,
            item.updated_at,
            item.closed_at,
            item.metrics,
            item.metadata,
            item.content,
            item.state_history,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Read the current watermark value for a source.
    pub async fn get_watermark(&self, source_name: &str) -> Result<Option<String>, Error> {
        sqlx::query_scalar!(
            r#"
            SELECT watermark_value
            FROM activity.ingestion_watermarks
            WHERE source_name = $1
            "#,
            source_name,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))
    }

    /// Upsert the ingestion watermark after a successful run.
    pub async fn upsert_watermark(
        &self,
        source_name: &str,
        value: &str,
        items_collected: i32,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO activity.ingestion_watermarks (
                source_name, watermark_value, last_successful_run, last_attempt, items_collected_last_run
            )
            VALUES ($1, $2, now(), now(), $3)
            ON CONFLICT (source_name)
            DO UPDATE SET
                watermark_value = $2,
                last_successful_run = now(),
                last_attempt = now(),
                last_error = NULL,
                items_collected_last_run = $3
            "#,
            source_name,
            value,
            items_collected,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Retrieve a cached `ETag` for a source + endpoint URL.
    pub async fn get_cached_etag(
        &self,
        source_name: &str,
        endpoint_url: &str,
    ) -> Result<Option<String>, Error> {
        sqlx::query_scalar!(
            r#"
            SELECT etag
            FROM activity.etag_cache
            WHERE source_name = $1
              AND endpoint_url = $2
            "#,
            source_name,
            endpoint_url,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))
    }

    /// Store or update a cached `ETag` for a source + endpoint URL.
    pub async fn set_cached_etag(
        &self,
        source_name: &str,
        endpoint_url: &str,
        etag: &str,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO activity.etag_cache (source_name, endpoint_url, etag, last_used)
            VALUES ($1, $2, $3, now())
            ON CONFLICT (source_name, endpoint_url)
            DO UPDATE SET etag = $3, last_used = now()
            "#,
            source_name,
            endpoint_url,
            etag,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Create a new ingestion run record with status 'running'.
    pub async fn create_run(&self, id: Uuid, source_name: &str) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO activity.ingestion_runs (id, source_name, started_at, status)
            VALUES ($1, $2, now(), 'running')
            "#,
            id,
            source_name,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

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
        .map_err(|e| Error::Database(e.to_string()))?;

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
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// List recent ingestion runs, optionally filtered by source name.
    pub async fn list_runs(
        &self,
        source_name: Option<&str>,
    ) -> Result<Vec<IngestionRunRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, source_name, started_at, completed_at, status,
                   items_collected, error_message
            FROM activity.ingestion_runs
            WHERE ($1::text IS NULL OR source_name = $1)
            ORDER BY started_at DESC
            LIMIT 50
            "#,
            source_name,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| IngestionRunRow {
                id: r.id,
                source_name: r.source_name,
                started_at: r.started_at,
                completed_at: r.completed_at,
                status: r.status,
                items_collected: r.items_collected,
                error_message: r.error_message,
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
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Store the Restate invocation ID for a running ingestion.
    pub async fn set_current_invocation_id(
        &self,
        source_name: &str,
        invocation_id: &str,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO activity.ingestion_watermarks (source_name, current_invocation_id)
            VALUES ($1, $2)
            ON CONFLICT (source_name)
            DO UPDATE SET current_invocation_id = $2
            "#,
            source_name,
            invocation_id,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Get the current Restate invocation ID for a source.
    pub async fn get_current_invocation_id(
        &self,
        source_name: &str,
    ) -> Result<Option<String>, Error> {
        Ok(sqlx::query_scalar!(
            r#"
            SELECT current_invocation_id
            FROM activity.ingestion_watermarks
            WHERE source_name = $1
            "#,
            source_name,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?
        .flatten())
    }

    /// Clear the current invocation ID (on completion or cancellation).
    pub async fn clear_current_invocation_id(&self, source_name: &str) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.ingestion_watermarks
            SET current_invocation_id = NULL
            WHERE source_name = $1
            "#,
            source_name,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
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
        .map_err(|e| Error::Database(e.to_string()))?;

        self.clear_current_invocation_id(source_name).await?;

        Ok(())
    }

    /// Get the status of all enabled sources (cross-schema join with
    /// `config.source_configs`).
    pub async fn get_source_statuses(&self) -> Result<Vec<SourceStatusRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                sc.name,
                sc.source_type,
                iw.watermark_value as "watermark_value?",
                iw.last_successful_run,
                iw.last_attempt,
                iw.last_error,
                iw.items_collected_last_run,
                ar.id IS NOT NULL as "has_active_run!",
                ar.items_collected as "active_run_items?",
                ar.started_at as "active_run_started_at?",
                iw.current_invocation_id as "current_invocation_id?"
            FROM config.source_configs sc
            LEFT JOIN activity.ingestion_watermarks iw
                ON sc.name = iw.source_name
            LEFT JOIN LATERAL (
                SELECT id, items_collected, started_at
                FROM activity.ingestion_runs ir
                WHERE ir.source_name = sc.name
                  AND ir.completed_at IS NULL
                ORDER BY ir.started_at DESC
                LIMIT 1
            ) ar ON true
            WHERE sc.enabled = true
            ORDER BY sc.name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| SourceStatusRow {
                name: r.name,
                source_type: r.source_type,
                watermark_value: r.watermark_value,
                last_successful_run: r.last_successful_run,
                last_attempt: r.last_attempt,
                last_error: r.last_error,
                items_collected_last_run: r.items_collected_last_run,
                has_active_run: r.has_active_run,
                active_run_items: r.active_run_items,
                active_run_started_at: r.active_run_started_at,
                current_invocation_id: r.current_invocation_id,
            })
            .collect())
    }

    /// Delete all activity data: contributions, watermarks, runs, etag cache, metric snapshots.
    /// Returns the number of contributions deleted.
    pub async fn reset_all(&self) -> Result<i64, Error> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        // Bulk DELETEs — parameterless, table names are hardcoded constants.
        for table in &["metrics.team_snapshots", "metrics.individual_snapshots"] {
            sqlx::query(&format!("DELETE FROM {table}"))
                .execute(&mut *tx)
                .await
                .map_err(|e| Error::Database(e.to_string()))?;
        }

        let contribs = sqlx::query("DELETE FROM activity.contributions")
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        for table in &[
            "activity.ingestion_runs",
            "activity.watermarks",
            "activity.etag_cache",
        ] {
            sqlx::query(&format!("DELETE FROM {table}"))
                .execute(&mut *tx)
                .await
                .map_err(|e| Error::Database(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(contribs.rows_affected().cast_signed())
    }
}
