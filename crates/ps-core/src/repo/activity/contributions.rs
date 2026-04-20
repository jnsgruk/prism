use crate::Error;
use crate::backup::ContributionRow;
use crate::ingestion::ContributionInput;
use crate::models::ContributionState;
use uuid::Uuid;

use super::ActivityRepo;

impl ActivityRepo {
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
            &*item.platform.as_cow(),
            item.contribution_type.as_str(),
            item.platform_id.as_str(),
            item.title,
            item.url,
            item.state.map(ContributionState::as_str),
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
        .map_err(Error::from)?;

        Ok(())
    }

    /// Bulk upsert multiple contributions in a single query using UNNEST arrays.
    ///
    /// Returns `(id, platform_id)` pairs for each upserted row so callers can
    /// map contribution IDs back to their enrichment content.
    pub async fn bulk_upsert_contributions(
        &self,
        ids: &[Uuid],
        person_ids: &[Option<Uuid>],
        items: &[&ContributionInput],
    ) -> Result<Vec<(Uuid, String)>, Error> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let platforms: Vec<String> = items.iter().map(|i| i.platform.to_string()).collect();
        let ctypes: Vec<String> = items
            .iter()
            .map(|i| i.contribution_type.to_string())
            .collect();
        let platform_ids: Vec<&str> = items.iter().map(|i| i.platform_id.as_str()).collect();
        let titles: Vec<Option<&str>> = items.iter().map(|i| i.title.as_deref()).collect();
        let urls: Vec<Option<&str>> = items.iter().map(|i| i.url.as_deref()).collect();
        let states: Vec<Option<String>> = items
            .iter()
            .map(|i| i.state.map(|s| s.to_string()))
            .collect();
        let created_ats: Vec<time::OffsetDateTime> = items.iter().map(|i| i.created_at).collect();
        let updated_ats: Vec<Option<time::OffsetDateTime>> =
            items.iter().map(|i| i.updated_at).collect();
        let closed_ats: Vec<Option<time::OffsetDateTime>> =
            items.iter().map(|i| i.closed_at).collect();
        let metrics_vals: Vec<&serde_json::Value> = items.iter().map(|i| &i.metrics).collect();
        let metadata_vals: Vec<&serde_json::Value> = items.iter().map(|i| &i.metadata).collect();
        let contents: Vec<Option<&str>> = items.iter().map(|i| i.content.as_deref()).collect();
        let state_histories: Vec<Option<&serde_json::Value>> =
            items.iter().map(|i| i.state_history.as_ref()).collect();

        let rows = sqlx::query!(
            r#"
            INSERT INTO activity.contributions (
                id, person_id, platform, contribution_type, platform_id,
                title, url, state, created_at, updated_at, closed_at,
                metrics, metadata, content, state_history, ingested_at
            )
            SELECT
                unnest($1::uuid[]),
                unnest($2::uuid[]),
                unnest($3::text[]),
                unnest($4::text[]),
                unnest($5::text[]),
                unnest($6::text[]),
                unnest($7::text[]),
                unnest($8::text[]),
                unnest($9::timestamptz[]),
                unnest($10::timestamptz[]),
                unnest($11::timestamptz[]),
                unnest($12::jsonb[]),
                unnest($13::jsonb[]),
                unnest($14::text[]),
                unnest($15::jsonb[]),
                now()
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
            RETURNING id, platform_id
            "#,
            ids,
            person_ids as &[Option<Uuid>],
            &platforms,
            &ctypes,
            &platform_ids as &[&str],
            &titles as &[Option<&str>],
            &urls as &[Option<&str>],
            &states as &[Option<String>],
            &created_ats,
            &updated_ats as &[Option<time::OffsetDateTime>],
            &closed_ats as &[Option<time::OffsetDateTime>],
            &metrics_vals as &[&serde_json::Value],
            &metadata_vals as &[&serde_json::Value],
            &contents as &[Option<&str>],
            &state_histories as &[Option<&serde_json::Value>],
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows.into_iter().map(|r| (r.id, r.platform_id)).collect())
    }

    /// Look up contribution IDs by platform and `platform_ids`.
    ///
    /// Used to re-enqueue enrichments for contributions whose diffs were
    /// retried after a rate limit sleep.
    pub async fn get_contribution_ids_by_platform_ids(
        &self,
        platform: &str,
        platform_ids: &[String],
    ) -> Result<Vec<(Uuid, String)>, Error> {
        if platform_ids.is_empty() {
            return Ok(vec![]);
        }
        let ids_as_str: Vec<&str> = platform_ids.iter().map(String::as_str).collect();
        let rows = sqlx::query!(
            r#"
            SELECT id, platform_id
            FROM activity.contributions
            WHERE platform = $1
              AND platform_id = ANY($2)
            "#,
            platform,
            &ids_as_str as &[&str],
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows.into_iter().map(|r| (r.id, r.platform_id)).collect())
    }

    /// Backfill `person_id` on Discourse contributions that have a username
    /// stored in `metadata->>'username'` but no `person_id` yet.
    ///
    /// Returns the number of rows updated.
    pub async fn backfill_discourse_person_ids(&self, platform: &str) -> Result<u64, Error> {
        let result = sqlx::query!(
            r#"
            UPDATE activity.contributions c
            SET person_id = pi.person_id
            FROM org.platform_identities pi
            WHERE c.platform = $1
              AND c.person_id IS NULL
              AND pi.platform = $1
              AND pi.platform_username = LOWER(c.metadata->>'username')
            "#,
            platform,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected())
    }

    // -----------------------------------------------------------------------
    // Backup export/import
    // -----------------------------------------------------------------------

    /// Count contributions (for backup manifest).
    pub async fn count_contributions(&self) -> Result<i64, Error> {
        sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!: i64" FROM activity.contributions"#)
            .fetch_one(&self.pool)
            .await
            .map_err(Error::from)
    }

    /// Export all contributions as typed rows for backup.
    ///
    /// Uses keyset pagination to avoid loading the full table into memory.
    /// Returns rows ordered by `id`; caller collects into a `Vec` or streams.
    pub async fn export_contributions(&self) -> Result<Vec<ContributionRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, person_id, platform, contribution_type, platform_id,
                   title, url, state, created_at, updated_at, closed_at,
                   metrics, metadata, content, state_history, ingested_at
            FROM activity.contributions
            ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| ContributionRow {
                id: r.id,
                person_id: r.person_id,
                platform: r.platform,
                contribution_type: r.contribution_type,
                platform_id: r.platform_id,
                title: r.title,
                url: r.url,
                state: r.state,
                created_at: r.created_at,
                updated_at: r.updated_at.map(|t| t.to_string()),
                closed_at: r.closed_at.map(|t| t.to_string()),
                metrics: r.metrics,
                metadata: r.metadata,
                content: r.content,
                state_history: r.state_history,
                ingested_at: r.ingested_at,
            })
            .collect())
    }

    /// Import contributions from backup. Upserts on `(platform, platform_id)`.
    ///
    /// Returns the number of rows upserted.
    pub async fn import_contributions(&self, rows: &[ContributionRow]) -> Result<i64, Error> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mut count: i64 = 0;
        for chunk in rows.chunks(500) {
            let ids: Vec<Uuid> = chunk.iter().map(|r| r.id).collect();
            let person_ids: Vec<Option<Uuid>> = chunk.iter().map(|r| r.person_id).collect();
            let platforms: Vec<&str> = chunk.iter().map(|r| r.platform.as_str()).collect();
            let ctypes: Vec<&str> = chunk.iter().map(|r| r.contribution_type.as_str()).collect();
            let platform_ids: Vec<&str> = chunk.iter().map(|r| r.platform_id.as_str()).collect();
            let titles: Vec<Option<&str>> = chunk.iter().map(|r| r.title.as_deref()).collect();
            let urls: Vec<Option<&str>> = chunk.iter().map(|r| r.url.as_deref()).collect();
            let states: Vec<Option<&str>> = chunk.iter().map(|r| r.state.as_deref()).collect();
            let created_ats: Vec<time::OffsetDateTime> =
                chunk.iter().map(|r| r.created_at).collect();
            let ingested_ats: Vec<time::OffsetDateTime> =
                chunk.iter().map(|r| r.ingested_at).collect();
            let metrics: Vec<&serde_json::Value> = chunk.iter().map(|r| &r.metrics).collect();
            let metadata: Vec<&serde_json::Value> = chunk.iter().map(|r| &r.metadata).collect();
            let contents: Vec<Option<&str>> = chunk.iter().map(|r| r.content.as_deref()).collect();
            let state_histories: Vec<Option<&serde_json::Value>> =
                chunk.iter().map(|r| r.state_history.as_ref()).collect();

            sqlx::query!(
                r#"
                INSERT INTO activity.contributions (
                    id, person_id, platform, contribution_type, platform_id,
                    title, url, state, created_at, metrics, metadata,
                    content, state_history, ingested_at
                )
                SELECT
                    unnest($1::uuid[]),
                    unnest($2::uuid[]),
                    unnest($3::text[]),
                    unnest($4::text[]),
                    unnest($5::text[]),
                    unnest($6::text[]),
                    unnest($7::text[]),
                    unnest($8::text[]),
                    unnest($9::timestamptz[]),
                    unnest($10::jsonb[]),
                    unnest($11::jsonb[]),
                    unnest($12::text[]),
                    unnest($13::jsonb[]),
                    unnest($14::timestamptz[])
                ON CONFLICT (platform, platform_id) DO UPDATE SET
                    person_id    = COALESCE(EXCLUDED.person_id, activity.contributions.person_id),
                    title        = EXCLUDED.title,
                    url          = EXCLUDED.url,
                    state        = EXCLUDED.state,
                    metrics      = EXCLUDED.metrics,
                    metadata     = EXCLUDED.metadata,
                    content      = EXCLUDED.content,
                    state_history = EXCLUDED.state_history,
                    ingested_at  = EXCLUDED.ingested_at
                "#,
                &ids,
                &person_ids as &[Option<Uuid>],
                &platforms as &[&str],
                &ctypes as &[&str],
                &platform_ids as &[&str],
                &titles as &[Option<&str>],
                &urls as &[Option<&str>],
                &states as &[Option<&str>],
                &created_ats,
                &metrics as &[&serde_json::Value],
                &metadata as &[&serde_json::Value],
                &contents as &[Option<&str>],
                &state_histories as &[Option<&serde_json::Value>],
                &ingested_ats,
            )
            .execute(&self.pool)
            .await
            .map_err(Error::from)?;

            count += i64::try_from(chunk.len()).unwrap_or(i64::MAX);
        }
        Ok(count)
    }
}
