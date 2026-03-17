use crate::Error;

use super::ActivityRepo;

impl ActivityRepo {
    /// Store the Restate invocation ID for a running ingestion.
    pub async fn set_current_invocation_id(
        &self,
        source_name: &str,
        invocation_id: &str,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO activity.ingestion_watermarks (source_name, watermark_value, current_invocation_id)
            VALUES ($1, '', $2)
            ON CONFLICT (source_name)
            DO UPDATE SET current_invocation_id = $2
            "#,
            source_name,
            invocation_id,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

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
        .map_err(Error::from)?
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
        .map_err(Error::from)?;

        Ok(())
    }
}
