use crate::Error;
use crate::models::ResolutionStatus;
use uuid::Uuid;

use super::OrgRepo;

/// A pending resolution row: person + their email for resolution strategies.
pub struct PendingResolution {
    pub person_id: Uuid,
    pub person_name: String,
    pub email: Option<String>,
}

impl OrgRepo {
    /// Ensure `identity_resolutions` rows exist for all active people on a
    /// given platform.  Inserts `pending` rows where missing; existing rows
    /// are left untouched.
    pub async fn ensure_resolution_rows(&self, platform: &str) -> Result<u64, Error> {
        let result = sqlx::query!(
            r#"
            INSERT INTO org.identity_resolutions (person_id, platform)
            SELECT p.id, $1
            FROM org.people p
            WHERE p.active = true
              AND p.last_import_at IS NOT NULL
              AND NOT EXISTS (
                  SELECT 1 FROM org.identity_resolutions ir
                  WHERE ir.person_id = p.id AND ir.platform = $1
              )
            "#,
            platform,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected())
    }

    /// Fetch all pending resolutions for a platform, returning person details
    /// needed by resolution strategies.
    pub async fn get_pending_resolutions(
        &self,
        platform: &str,
    ) -> Result<Vec<PendingResolution>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT ir.person_id, p.name AS person_name, p.email
            FROM org.identity_resolutions ir
            JOIN org.people p ON p.id = ir.person_id
            WHERE ir.platform = $1
              AND ir.status = 'pending'
              AND p.active = true
            ORDER BY p.name
            "#,
            platform,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| PendingResolution {
                person_id: r.person_id,
                person_name: r.person_name,
                email: r.email,
            })
            .collect())
    }

    /// Mark a resolution as resolved and create the platform identity.
    pub async fn resolve_identity(
        &self,
        person_id: Uuid,
        platform: &str,
        platform_username: &str,
    ) -> Result<(), Error> {
        let identity_id = Uuid::now_v7();

        // Create the platform identity.
        sqlx::query!(
            r#"
            INSERT INTO org.platform_identities (id, person_id, platform, platform_username)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (platform, platform_username)
            DO UPDATE SET person_id = EXCLUDED.person_id
            "#,
            identity_id,
            person_id,
            platform,
            platform_username,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        // Update resolution status.
        sqlx::query!(
            r#"
            UPDATE org.identity_resolutions
            SET status = 'resolved', resolved_at = now(), attempted_at = now()
            WHERE person_id = $1 AND platform = $2
            "#,
            person_id,
            platform,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Mark a resolution as unresolved (attempted but no match found).
    pub async fn mark_unresolved(&self, person_id: Uuid, platform: &str) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE org.identity_resolutions
            SET status = 'unresolved', attempted_at = now()
            WHERE person_id = $1 AND platform = $2
            "#,
            person_id,
            platform,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Manually set a platform identity for a person (admin override).
    pub async fn manual_resolve_identity(
        &self,
        person_id: Uuid,
        platform: &str,
        platform_username: &str,
    ) -> Result<(), Error> {
        // Create or update the platform identity.
        let identity_id = Uuid::now_v7();
        sqlx::query!(
            r#"
            INSERT INTO org.platform_identities (id, person_id, platform, platform_username)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (platform, platform_username)
            DO UPDATE SET person_id = EXCLUDED.person_id
            "#,
            identity_id,
            person_id,
            platform,
            platform_username,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        // Upsert resolution status as manual.
        sqlx::query!(
            r#"
            INSERT INTO org.identity_resolutions (person_id, platform, status, resolved_at, attempted_at)
            VALUES ($1, $2, 'manual', now(), now())
            ON CONFLICT (person_id, platform)
            DO UPDATE SET status = 'manual', resolved_at = now(), attempted_at = now()
            "#,
            person_id,
            platform,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Get all existing platform usernames for a person (used for username
    /// probing strategy — try GitHub/Launchpad/etc. usernames on Discourse).
    ///
    /// Excludes email addresses (contain `@`) since they are never valid
    /// usernames on platforms like Discourse or GitHub.
    pub async fn get_candidate_usernames(&self, person_id: Uuid) -> Result<Vec<String>, Error> {
        let rows = sqlx::query_scalar!(
            r#"
            SELECT DISTINCT platform_username
            FROM org.platform_identities
            WHERE person_id = $1
              AND platform_username NOT LIKE '%@%'
            "#,
            person_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows)
    }

    /// Ensure resolution rows exist for all active people across multiple
    /// platforms.  Returns the total number of new rows inserted.
    ///
    /// Platforms are processed concurrently — each `ensure_resolution_rows`
    /// call is an independent INSERT … WHERE NOT EXISTS that doesn't
    /// conflict with other platforms.
    pub async fn ensure_resolution_rows_for_platforms(
        &self,
        platforms: &[String],
    ) -> Result<u64, Error> {
        let mut set = tokio::task::JoinSet::new();
        for platform in platforms {
            let this = self.clone();
            let platform = platform.clone();
            set.spawn(async move { this.ensure_resolution_rows(&platform).await });
        }
        let mut total = 0u64;
        while let Some(result) = set.join_next().await {
            total += result.map_err(|e| Error::Internal(e.to_string()))??;
        }
        Ok(total)
    }

    /// Get resolution statuses for a person across all platforms.
    pub async fn get_resolution_statuses(
        &self,
        person_id: Uuid,
    ) -> Result<Vec<(String, ResolutionStatus)>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT platform, status
            FROM org.identity_resolutions
            WHERE person_id = $1
            ORDER BY platform
            "#,
            person_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                r.status
                    .parse::<ResolutionStatus>()
                    .ok()
                    .map(|s| (r.platform, s))
            })
            .collect())
    }
}
