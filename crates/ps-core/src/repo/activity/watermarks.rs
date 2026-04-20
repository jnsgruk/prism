use crate::Error;
use crate::backup::WatermarkRow;

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

    // -----------------------------------------------------------------------
    // Backup export/import
    // -----------------------------------------------------------------------

    /// Export all watermark rows for backup.
    pub async fn export_watermarks(&self) -> Result<Vec<WatermarkRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT source_name, watermark_value, last_successful_run, last_attempt,
                   last_error, items_collected_last_run
            FROM activity.ingestion_watermarks
            ORDER BY source_name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| WatermarkRow {
                source_name: r.source_name,
                watermark_value: r.watermark_value,
                last_successful_run: r.last_successful_run.map(|t| t.to_string()),
                last_attempt: r.last_attempt.map(|t| t.to_string()),
                last_error: r.last_error,
                items_collected_last_run: r.items_collected_last_run,
            })
            .collect())
    }

    /// Count watermark rows (for backup manifest).
    pub async fn count_watermarks(&self) -> Result<i64, Error> {
        sqlx::query_scalar!("SELECT COUNT(*) FROM activity.ingestion_watermarks")
            .fetch_one(&self.pool)
            .await
            .map(|c| c.unwrap_or(0))
            .map_err(Error::from)
    }

    /// Import watermarks from backup. Upserts on `source_name`.
    pub async fn import_watermarks(&self, rows: &[WatermarkRow]) -> Result<i64, Error> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mut count: i64 = 0;
        for chunk in rows.chunks(1000) {
            let names: Vec<&str> = chunk.iter().map(|r| r.source_name.as_str()).collect();
            let values: Vec<&str> = chunk.iter().map(|r| r.watermark_value.as_str()).collect();

            sqlx::query!(
                r#"
                INSERT INTO activity.ingestion_watermarks (source_name, watermark_value)
                SELECT unnest($1::text[]), unnest($2::text[])
                ON CONFLICT (source_name) DO UPDATE
                    SET watermark_value = EXCLUDED.watermark_value
                "#,
                &names as &[&str],
                &values as &[&str],
            )
            .execute(&self.pool)
            .await
            .map_err(Error::from)?;

            count += i64::try_from(chunk.len()).unwrap_or(i64::MAX);
        }
        Ok(count)
    }
}
