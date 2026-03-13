use std::collections::HashMap;

use crate::Error;
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
        .map_err(|e| Error::Database(e.to_string()))?;

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
    /// For any username that doesn't have an existing platform identity, a new
    /// person + identity record is auto-created so that contributions can always
    /// be attributed.
    pub async fn batch_resolve_person_ids(
        &self,
        platform: &str,
        usernames: &[String],
    ) -> Result<HashMap<String, Uuid>, Error> {
        if usernames.is_empty() {
            return Ok(HashMap::new());
        }

        let rows = sqlx::query!(
            r#"
            SELECT platform_username, person_id
            FROM org.platform_identities
            WHERE platform = $1
              AND platform_username = ANY($2)
            "#,
            platform,
            usernames,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        let mut map: HashMap<String, Uuid> = rows
            .into_iter()
            .map(|r| (r.platform_username, r.person_id))
            .collect();

        // Auto-create people + identities for unknown usernames
        let missing: Vec<&String> = usernames.iter().filter(|u| !map.contains_key(*u)).collect();
        for username in missing {
            let person_id = Uuid::now_v7();
            let identity_id = Uuid::now_v7();
            sqlx::query!(
                r#"
                INSERT INTO org.people (id, name, active)
                VALUES ($1, $2, true)
                "#,
                person_id,
                username,
            )
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

            sqlx::query!(
                r#"
                INSERT INTO org.platform_identities (id, person_id, platform, platform_username)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (platform, platform_username) DO NOTHING
                "#,
                identity_id,
                person_id,
                platform,
                username,
            )
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

            map.insert(username.clone(), person_id);
        }

        Ok(map)
    }
}
