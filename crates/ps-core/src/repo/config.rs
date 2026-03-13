use crate::Error;
use crate::models::SourceConfig;
use sqlx::PgPool;
use uuid::Uuid;

/// Repository for the `config` schema: source configurations and encrypted secrets.
#[derive(Clone)]
pub struct ConfigRepo {
    pool: PgPool,
}

impl ConfigRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// List all source configurations ordered by name.
    pub async fn list_sources(&self) -> Result<Vec<SourceConfig>, Error> {
        sqlx::query_as!(
            SourceConfig,
            r#"
            SELECT id, source_type, name, enabled, settings, schedule_cron,
                   created_at, updated_at
            FROM config.source_configs
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))
    }

    /// Get a single source configuration by ID.
    pub async fn get_source(&self, id: Uuid) -> Result<Option<SourceConfig>, Error> {
        sqlx::query_as!(
            SourceConfig,
            r#"
            SELECT id, source_type, name, enabled, settings, schedule_cron,
                   created_at, updated_at
            FROM config.source_configs
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))
    }

    /// Get an enabled source configuration by name.
    pub async fn get_enabled_source_by_name(
        &self,
        name: &str,
    ) -> Result<Option<SourceConfig>, Error> {
        sqlx::query_as!(
            SourceConfig,
            r#"
            SELECT id, source_type, name, enabled, settings, schedule_cron,
                   created_at, updated_at
            FROM config.source_configs
            WHERE name = $1 AND enabled = true
            "#,
            name,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))
    }

    /// Create a new source configuration.
    pub async fn create_source(
        &self,
        id: Uuid,
        source_type: &str,
        name: &str,
        settings: &serde_json::Value,
        schedule_cron: Option<&str>,
    ) -> Result<SourceConfig, Error> {
        sqlx::query_as!(
            SourceConfig,
            r#"
            INSERT INTO config.source_configs (id, source_type, name, settings, schedule_cron)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, source_type, name, enabled, settings, schedule_cron, created_at, updated_at
            "#,
            id,
            source_type,
            name,
            settings,
            schedule_cron,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("duplicate key") {
                Error::Conflict(format!("source '{name}' already exists"))
            } else {
                Error::Database(e.to_string())
            }
        })
    }

    /// Check whether a source exists by ID.
    pub async fn source_exists(&self, id: Uuid) -> Result<bool, Error> {
        let row = sqlx::query_scalar!("SELECT id FROM config.source_configs WHERE id = $1", id,)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(row.is_some())
    }

    /// Update the `enabled` flag on a source.
    pub async fn update_source_enabled(&self, id: Uuid, enabled: bool) -> Result<(), Error> {
        sqlx::query!(
            "UPDATE config.source_configs SET enabled = $1, updated_at = now() WHERE id = $2",
            enabled,
            id,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Update the `settings` JSON on a source.
    pub async fn update_source_settings(
        &self,
        id: Uuid,
        settings: &serde_json::Value,
    ) -> Result<(), Error> {
        sqlx::query!(
            "UPDATE config.source_configs SET settings = $1, updated_at = now() WHERE id = $2",
            settings,
            id,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Update the `schedule_cron` on a source.
    pub async fn update_source_schedule(&self, id: Uuid, cron: &str) -> Result<(), Error> {
        sqlx::query!(
            "UPDATE config.source_configs SET schedule_cron = $1, updated_at = now() WHERE id = $2",
            cron,
            id,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Delete a source configuration. Returns `true` if a row was deleted.
    pub async fn delete_source(&self, id: Uuid) -> Result<bool, Error> {
        let result = sqlx::query!("DELETE FROM config.source_configs WHERE id = $1", id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    /// List the secret keys configured for a source (values are NOT returned).
    pub async fn list_secret_keys(&self, source_id: Uuid) -> Result<Vec<String>, Error> {
        sqlx::query_scalar!(
            "SELECT secret_key FROM config.secrets WHERE source_id = $1",
            source_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))
    }

    /// Get the encrypted value for a specific secret.
    pub async fn get_encrypted_secret(
        &self,
        source_id: Uuid,
        key: &str,
    ) -> Result<Option<Vec<u8>>, Error> {
        sqlx::query_scalar!(
            r#"
            SELECT encrypted_value
            FROM config.secrets
            WHERE source_id = $1
              AND secret_key = $2
            "#,
            source_id,
            key,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))
    }

    /// Insert or update an encrypted secret.
    pub async fn upsert_secret(
        &self,
        id: Uuid,
        source_id: Uuid,
        key: &str,
        encrypted: &[u8],
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO config.secrets (id, source_id, secret_key, encrypted_value)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (source_id, secret_key)
            DO UPDATE SET encrypted_value = $4, updated_at = now()
            "#,
            id,
            source_id,
            key,
            encrypted,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Count source configurations (for backup manifest).
    pub async fn count_sources(&self) -> Result<i64, Error> {
        sqlx::query_scalar!("SELECT COUNT(*) FROM config.source_configs")
            .fetch_one(&self.pool)
            .await
            .map(|c| c.unwrap_or(0))
            .map_err(|e| Error::Database(e.to_string()))
    }

    /// Export all source configurations as JSON rows (for backup).
    pub async fn export_sources(&self) -> Result<Vec<serde_json::Value>, Error> {
        let sources = self.list_sources().await?;

        Ok(sources
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "source_type": s.source_type,
                    "name": s.name,
                    "enabled": s.enabled,
                    "settings": s.settings,
                    "schedule_cron": s.schedule_cron,
                    "created_at": s.created_at.to_string(),
                    "updated_at": s.updated_at.to_string(),
                })
            })
            .collect())
    }
}
