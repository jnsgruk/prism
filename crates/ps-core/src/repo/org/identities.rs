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
        let rows = sqlx::query!(
            r#"
            SELECT platform_username, person_id
            FROM org.platform_identities
            WHERE platform = $1
              AND platform_username = ANY($2)
            "#,
            platform_str,
            usernames,
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
}
