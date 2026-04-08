use crate::Error;
use crate::models::{AiModel, AiProvider, GlobalSetting, Platform, SourceConfig};
use sqlx::PgPool;
use uuid::Uuid;

/// Repository for the `config` schema: source configurations and encrypted secrets.
#[derive(Clone)]
pub struct ConfigRepo {
    pool: PgPool,
}

/// Map a query row to `SourceConfig`, parsing `source_type` from its TEXT column.
///
/// Handles bare `"discourse"` by deriving the instance suffix from the source
/// name (e.g. source named "Ubuntu" → `Platform::Discourse("ubuntu")`).
/// Falls back to `Platform::Github` for truly unknown values with a warning.
macro_rules! map_source_config {
    ($r:expr) => {{
        let source_type = Platform::from_str_opt(&$r.source_type).unwrap_or_else(|| {
            // Handle bare "discourse" stored before the instance-qualifying fix.
            if $r.source_type == "discourse" {
                let slug = $r
                    .name
                    .to_lowercase()
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                    .collect::<String>()
                    .split('-')
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("-");
                Platform::Discourse(slug)
            } else {
                tracing::warn!(
                    source_type = %$r.source_type,
                    source_name = %$r.name,
                    "unrecognised source_type in DB — defaulting to Github"
                );
                Platform::Github
            }
        });
        SourceConfig {
            id: crate::models::SourceId::new($r.id),
            source_type,
            name: $r.name,
            enabled: $r.enabled,
            settings: $r.settings,
            schedule_cron: $r.schedule_cron,
            created_at: $r.created_at,
            updated_at: $r.updated_at,
        }
    }};
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
        let rows = sqlx::query!(
            r#"
            SELECT id, source_type, name, enabled, settings, schedule_cron,
                   created_at, updated_at
            FROM config.source_configs
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows.into_iter().map(|r| map_source_config!(r)).collect())
    }

    /// Get a single source configuration by ID.
    pub async fn get_source(&self, id: Uuid) -> Result<Option<SourceConfig>, Error> {
        let row = sqlx::query!(
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
        .map_err(Error::from)?;

        Ok(row.map(|r| map_source_config!(r)))
    }

    /// Get an enabled source configuration by name.
    pub async fn get_enabled_source_by_name(
        &self,
        name: &str,
    ) -> Result<Option<SourceConfig>, Error> {
        let row = sqlx::query!(
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
        .map_err(Error::from)?;

        Ok(row.map(|r| map_source_config!(r)))
    }

    /// Get an enabled source configuration by source type (e.g. "github", "discourse-ubuntu").
    ///
    /// Used by Restate workers where the virtual object key is the source type.
    pub async fn get_enabled_source_by_type(
        &self,
        source_type: &str,
    ) -> Result<Option<SourceConfig>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT id, source_type, name, enabled, settings, schedule_cron,
                   created_at, updated_at
            FROM config.source_configs
            WHERE source_type = $1 AND enabled = true
            "#,
            source_type,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row.map(|r| map_source_config!(r)))
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
        let row = sqlx::query!(
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
            if e.as_database_error()
                .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
            {
                Error::Conflict(format!("source '{name}' already exists"))
            } else {
                Error::from(e)
            }
        })?;

        Ok(map_source_config!(row))
    }

    /// Check whether a source exists by ID.
    pub async fn source_exists(&self, id: Uuid) -> Result<bool, Error> {
        let row = sqlx::query_scalar!("SELECT id FROM config.source_configs WHERE id = $1", id,)
            .fetch_optional(&self.pool)
            .await
            .map_err(Error::from)?;

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
        .map_err(Error::from)?;

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
        .map_err(Error::from)?;

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
        .map_err(Error::from)?;

        Ok(())
    }

    /// Delete a source configuration. Returns `true` if a row was deleted.
    pub async fn delete_source(&self, id: Uuid) -> Result<bool, Error> {
        let result = sqlx::query!("DELETE FROM config.source_configs WHERE id = $1", id)
            .execute(&self.pool)
            .await
            .map_err(Error::from)?;

        Ok(result.rows_affected() > 0)
    }

    /// List all secret keys grouped by source ID. Used to avoid N+1 queries
    /// when listing sources.
    pub async fn list_all_secret_keys(
        &self,
    ) -> Result<std::collections::HashMap<Uuid, Vec<String>>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT source_id as "source_id!: Uuid", secret_key
            FROM config.secrets
            WHERE source_id IS NOT NULL
            ORDER BY source_id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let mut map = std::collections::HashMap::<Uuid, Vec<String>>::new();
        for r in rows {
            map.entry(r.source_id).or_default().push(r.secret_key);
        }
        Ok(map)
    }

    /// List the secret keys configured for a source (values are NOT returned).
    pub async fn list_secret_keys(&self, source_id: Uuid) -> Result<Vec<String>, Error> {
        sqlx::query_scalar!(
            "SELECT secret_key FROM config.secrets WHERE source_id = $1",
            source_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)
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
        .map_err(Error::from)
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
            ON CONFLICT (source_id, secret_key) WHERE source_id IS NOT NULL
            DO UPDATE SET encrypted_value = $4, updated_at = now()
            "#,
            id,
            source_id,
            key,
            encrypted,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Count source configurations (for backup manifest).
    pub async fn count_sources(&self) -> Result<i64, Error> {
        sqlx::query_scalar!("SELECT COUNT(*) FROM config.source_configs")
            .fetch_one(&self.pool)
            .await
            .map(|c| c.unwrap_or(0))
            .map_err(Error::from)
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

    // -----------------------------------------------------------------------
    // Global secrets (source_id IS NULL)
    // -----------------------------------------------------------------------

    /// List all global secret keys (not tied to any source).
    pub async fn list_global_secret_keys(&self) -> Result<Vec<String>, Error> {
        sqlx::query_scalar!("SELECT secret_key FROM config.secrets WHERE source_id IS NULL")
            .fetch_all(&self.pool)
            .await
            .map_err(Error::from)
    }

    /// Get the encrypted value for a global secret.
    pub async fn get_global_secret(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        sqlx::query_scalar!(
            r#"
            SELECT encrypted_value
            FROM config.secrets
            WHERE source_id IS NULL
              AND secret_key = $1
            "#,
            key,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)
    }

    /// Insert or update a global encrypted secret (`source_id` = NULL).
    pub async fn upsert_global_secret(
        &self,
        id: Uuid,
        key: &str,
        encrypted: &[u8],
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO config.secrets (id, source_id, secret_key, encrypted_value)
            VALUES ($1, NULL, $2, $3)
            ON CONFLICT (secret_key) WHERE source_id IS NULL
            DO UPDATE SET encrypted_value = $3, updated_at = now()
            "#,
            id,
            key,
            encrypted,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Delete a global secret by key.
    pub async fn delete_global_secret(&self, key: &str) -> Result<bool, Error> {
        let result = sqlx::query!(
            "DELETE FROM config.secrets WHERE source_id IS NULL AND secret_key = $1",
            key,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected() > 0)
    }

    // -----------------------------------------------------------------------
    // Global settings (config.global_settings)
    // -----------------------------------------------------------------------

    /// Get a single global setting by key.
    pub async fn get_global_setting(&self, key: &str) -> Result<Option<GlobalSetting>, Error> {
        let row = sqlx::query_as!(
            GlobalSetting,
            r#"
            SELECT key, value, updated_at
            FROM config.global_settings
            WHERE key = $1
            "#,
            key,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row)
    }

    /// List all global settings matching a key prefix (e.g. `"ai."` for all AI settings).
    pub async fn list_global_settings(&self, prefix: &str) -> Result<Vec<GlobalSetting>, Error> {
        let pattern = format!("{prefix}%");
        sqlx::query_as!(
            GlobalSetting,
            r#"
            SELECT key, value, updated_at
            FROM config.global_settings
            WHERE key LIKE $1
            ORDER BY key
            "#,
            pattern,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)
    }

    /// Upsert a global setting.
    pub async fn set_global_setting(
        &self,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO config.global_settings (key, value)
            VALUES ($1, $2)
            ON CONFLICT (key)
            DO UPDATE SET value = $2, updated_at = now()
            "#,
            key,
            value,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Delete a global setting by key.
    pub async fn delete_global_setting(&self, key: &str) -> Result<bool, Error> {
        let result = sqlx::query!("DELETE FROM config.global_settings WHERE key = $1", key,)
            .execute(&self.pool)
            .await
            .map_err(Error::from)?;

        Ok(result.rows_affected() > 0)
    }

    // -----------------------------------------------------------------------
    // AI model catalogue (config.ai_models)
    // -----------------------------------------------------------------------

    /// Replace all cached models for a provider (full refresh).
    ///
    /// Deletes existing entries and inserts the new list in a single transaction
    /// so deprecated/removed models are cleaned up automatically.
    pub async fn replace_ai_models(&self, provider: &str, models: &[AiModel]) -> Result<(), Error> {
        let mut tx = self.pool.begin().await.map_err(Error::from)?;

        sqlx::query!("DELETE FROM config.ai_models WHERE provider = $1", provider)
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        // Insert models individually within the transaction. Model lists are
        // small (~50-200 per provider) so per-row inserts are fine here; the
        // UNNEST pattern doesn't work well with TEXT[] columns in sqlx.
        for m in models {
            sqlx::query!(
                r#"
                INSERT INTO config.ai_models
                    (id, provider, display_name, description, context_length,
                     input_price, output_price, capabilities)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                "#,
                m.id,
                provider,
                m.display_name,
                m.description.as_deref(),
                m.context_length,
                m.input_price,
                m.output_price,
                &m.capabilities,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        }

        tx.commit().await.map_err(Error::from)?;
        Ok(())
    }

    /// List cached models, optionally filtered by provider and/or capability.
    pub async fn list_ai_models(
        &self,
        provider: Option<&str>,
        capability: Option<&str>,
    ) -> Result<Vec<AiModel>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, provider, display_name, description, context_length,
                   input_price, output_price, capabilities
            FROM config.ai_models
            WHERE ($1::text IS NULL OR provider = $1)
              AND ($2::text IS NULL OR $2 = ANY(capabilities))
            ORDER BY provider, display_name
            "#,
            provider,
            capability,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        rows.into_iter()
            .map(|r| {
                Ok(AiModel {
                    id: r.id,
                    provider: r.provider.parse::<AiProvider>().map_err(|e| {
                        Error::Internal(format!("invalid provider in ai_models: {e}"))
                    })?,
                    display_name: r.display_name,
                    description: r.description,
                    context_length: r.context_length,
                    input_price: r.input_price,
                    output_price: r.output_price,
                    capabilities: r.capabilities,
                })
            })
            .collect()
    }

    /// Return the on-disk size of the current `PostgreSQL` database in bytes.
    pub async fn database_size_bytes(&self) -> Result<i64, Error> {
        let row = sqlx::query!("SELECT pg_database_size(current_database()) AS size")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.size.unwrap_or(0))
    }
}
