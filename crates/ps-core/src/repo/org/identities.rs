use std::collections::HashMap;

use crate::Error;
use crate::models::Platform;
use uuid::Uuid;

use super::{IdentityRow, OrgRepo};

impl OrgRepo {
    /// Active saved GitHub identities include people without team membership.
    pub async fn active_github_usernames(&self) -> Result<Vec<String>, Error> {
        Ok(self
            .active_discovery_identities(&Platform::Github)
            .await?
            .into_iter()
            .map(|identity| identity.username.to_string())
            .collect())
    }

    /// Saved active accounts eligible for this exact platform/instance, whether
    /// assigned to a team or manually added without a membership.
    pub async fn active_discovery_identities(
        &self,
        platform: &Platform,
    ) -> Result<Vec<crate::ingestion::IdentitySnapshot>, Error> {
        let platform_name = platform.to_string();
        let rows = sqlx::query!(
            r#"
            SELECT pi.id, pi.person_id, pi.platform_username, pi.platform_user_id
            FROM org.platform_identities pi
            JOIN org.people p ON p.id = pi.person_id AND p.active
            WHERE pi.platform = $1
            ORDER BY pi.platform_username, pi.id
            "#,
            platform_name,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;
        Ok(rows
            .into_iter()
            .map(|row| crate::ingestion::IdentitySnapshot {
                identity_id: row.id,
                person_id: row.person_id.into(),
                platform: platform.clone(),
                username: row.platform_username.into(),
                platform_user_id: row.platform_user_id,
            })
            .collect())
    }

    /// Get platform identities for a set of person IDs.
    pub async fn get_identities_for_people(
        &self,
        person_ids: &[Uuid],
    ) -> Result<Vec<IdentityRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, person_id, platform, platform_username, platform_user_id, management AS "management!: crate::models::Management"
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
                id: i.id,
                platform_user_id: i.platform_user_id,
                management: i.management,
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
            SELECT platform_username, min(person_id::text)::uuid AS "person_id!"
            FROM org.platform_identities
            WHERE platform = $1
              AND platform_username = ANY($2)
            GROUP BY platform_username
            HAVING count(DISTINCT person_id) = 1
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

        let mut tx = self.pool.begin().await.map_err(Error::from)?;

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
        .execute(&mut *tx)
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
            ON CONFLICT (platform, platform_username)
                WHERE platform <> 'jira' OR platform_user_id IS NULL
            DO NOTHING
            "#,
            &identity_ids,
            &person_ids,
            &platforms,
            &usernames_for_insert,
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;

        // A concurrent manual claim can win the unique constraint. Resolve
        // actual owners and remove unused provisional people in that case.
        sqlx::query!(
            r#"
            DELETE FROM org.people p
            WHERE p.id = ANY($1)
              AND NOT EXISTS (
                  SELECT 1 FROM org.platform_identities pi WHERE pi.person_id = p.id
              )
            "#,
            &person_ids,
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;

        let owners = sqlx::query!(
            r#"
            SELECT platform_username, person_id
            FROM org.platform_identities
            WHERE platform = $1 AND platform_username = ANY($2)
            "#,
            platform_str,
            &usernames,
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(Error::from)?;

        for owner in owners {
            map.insert(owner.platform_username, owner.person_id);
        }

        tx.commit().await.map_err(Error::from)?;

        Ok(map)
    }
}
