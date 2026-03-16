use crate::Error;
use crate::models::TeamType;
use uuid::Uuid;

use super::OrgRepo;

impl OrgRepo {
    /// Upsert a repository record.
    pub async fn upsert_repository(
        &self,
        id: Uuid,
        github_org: &str,
        github_repo: &str,
        default_branch: Option<&str>,
        primary_language: Option<&str>,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO org.repositories (id, github_org, github_repo, default_branch, primary_language)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (github_org, github_repo)
            DO UPDATE SET
                default_branch = COALESCE(EXCLUDED.default_branch, org.repositories.default_branch),
                primary_language = COALESCE(EXCLUDED.primary_language, org.repositories.primary_language)
            "#,
            id,
            github_org,
            github_repo,
            default_branch,
            primary_language,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Batch upsert multiple repository records using UNNEST arrays.
    pub async fn bulk_upsert_repositories(
        &self,
        ids: &[Uuid],
        github_orgs: &[&str],
        github_repos: &[&str],
        default_branches: &[Option<&str>],
        primary_languages: &[Option<&str>],
    ) -> Result<(), Error> {
        if ids.is_empty() {
            return Ok(());
        }
        // Convert Option<&str> slices to Option<String> vecs for sqlx binding
        let branches: Vec<Option<String>> = default_branches
            .iter()
            .map(|b| b.map(String::from))
            .collect();
        let languages: Vec<Option<String>> = primary_languages
            .iter()
            .map(|l| l.map(String::from))
            .collect();
        let orgs: Vec<String> = github_orgs.iter().map(|s| (*s).to_string()).collect();
        let repos: Vec<String> = github_repos.iter().map(|s| (*s).to_string()).collect();

        sqlx::query!(
            r#"
            INSERT INTO org.repositories (id, github_org, github_repo, default_branch, primary_language)
            SELECT * FROM UNNEST($1::uuid[], $2::text[], $3::text[], $4::text[], $5::text[])
            ON CONFLICT (github_org, github_repo)
            DO UPDATE SET
                default_branch = COALESCE(EXCLUDED.default_branch, org.repositories.default_branch),
                primary_language = COALESCE(EXCLUDED.primary_language, org.repositories.primary_language)
            "#,
            ids,
            &orgs,
            &repos,
            &branches as &[Option<String>],
            &languages as &[Option<String>],
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Count people (for backup manifest).
    pub async fn count_people(&self) -> Result<i64, Error> {
        sqlx::query_scalar!("SELECT COUNT(*) FROM org.people")
            .fetch_one(&self.pool)
            .await
            .map(|c| c.unwrap_or(0))
            .map_err(Error::from)
    }

    /// Count teams (for backup manifest).
    pub async fn count_teams(&self) -> Result<i64, Error> {
        sqlx::query_scalar!("SELECT COUNT(*) FROM org.teams")
            .fetch_one(&self.pool)
            .await
            .map(|c| c.unwrap_or(0))
            .map_err(Error::from)
    }

    /// Export all people as JSON rows (for backup).
    pub async fn export_people(&self) -> Result<Vec<serde_json::Value>, Error> {
        let people = sqlx::query!(
            "SELECT id, name, email, level, directory_id, created_at, updated_at FROM org.people"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(people
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "email": p.email,
                    "level": p.level,
                    "directory_id": p.directory_id,
                    "created_at": p.created_at.to_string(),
                    "updated_at": p.updated_at.to_string(),
                })
            })
            .collect())
    }

    /// Export all teams as JSON rows (for backup).
    pub async fn export_teams(&self) -> Result<Vec<serde_json::Value>, Error> {
        let teams = sqlx::query!(
            r#"SELECT id, name, org_name, parent_team_id, lead_id,
                      team_type AS "team_type: TeamType", created_at
               FROM org.teams"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(teams
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "name": t.name,
                    "org_name": t.org_name,
                    "parent_team_id": t.parent_team_id,
                    "lead_id": t.lead_id,
                    "team_type": t.team_type.to_string(),
                    "created_at": t.created_at.to_string(),
                })
            })
            .collect())
    }

    /// Delete all org data: memberships, identities, people, teams.
    /// Returns (`people_deleted`, `teams_deleted`).
    pub async fn reset_all(&self) -> Result<(i64, i64), Error> {
        let mut tx = self.pool.begin().await.map_err(Error::from)?;

        // Order matters: children first due to foreign keys.
        sqlx::query!("DELETE FROM org.team_memberships")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        sqlx::query!("DELETE FROM org.platform_identities")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        // Clear lead_id references before deleting people.
        sqlx::query!("UPDATE org.teams SET lead_id = NULL")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        let people = sqlx::query!("DELETE FROM org.people")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        let teams = sqlx::query!("DELETE FROM org.teams")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        tx.commit().await.map_err(Error::from)?;

        Ok((
            people.rows_affected().cast_signed(),
            teams.rows_affected().cast_signed(),
        ))
    }
}
