use crate::Error;
use uuid::Uuid;

use super::OrgRepo;

/// Map a sqlx row with GitHub team fields into a `GitHubTeamRow` struct.
macro_rules! github_team_row {
    ($row:expr) => {
        GitHubTeamRow {
            id: $row.id,
            source_id: $row.source_id,
            github_org: $row.github_org,
            github_team_id: $row.github_team_id,
            slug: $row.slug,
            name: $row.name,
            description: $row.description,
            member_count: $row.member_count,
            repo_count: $row.repo_count,
        }
    };
}

/// A discovered GitHub team row.
pub struct GitHubTeamRow {
    pub id: Uuid,
    pub source_id: Uuid,
    pub github_org: String,
    pub github_team_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub member_count: i64,
    pub repo_count: i64,
}

/// A suggestion for mapping a GitHub team to a Prism team.
pub struct TeamMappingSuggestion {
    pub github_team_id: Uuid,
    pub github_team_name: String,
    pub github_org: String,
    pub github_team_slug: String,
    pub prism_team_id: Uuid,
    pub prism_team_name: String,
    pub overlap_count: i64,
    pub github_coverage: f64,
    pub prism_coverage: f64,
}

impl OrgRepo {
    /// Upsert a discovered GitHub team.
    pub async fn upsert_github_team(
        &self,
        source_id: Uuid,
        github_org: &str,
        github_team_id: i64,
        slug: &str,
        name: &str,
        description: Option<&str>,
    ) -> Result<Uuid, Error> {
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO org.github_teams (source_id, github_org, github_team_id, slug, name, description, last_synced_at)
            VALUES ($1, $2, $3, $4, $5, $6, now())
            ON CONFLICT (source_id, github_org, slug) DO UPDATE
            SET github_team_id = EXCLUDED.github_team_id,
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                last_synced_at = now()
            RETURNING id
            "#,
            source_id,
            github_org,
            github_team_id,
            slug,
            name,
            description,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(id)
    }

