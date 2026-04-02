use crate::Error;
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
}
