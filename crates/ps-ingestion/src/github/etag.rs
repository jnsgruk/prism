use sqlx::PgPool;

/// Retrieve a cached `ETag` for a given source + endpoint URL.
pub async fn get_cached_etag(
    pool: &PgPool,
    source_name: &str,
    endpoint_url: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query_scalar!(
        r#"
        SELECT etag
        FROM activity.etag_cache
        WHERE source_name = $1
          AND endpoint_url = $2
        "#,
        source_name,
        endpoint_url,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Store or update a cached `ETag` for a given source + endpoint URL.
pub async fn set_cached_etag(
    pool: &PgPool,
    source_name: &str,
    endpoint_url: &str,
    etag: &str,
) -> Result<(), sqlx::Error> {
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
    .execute(pool)
    .await?;

    Ok(())
}

/// Normalise an endpoint URL for `ETag` caching.
///
/// Strips query parameters that change between runs (like `since`, `page`)
/// so the same logical endpoint maps to the same cache key.
pub fn normalise_endpoint(url: &str) -> String {
    let Some(base) = url.split('?').next() else {
        return url.to_string();
    };
    base.to_string()
}
