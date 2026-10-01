use std::collections::HashMap;

use crate::Error;

use super::export_import::{
    build_person_maps, import_github_mappings, import_memberships, import_teams,
    topological_sort_teams, wipe_org_data, wire_team_leads,
};
use super::export_people::{import_identities, import_people};
use crate::models::{Management, ResolutionStatus, TeamType};
use time::OffsetDateTime;
use uuid::Uuid;

use super::OrgRepo;

// ---------------------------------------------------------------------------
// Org export/import types
// ---------------------------------------------------------------------------

/// Full org export document for serialization.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct OrgExport {
    pub version: u32,
    pub exported_at: String,
    pub teams: Vec<ExportTeam>,
    pub people: Vec<ExportPerson>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ExportTeam {
    pub name: String,
    pub org_name: String,
    pub team_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_team: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_email: Option<String>,
    #[serde(default)]
    pub github_teams: Vec<ExportGitHubTeamRef>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ExportGitHubTeamRef {
    pub github_org: String,
    pub slug: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ExportPerson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_import_at: Option<String>,
    #[serde(default)]
    pub membership_management: Management,
    /// Explicit platform choices, including intentionally removed accounts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manual_identity_platforms: Vec<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(default)]
    pub identities: Vec<ExportIdentity>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ExportIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_user_id: Option<String>,
    #[serde(default)]
    pub management: Management,
    pub platform: String,
    pub username: String,
}

/// Result of an org import operation.
pub struct OrgImportResult {
    pub teams_created: i32,
    pub teams_updated: i32,
    pub people_created: i32,
    pub people_updated: i32,
    pub identities_created: i32,
    pub github_mappings_created: i32,
    pub github_mappings_skipped: i32,
    pub warnings: Vec<String>,
}

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

        // Repositories reference teams (no ON DELETE CASCADE).
        sqlx::query!("DELETE FROM org.repositories")
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

    // -----------------------------------------------------------------------
    // Full org export
    // -----------------------------------------------------------------------

    /// Export the complete organisation as a portable JSON-serializable struct.
    pub async fn export_org(&self) -> Result<OrgExport, Error> {
        // 1. Teams with parent name + lead email.
        let team_rows = sqlx::query!(
            r#"
            SELECT t.id, t.name, t.org_name,
                   t.team_type AS "team_type: TeamType",
                   pt.name AS "parent_team_name?",
                   lp.email AS "lead_email?",
                   lp.name AS "lead_name?"
            FROM org.teams t
            LEFT JOIN org.teams pt ON pt.id = t.parent_team_id
            LEFT JOIN org.people lp ON lp.id = t.lead_id
            ORDER BY t.name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        // 2. GitHub team mappings.
        let gh_rows = sqlx::query!(
            r#"
            SELECT t.name AS team_name, t.org_name,
                   gt.github_org, gt.slug
            FROM org.team_github_team_mappings m
            JOIN org.teams t ON t.id = m.team_id
            JOIN org.github_teams gt ON gt.id = m.github_team_id
            ORDER BY t.name, gt.github_org, gt.slug
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        // Group github mappings by (team_name, org_name).
        let mut gh_map: HashMap<(String, String), Vec<ExportGitHubTeamRef>> = HashMap::new();
        for row in &gh_rows {
            gh_map
                .entry((row.team_name.clone(), row.org_name.clone()))
                .or_default()
                .push(ExportGitHubTeamRef {
                    github_org: row.github_org.clone(),
                    slug: row.slug.clone(),
                });
        }

        // Build teams.
        let teams: Vec<ExportTeam> = team_rows
            .iter()
            .map(|t| {
                let key = (t.name.clone(), t.org_name.clone());
                ExportTeam {
                    name: t.name.clone(),
                    org_name: t.org_name.clone(),
                    team_type: t.team_type.to_string(),
                    parent_team: t.parent_team_name.clone(),
                    lead_email: t.lead_email.clone().or_else(|| t.lead_name.clone()),
                    github_teams: gh_map.remove(&key).unwrap_or_default(),
                }
            })
            .collect();

        // 3. People with current team assignment.
        let people_rows = sqlx::query!(
            r#"
            SELECT p.id, p.name, p.email, p.level, p.active, p.directory_id, p.last_import_at,
                   p.membership_management AS "membership_management: Management",
                   t.name AS "team_name?"
            FROM org.people p
            LEFT JOIN org.team_memberships tm ON tm.person_id = p.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.teams t ON t.id = tm.team_id
            ORDER BY p.name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let person_ids: Vec<Uuid> = people_rows.iter().map(|p| p.id).collect();

        // 4. Platform identities.
        let identity_rows = if person_ids.is_empty() {
            vec![]
        } else {
            sqlx::query!(
                r#"
                SELECT person_id, platform, platform_username, platform_user_id, management AS "management: Management"
                FROM org.platform_identities
                WHERE person_id = ANY($1)
                ORDER BY person_id, platform
                "#,
                &person_ids,
            )
            .fetch_all(&self.pool)
            .await
            .map_err(Error::from)?
        };

        let manual = ResolutionStatus::Manual;
        let manual_rows = sqlx::query!(
            "SELECT person_id, platform FROM org.identity_resolutions WHERE person_id = ANY($1) AND status = $2 ORDER BY person_id, platform",
            &person_ids, manual as ResolutionStatus,
        ).fetch_all(&self.pool).await.map_err(Error::from)?;
        let mut manual_map: HashMap<Uuid, Vec<String>> = HashMap::new();
        for row in manual_rows {
            manual_map
                .entry(row.person_id)
                .or_default()
                .push(row.platform);
        }

        // Group identities by person_id.
        let mut id_map: HashMap<Uuid, Vec<ExportIdentity>> = HashMap::new();
        for row in &identity_rows {
            id_map
                .entry(row.person_id)
                .or_default()
                .push(ExportIdentity {
                    platform_user_id: row.platform_user_id.clone(),
                    management: row.management,
                    platform: row.platform.clone(),
                    username: row.platform_username.clone(),
                });
        }

        let people: Vec<ExportPerson> = people_rows
            .iter()
            .map(|p| ExportPerson {
                id: Some(p.id),
                directory_id: p.directory_id.clone(),
                last_import_at: p.last_import_at.and_then(|t| {
                    t.format(&time::format_description::well_known::Rfc3339)
                        .ok()
                }),
                membership_management: p.membership_management,
                manual_identity_platforms: manual_map.remove(&p.id).unwrap_or_default(),
                name: p.name.clone(),
                email: p.email.clone(),
                level: p.level.clone(),
                active: p.active,
                team: p.team_name.clone(),
                identities: id_map.remove(&p.id).unwrap_or_default(),
            })
            .collect();

        Ok(OrgExport {
            version: 1,
            exported_at: OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            teams,
            people,
        })
    }

    // -----------------------------------------------------------------------
    // Full org import
    // -----------------------------------------------------------------------

    /// Import an organisation export. If `replace` is true, wipes all existing
    /// org data first. Otherwise merges without overwriting existing entities.
    pub async fn import_org(
        &self,
        export: &OrgExport,
        replace: bool,
    ) -> Result<OrgImportResult, Error> {
        let mut result = OrgImportResult {
            teams_created: 0,
            teams_updated: 0,
            people_created: 0,
            people_updated: 0,
            identities_created: 0,
            github_mappings_created: 0,
            github_mappings_skipped: 0,
            warnings: Vec::new(),
        };

        let mut tx = self.pool.begin().await.map_err(Error::from)?;

        if replace {
            wipe_org_data(&mut tx).await?;
        }

        let ordered_teams = topological_sort_teams(&export.teams);
        let team_map = import_teams(&mut tx, &ordered_teams, &mut result).await?;
        let resolved = import_people(&mut tx, export, &mut result).await?;
        let maps = build_person_maps(&mut tx, export, &resolved).await?;
        wire_team_leads(
            &mut tx,
            &ordered_teams,
            &team_map,
            &maps,
            &mut result.warnings,
        )
        .await?;
        import_identities(&mut tx, export, &mut result, &resolved).await?;
        import_memberships(&mut tx, export, &team_map, replace, &resolved).await?;
        import_github_mappings(&mut tx, &ordered_teams, &team_map, &mut result).await?;

        tx.commit().await.map_err(Error::from)?;
        Ok(result)
    }
}
