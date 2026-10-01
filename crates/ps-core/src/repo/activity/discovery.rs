use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Error, ingestion::IdentitySnapshot};

use super::ActivityRepo;

impl ActivityRepo {
    /// A source-wide watermark cannot establish complete user discovery.
    pub async fn identity_discovery_cutoff(
        &self,
        source_id: Uuid,
        identity_id: Uuid,
        version: &str,
    ) -> Result<Option<OffsetDateTime>, Error> {
        sqlx::query_scalar!(
            r#"
            SELECT covered_through
            FROM activity.identity_discovery_coverage
            WHERE source_id = $1 AND identity_id = $2 AND identity_version = $3
            "#,
            source_id,
            identity_id,
            version,
        )
        .fetch_optional(&self.pool)
        .await
        .map(Option::flatten)
        .map_err(Error::from)
    }

    /// First-attempt lower bound, retained independently of completed coverage.
    pub async fn identity_discovery_initial_since(
        &self,
        source_id: Uuid,
        identity_id: Uuid,
        version: &str,
    ) -> Result<Option<OffsetDateTime>, Error> {
        sqlx::query_scalar!(
            r#"
            SELECT initial_since
            FROM activity.identity_discovery_coverage
            WHERE source_id = $1
                AND identity_id = $2
                AND identity_version = $3
            "#,
            source_id,
            identity_id,
            version,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)
    }

    /// Journalled planning freezes the first window before any user API call.
    /// Bulk insertion validates active saved accounts and exact source bounds;
    /// an unchanged version retains its baseline and completed cutoff.
    pub async fn begin_identity_discovery(
        &self,
        source_id: Uuid,
        targets: &[(IdentitySnapshot, String)],
        initial_since: OffsetDateTime,
    ) -> Result<(), Error> {
        if targets.is_empty() {
            return Ok(());
        }

        let ids: Vec<_> = targets
            .iter()
            .map(|(identity, _)| identity.identity_id)
            .collect();
        let people: Vec<_> = targets
            .iter()
            .map(|(identity, _)| identity.person_id.into_inner())
            .collect();
        let platforms: Vec<_> = targets
            .iter()
            .map(|(identity, _)| identity.platform.to_string())
            .collect();
        let usernames: Vec<_> = targets
            .iter()
            .map(|(identity, _)| identity.username.to_string())
            .collect();
        let user_ids: Vec<_> = targets
            .iter()
            .map(|(identity, _)| identity.platform_user_id.clone())
            .collect();
        let versions: Vec<_> = targets.iter().map(|(_, version)| version.clone()).collect();

        sqlx::query!(
            r#"
            INSERT INTO activity.identity_discovery_coverage
                (source_id, identity_id, identity_version, initial_since)
            SELECT $1, input.identity_id, input.version, $2
            FROM UNNEST($3::uuid[], $4::uuid[], $5::text[], $6::text[], $7::text[], $8::text[])
                AS input(identity_id, person_id, platform, username, user_id, version)
            JOIN org.platform_identities pi ON pi.id = input.identity_id
                AND pi.person_id = input.person_id
                AND pi.platform = input.platform
                AND pi.platform_username = input.username
                AND pi.platform_user_id IS NOT DISTINCT FROM input.user_id
            JOIN org.people p ON p.id = pi.person_id AND p.active
            JOIN config.source_configs source ON source.id = $1 AND source.enabled
                AND source.source_type = pi.platform
            ON CONFLICT (source_id, identity_id) DO UPDATE
            SET initial_since = CASE
                    WHEN identity_discovery_coverage.identity_version = EXCLUDED.identity_version
                    THEN identity_discovery_coverage.initial_since
                    ELSE EXCLUDED.initial_since
                END,
                covered_through = CASE
                    WHEN identity_discovery_coverage.identity_version = EXCLUDED.identity_version
                    THEN identity_discovery_coverage.covered_through
                    ELSE NULL
                END,
                identity_version = EXCLUDED.identity_version,
                updated_at = now()
            "#,
            source_id,
            initial_since,
            &ids,
            &people,
            &platforms,
            &usernames,
            &user_ids as &[Option<String>],
            &versions,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Call only after all pages and their stores succeeded. Account changes
    /// or deactivation during collection invalidate this frozen target.
    pub async fn advance_identity_discovery(
        &self,
        source_id: Uuid,
        identity: &IdentitySnapshot,
        version: &str,
        cutoff: OffsetDateTime,
    ) -> Result<(), Error> {
        let platform = identity.platform.to_string();
        sqlx::query!(
            r#"
            INSERT INTO activity.identity_discovery_coverage
                (source_id, identity_id, identity_version, covered_through, initial_since)
            SELECT $1, pi.id, $2, $3, $3
            FROM org.platform_identities pi
            JOIN org.people p ON p.id = pi.person_id AND p.active
            WHERE pi.id = $4 AND pi.person_id = $5 AND pi.platform = $6
              AND pi.platform_username = $7
              AND pi.platform_user_id IS NOT DISTINCT FROM $8
            ON CONFLICT (source_id, identity_id) DO UPDATE
            SET initial_since = CASE
                    WHEN identity_discovery_coverage.identity_version = EXCLUDED.identity_version
                    THEN identity_discovery_coverage.initial_since
                    ELSE EXCLUDED.initial_since
                END,
                identity_version = EXCLUDED.identity_version,
                covered_through = CASE
                    WHEN identity_discovery_coverage.identity_version = EXCLUDED.identity_version
                    THEN greatest(identity_discovery_coverage.covered_through, EXCLUDED.covered_through)
                    ELSE EXCLUDED.covered_through
                END,
                updated_at = now()
            "#,
            source_id,
            version,
            cutoff,
            identity.identity_id,
            identity.person_id.as_uuid(),
            platform,
            identity.username.as_str(),
            identity.platform_user_id,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;
        Ok(())
    }
}
