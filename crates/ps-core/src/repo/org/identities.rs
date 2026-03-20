use std::collections::HashMap;

use crate::Error;
use crate::models::Platform;
use uuid::Uuid;

use super::{IdentityRow, OrgRepo};

impl OrgRepo {
    /// Get platform identities for a set of person IDs.
    pub async fn get_identities_for_people(
        &self,
        person_ids: &[Uuid],
    ) -> Result<Vec<IdentityRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT person_id, platform, platform_username
            FROM org.platform_identities
            WHERE person_id = ANY($1)
            "#,
            person_ids,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|i| IdentityRow {
                person_id: i.person_id,
                platform: i.platform,
                platform_username: i.platform_username,
            })
            .collect())
    }

    /// Batch-resolve platform usernames to person IDs.
    ///
    /// Returns mappings only for usernames that already have a platform identity
    /// configured in the system. Unknown usernames are silently skipped — only
    /// people defined in the app's configuration are tracked.
    pub async fn batch_resolve_person_ids(
        &self,
        platform: &Platform,
        usernames: &[String],
    ) -> Result<HashMap<String, Uuid>, Error> {
        if usernames.is_empty() {
            return Ok(HashMap::new());
        }

        let platform_str = platform.to_string();
        let usernames_lower: Vec<String> = usernames.iter().map(|u| u.to_lowercase()).collect();
        let rows = sqlx::query!(
            r#"
            SELECT platform_username, person_id
            FROM org.platform_identities
            WHERE platform = $1
              AND platform_username = ANY($2)
            "#,
            platform_str,
            &usernames_lower,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let map: HashMap<String, Uuid> = rows
            .into_iter()
            .map(|r| (r.platform_username, r.person_id))
            .collect();

        Ok(map)
    }

    /// Batch-resolve platform user IDs (e.g. Jira `accountId`) to person IDs.
    ///
    /// This resolves against `platform_user_id` instead of `platform_username`,
    /// which is necessary for platforms like Jira where the identifier used in
    /// API responses is an opaque account ID rather than a human-readable username.
    pub async fn batch_resolve_by_user_id(
        &self,
        platform: &Platform,
        user_ids: &[String],
    ) -> Result<HashMap<String, Uuid>, Error> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let platform_str = platform.to_string();
        let rows = sqlx::query!(
            r#"
            SELECT platform_user_id, person_id
            FROM org.platform_identities
            WHERE platform = $1
              AND platform_user_id = ANY($2)
              AND platform_user_id IS NOT NULL
            "#,
            platform_str,
            user_ids,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let map: HashMap<String, Uuid> = rows
            .into_iter()
            .filter_map(|r| r.platform_user_id.map(|uid| (uid, r.person_id)))
            .collect();

        Ok(map)
    }

    /// Auto-create people and platform identities for usernames not yet in the
    /// system.  Returns a complete `username → person_id` map covering both
    /// pre-existing and newly-created identities.
    ///
    /// Used by sources (e.g. Discourse) where the API response is the
    /// authoritative user list — every observed username should have an identity.
    pub async fn batch_ensure_identities(
        &self,
        platform: &Platform,
        users: &[(String, Option<String>)], // (username, display_name)
    ) -> Result<HashMap<String, Uuid>, Error> {
        if users.is_empty() {
            return Ok(HashMap::new());
        }

        // Normalise usernames to lowercase for case-insensitive matching.
        let users_lower: Vec<(String, Option<String>)> = users
            .iter()
            .map(|(u, d)| (u.to_lowercase(), d.clone()))
            .collect();
        let usernames: Vec<String> = users_lower.iter().map(|(u, _)| u.clone()).collect();

        // Resolve existing identities first.
        let mut map = self.batch_resolve_person_ids(platform, &usernames).await?;

        // Collect users that need to be created.
        let new_users: Vec<&(String, Option<String>)> = users_lower
            .iter()
            .filter(|(u, _)| !u.is_empty() && !map.contains_key(u))
            .collect();

        if new_users.is_empty() {
            return Ok(map);
        }

        // Deduplicate by username (in case the batch has duplicates).
        let mut seen = std::collections::HashSet::new();
        let deduped: Vec<&&(String, Option<String>)> = new_users
            .iter()
            .filter(|(u, _)| seen.insert(u.clone()))
            .collect();

        let platform_str = platform.to_string();

        // Batch-create people.
        let person_ids: Vec<Uuid> = deduped.iter().map(|_| Uuid::now_v7()).collect();
        let names: Vec<String> = deduped
            .iter()
            .map(|(username, display_name)| {
                display_name
                    .as_ref()
                    .filter(|n| !n.is_empty())
                    .cloned()
                    .unwrap_or_else(|| username.clone())
            })
            .collect();

        sqlx::query!(
            r#"
            INSERT INTO org.people (id, name)
            SELECT * FROM UNNEST($1::uuid[], $2::text[])
            "#,
            &person_ids,
            &names,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        // Batch-create platform identities.
        let identity_ids: Vec<Uuid> = deduped.iter().map(|_| Uuid::now_v7()).collect();
        let platforms: Vec<String> = deduped.iter().map(|_| platform_str.clone()).collect();
        let usernames_for_insert: Vec<String> = deduped.iter().map(|(u, _)| u.clone()).collect();

        sqlx::query!(
            r#"
            INSERT INTO org.platform_identities (id, person_id, platform, platform_username)
            SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[], $4::text[])
            ON CONFLICT (platform, platform_username) DO NOTHING
            "#,
            &identity_ids,
            &person_ids,
            &platforms,
            &usernames_for_insert,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        // Add newly created mappings to the result.
        for ((username, _), &person_id) in deduped.iter().zip(&person_ids) {
            map.insert(username.clone(), person_id);
        }

        Ok(map)
    }

    /// Import Jira users by matching email addresses to existing people and
    /// creating platform identities with `platform_user_id` set to the Jira
    /// `accountId`.
    ///
    /// Returns `(mapped_count, unmatched_count, warnings)`.
    pub async fn import_jira_users(
        &self,
        records: &[crate::directory::JiraUserRecord],
    ) -> Result<(i32, i32, Vec<String>), Error> {
        if records.is_empty() {
            return Ok((0, 0, vec![]));
        }

        // Collect unique emails for batch lookup
        let emails: Vec<String> = records.iter().map(|r| r.email.to_lowercase()).collect();

        // Look up people by email (case-insensitive)
        let rows = sqlx::query!(
            r#"
            SELECT id, LOWER(email) as "email!"
            FROM org.people
            WHERE LOWER(email) = ANY($1)
              AND active = true
            "#,
            &emails,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let email_to_person: HashMap<String, Uuid> =
            rows.into_iter().map(|r| (r.email, r.id)).collect();

        let mut mapped_count = 0i32;
        let mut unmatched_count = 0i32;
        let mut warnings = Vec::new();

        // Collect matched records for batch upsert
        let mut ids = Vec::new();
        let mut person_ids = Vec::new();
        let mut platforms = Vec::new();
        let mut usernames = Vec::new();
        let mut user_ids = Vec::new();

        for record in records {
            let email_lower = record.email.to_lowercase();
            if let Some(&person_id) = email_to_person.get(&email_lower) {
                ids.push(Uuid::now_v7());
                person_ids.push(person_id);
                platforms.push("jira".to_string());
                usernames.push(record.email.to_lowercase());
                user_ids.push(record.account_id.clone());
                mapped_count += 1;
            } else {
                unmatched_count += 1;
                warnings.push(format!(
                    "No person found for Jira user {} <{}>",
                    record.display_name, record.email
                ));
            }
        }

        // Batch upsert platform identities
        if !person_ids.is_empty() {
            sqlx::query!(
                r#"
                INSERT INTO org.platform_identities (id, person_id, platform, platform_username, platform_user_id)
                SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[], $4::text[], $5::text[])
                ON CONFLICT (platform, platform_username)
                DO UPDATE SET
                    platform_user_id = EXCLUDED.platform_user_id,
                    person_id = EXCLUDED.person_id
                "#,
                &ids,
                &person_ids,
                &platforms,
                &usernames,
                &user_ids as &[String],
            )
            .execute(&self.pool)
            .await
            .map_err(Error::from)?;

            // Backfill person_id on existing Jira contributions whose assignee
            // now has a known identity mapping.
            sqlx::query!(
                r#"
                UPDATE activity.contributions c
                SET person_id = pi.person_id
                FROM org.platform_identities pi
                WHERE c.platform = 'jira'
                  AND c.person_id IS NULL
                  AND pi.platform = 'jira'
                  AND pi.platform_user_id = c.metadata->>'assignee_account_id'
                "#,
            )
            .execute(&self.pool)
            .await
            .map_err(Error::from)?;
        }

        Ok((mapped_count, unmatched_count, warnings))
    }
}