    /// Replace all members for a GitHub team (delete + insert).
    pub async fn replace_github_team_members(
        &self,
        github_team_id: Uuid,
        usernames: &[String],
    ) -> Result<(), Error> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        sqlx::query!(
            "DELETE FROM org.github_team_members WHERE github_team_id = $1",
            github_team_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        for username in usernames {
            sqlx::query!(
                r#"
                INSERT INTO org.github_team_members (github_team_id, github_username, last_synced_at)
                VALUES ($1, $2, now())
                "#,
                github_team_id,
                username,
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Replace all repos for a GitHub team (delete + insert).
    pub async fn replace_github_team_repos(
        &self,
        github_team_id: Uuid,
        repos: &[(String, String)], // (org, repo)
    ) -> Result<(), Error> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        sqlx::query!(
            "DELETE FROM org.github_team_repos WHERE github_team_id = $1",
            github_team_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        for (org, repo) in repos {
            sqlx::query!(
                r#"
                INSERT INTO org.github_team_repos (github_team_id, github_org, github_repo, last_synced_at)
                VALUES ($1, $2, $3, now())
                "#,
                github_team_id,
                org,
                repo,
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// List all discovered GitHub teams, optionally filtered by search term.
    pub async fn list_github_teams(
        &self,
        search: Option<&str>,
        github_org: Option<&str>,
    ) -> Result<Vec<GitHubTeamRow>, Error> {
        let search_pattern = search.map(|s| format!("%{}%", super::super::escape_like(s)));
        let rows = sqlx::query!(
            r#"
            SELECT gt.id, gt.source_id, gt.github_org, gt.github_team_id, gt.slug, gt.name, gt.description,
                   (SELECT COUNT(*) FROM org.github_team_members WHERE github_team_id = gt.id) AS "member_count!",
                   (SELECT COUNT(*) FROM org.github_team_repos WHERE github_team_id = gt.id) AS "repo_count!"
            FROM org.github_teams gt
            WHERE ($1::text IS NULL OR gt.name ILIKE $1 OR gt.slug ILIKE $1)
              AND ($2::text IS NULL OR gt.github_org = $2)
            ORDER BY gt.github_org, gt.name
            "#,
            search_pattern,
            github_org,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows.into_iter().map(|r| github_team_row!(r)).collect())
    }

    /// Assign a GitHub team to a Prism team.
    pub async fn assign_github_team(
        &self,
        team_id: Uuid,
        github_team_id: Uuid,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO org.team_github_team_mappings (team_id, github_team_id)
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
            team_id,
            github_team_id,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Remove a GitHub team mapping from a Prism team.
    pub async fn unassign_github_team(
        &self,
        team_id: Uuid,
        github_team_id: Uuid,
    ) -> Result<(), Error> {
        sqlx::query!(
            "DELETE FROM org.team_github_team_mappings WHERE team_id = $1 AND github_team_id = $2",
            team_id,
            github_team_id,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// List GitHub teams assigned to a Prism team.
    pub async fn list_team_github_teams(&self, team_id: Uuid) -> Result<Vec<GitHubTeamRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT gt.id, gt.source_id, gt.github_org, gt.github_team_id, gt.slug, gt.name, gt.description,
                   (SELECT COUNT(*) FROM org.github_team_members WHERE github_team_id = gt.id) AS "member_count!",
                   (SELECT COUNT(*) FROM org.github_team_repos WHERE github_team_id = gt.id) AS "repo_count!"
            FROM org.github_teams gt
            JOIN org.team_github_team_mappings m ON m.github_team_id = gt.id
            WHERE m.team_id = $1
            ORDER BY gt.github_org, gt.name
            "#,
            team_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows.into_iter().map(|r| github_team_row!(r)).collect())
    }

    /// Get the union of all repos across all mapped GitHub teams for scoped ingestion.
    pub async fn get_mapped_github_team_repos(
        &self,
        source_id: Uuid,
    ) -> Result<Vec<(String, String)>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT gtr.github_org, gtr.github_repo
            FROM org.github_team_repos gtr
            JOIN org.github_teams gt ON gt.id = gtr.github_team_id
            JOIN org.team_github_team_mappings m ON m.github_team_id = gt.id
            WHERE gt.source_id = $1
            "#,
            source_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| (r.github_org, r.github_repo))
            .collect())
    }

    /// Compute overlap-based suggestions for GitHub team → Prism team mappings.
    pub async fn get_team_mapping_suggestions(&self) -> Result<Vec<TeamMappingSuggestion>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                gt.id AS github_team_id,
                gt.name AS github_team_name,
                gt.github_org,
                gt.slug AS github_team_slug,
                t.id AS prism_team_id,
                t.name AS prism_team_name,
                COUNT(*)::bigint AS "overlap_count!",
                COUNT(*)::float / NULLIF(gt_total.total, 0)::float AS "github_coverage!",
                COUNT(*)::float / NULLIF(pt_total.total, 0)::float AS "prism_coverage!"
            FROM org.github_team_members gtm
            JOIN org.platform_identities pi
                ON pi.platform = 'github' AND pi.platform_username = gtm.github_username
            JOIN org.team_memberships tm
                ON tm.person_id = pi.person_id AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            JOIN org.teams t ON t.id = tm.team_id
            JOIN org.github_teams gt ON gt.id = gtm.github_team_id
            CROSS JOIN LATERAL (
                SELECT COUNT(*)::bigint AS total FROM org.github_team_members WHERE github_team_id = gt.id
            ) gt_total
            CROSS JOIN LATERAL (
                SELECT COUNT(*)::bigint AS total FROM org.team_memberships
                WHERE team_id = t.id AND (end_date IS NULL OR end_date > CURRENT_DATE)
            ) pt_total
            -- Exclude already-mapped and dismissed pairs
            WHERE NOT EXISTS (
                SELECT 1 FROM org.team_github_team_mappings m
                WHERE m.team_id = t.id AND m.github_team_id = gt.id
            )
            AND NOT EXISTS (
                SELECT 1 FROM org.dismissed_github_team_suggestions d
                WHERE d.team_id = t.id AND d.github_team_id = gt.id
            )
            GROUP BY gt.id, gt.name, gt.github_org, gt.slug, t.id, t.name, gt_total.total, pt_total.total
            HAVING COUNT(*) > 0
            ORDER BY COUNT(*)::float / NULLIF(gt_total.total, 0)::float DESC NULLS LAST
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| TeamMappingSuggestion {
                github_team_id: r.github_team_id,
                github_team_name: r.github_team_name,
                github_org: r.github_org,
                github_team_slug: r.github_team_slug,
                prism_team_id: r.prism_team_id,
                prism_team_name: r.prism_team_name,
                overlap_count: r.overlap_count,
                github_coverage: r.github_coverage,
                prism_coverage: r.prism_coverage,
            })
            .collect())
    }

    /// Dismiss a suggestion so it doesn't resurface.
    pub async fn dismiss_github_team_suggestion(
        &self,
        team_id: Uuid,
        github_team_id: Uuid,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO org.dismissed_github_team_suggestions (team_id, github_team_id)
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
            team_id,
            github_team_id,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Get distinct GitHub usernames for all active Prism team members.
    ///
    /// Combines two sources:
    /// 1. GitHub team members from mapped teams (via `github_team_members`)
    /// 2. All people with GitHub platform identities who are in any Prism team
    ///    (catches teams that don't have a GitHub team mapping)
    ///
    /// Used by the member search phase to find cross-repo contributions.
    pub async fn get_all_github_team_member_usernames(&self) -> Result<Vec<String>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT username FROM (
                -- People with GitHub platform identities who are active team members
                SELECT pi.platform_username AS username
                FROM org.platform_identities pi
                JOIN org.team_memberships tm ON tm.person_id = pi.person_id
                    AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
                JOIN org.people p ON p.id = pi.person_id AND p.active = true
                WHERE pi.platform = 'github'
            ) all_users
            ORDER BY username
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.username).collect())
    }

    /// Remove stale GitHub teams that weren't seen in the latest sync.
    pub async fn remove_stale_github_teams(
        &self,
        source_id: Uuid,
        github_org: &str,
        current_slugs: &[String],
    ) -> Result<u64, Error> {
        let result = sqlx::query!(
            r#"
            DELETE FROM org.github_teams
            WHERE source_id = $1 AND github_org = $2 AND slug != ALL($3)
            "#,
            source_id,
            github_org,
            current_slugs,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(result.rows_affected())
    }
}
