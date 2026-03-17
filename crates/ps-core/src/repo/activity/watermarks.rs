use crate::Error;

use super::ActivityRepo;

impl ActivityRepo {
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
        .map_err(Error::from)
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
        .map_err(Error::from)?;

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
        .map_err(Error::from)
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
        .map_err(Error::from)?;

        Ok(())
    }
}
