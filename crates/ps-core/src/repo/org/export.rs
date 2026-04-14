use std::collections::HashMap;

use crate::Error;
use crate::models::TeamType;
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
            SELECT p.id, p.name, p.email, p.level, p.active,
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
                SELECT person_id, platform, platform_username
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

        // Group identities by person_id.
        let mut id_map: HashMap<Uuid, Vec<ExportIdentity>> = HashMap::new();
        for row in &identity_rows {
            id_map
                .entry(row.person_id)
                .or_default()
                .push(ExportIdentity {
                    platform: row.platform.clone(),
                    username: row.platform_username.clone(),
                });
        }

        let people: Vec<ExportPerson> = people_rows
            .iter()
            .map(|p| ExportPerson {
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
        import_people(&mut tx, export, &mut result).await?;
        wire_team_leads(&mut tx, &ordered_teams, &team_map, &mut result.warnings).await?;
        import_identities(&mut tx, export, &mut result.identities_created).await?;
        import_memberships(&mut tx, export, &team_map, replace).await?;
        import_github_mappings(&mut tx, &ordered_teams, &team_map, &mut result).await?;

        tx.commit().await.map_err(Error::from)?;
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Import helper types and functions
// ---------------------------------------------------------------------------

/// Lookup maps for resolving people by email or name.
struct PersonMaps {
    by_email: HashMap<String, Uuid>,
    by_name: HashMap<String, Uuid>,
}

fn resolve_person_id(maps: &PersonMaps, email: Option<&str>, name: &str) -> Option<Uuid> {
    email
        .and_then(|e| maps.by_email.get(&e.to_lowercase()).copied())
        .or_else(|| maps.by_name.get(name).copied())
}

async fn wipe_org_data(tx: &mut sqlx::PgConnection) -> Result<(), Error> {
    sqlx::query!("DELETE FROM org.team_memberships")
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;
    sqlx::query!("DELETE FROM org.platform_identities")
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;
    sqlx::query!("UPDATE org.teams SET lead_id = NULL")
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;
    sqlx::query!("DELETE FROM org.people")
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;
    sqlx::query!("DELETE FROM org.team_github_team_mappings")
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;
    sqlx::query!("DELETE FROM org.teams")
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;
    Ok(())
}

async fn import_teams(
    tx: &mut sqlx::PgConnection,
    ordered_teams: &[&ExportTeam],
    result: &mut OrgImportResult,
) -> Result<HashMap<(String, String), Uuid>, Error> {
    let mut team_map: HashMap<(String, String), Uuid> = HashMap::new();

    for team in ordered_teams {
        let existing = sqlx::query_scalar!(
            "SELECT id FROM org.teams WHERE name = $1 AND org_name = $2",
            team.name,
            team.org_name,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::from)?;

        let team_id = if let Some(id) = existing {
            result.teams_updated += 1;
            id
        } else {
            let id = Uuid::now_v7();
            let team_type = parse_team_type(&team.team_type);
            sqlx::query!(
                r#"
                INSERT INTO org.teams (id, name, org_name, team_type)
                VALUES ($1, $2, $3, $4::org.team_type)
                "#,
                id,
                team.name,
                team.org_name,
                team_type as TeamType,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
            result.teams_created += 1;
            id
        };

        team_map.insert((team.name.clone(), team.org_name.clone()), team_id);
    }

    // Wire parent_team_id.
    for team in ordered_teams {
        if let Some(parent_name) = &team.parent_team {
            let key = (team.name.clone(), team.org_name.clone());
            let Some(&team_id) = team_map.get(&key) else {
                continue;
            };
            let parent_key = (parent_name.clone(), team.org_name.clone());
            if let Some(&parent_id) = team_map.get(&parent_key) {
                sqlx::query!(
                    "UPDATE org.teams SET parent_team_id = $1 WHERE id = $2",
                    parent_id,
                    team_id,
                )
                .execute(&mut *tx)
                .await
                .map_err(Error::from)?;
            } else {
                result.warnings.push(format!(
                    "Parent team '{}' not found for '{}'",
                    parent_name, team.name
                ));
            }
        }
    }

    Ok(team_map)
}

async fn import_people(
    tx: &mut sqlx::PgConnection,
    export: &OrgExport,
    result: &mut OrgImportResult,
) -> Result<(), Error> {
    for person in &export.people {
        let existing = if let Some(email) = &person.email {
            sqlx::query_scalar!(
                "SELECT id FROM org.people WHERE LOWER(email) = LOWER($1)",
                email,
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(Error::from)?
        } else {
            None
        };

        let existing = if existing.is_some() {
            existing
        } else {
            let rows =
                sqlx::query_scalar!("SELECT id FROM org.people WHERE name = $1", person.name)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(Error::from)?;

            if rows.len() > 1 {
                result.warnings.push(format!(
                    "Ambiguous name match for '{}' ({} people) — skipped",
                    person.name,
                    rows.len()
                ));
                continue;
            }
            rows.into_iter().next()
        };

        if existing.is_some() {
            result.people_updated += 1;
        } else {
            let id = Uuid::now_v7();
            sqlx::query!(
                r#"
                INSERT INTO org.people (id, name, email, level, active)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                id,
                person.name,
                person.email,
                person.level,
                person.active,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
            result.people_created += 1;
        }
    }
    Ok(())
}

/// Build person lookup maps by re-querying the DB after people have been inserted.
async fn build_person_maps(tx: &mut sqlx::PgConnection) -> Result<PersonMaps, Error> {
    let rows = sqlx::query!("SELECT id, name, email FROM org.people")
        .fetch_all(&mut *tx)
        .await
        .map_err(Error::from)?;

    let mut maps = PersonMaps {
        by_email: HashMap::new(),
        by_name: HashMap::new(),
    };
    for r in rows {
        if let Some(email) = &r.email {
            maps.by_email.insert(email.to_lowercase(), r.id);
        }
        maps.by_name.insert(r.name.clone(), r.id);
    }
    Ok(maps)
}

async fn wire_team_leads(
    tx: &mut sqlx::PgConnection,
    ordered_teams: &[&ExportTeam],
    team_map: &HashMap<(String, String), Uuid>,
    warnings: &mut Vec<String>,
) -> Result<(), Error> {
    let maps = build_person_maps(tx).await?;

    for team in ordered_teams {
        if let Some(lead_ref) = &team.lead_email {
            let key = (team.name.clone(), team.org_name.clone());
            let Some(&team_id) = team_map.get(&key) else {
                continue;
            };
            let lead_id = resolve_person_id(&maps, Some(lead_ref), lead_ref);
            if let Some(lid) = lead_id {
                sqlx::query!(
                    "UPDATE org.teams SET lead_id = $1 WHERE id = $2",
                    lid,
                    team_id,
                )
                .execute(&mut *tx)
                .await
                .map_err(Error::from)?;
            } else {
                warnings.push(format!(
                    "Lead '{}' not found for team '{}'",
                    lead_ref, team.name
                ));
            }
        }
    }
    Ok(())
}

async fn import_identities(
    tx: &mut sqlx::PgConnection,
    export: &OrgExport,
    identities_created: &mut i32,
) -> Result<(), Error> {
    let maps = build_person_maps(tx).await?;

    for person in &export.people {
        let Some(pid) = resolve_person_id(&maps, person.email.as_deref(), &person.name) else {
            continue;
        };

        for identity in &person.identities {
            let id = Uuid::now_v7();
            let rows = sqlx::query!(
                r#"
                INSERT INTO org.platform_identities (id, person_id, platform, platform_username)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (platform, platform_username) DO NOTHING
                "#,
                id,
                pid,
                identity.platform,
                identity.username,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

            if rows.rows_affected() > 0 {
                *identities_created += 1;
            }
        }
    }
    Ok(())
}

async fn import_memberships(
    tx: &mut sqlx::PgConnection,
    export: &OrgExport,
    team_map: &HashMap<(String, String), Uuid>,
    replace: bool,
) -> Result<(), Error> {
    let maps = build_person_maps(tx).await?;

    for person in &export.people {
        let Some(team_name) = &person.team else {
            continue;
        };
        let Some(pid) = resolve_person_id(&maps, person.email.as_deref(), &person.name) else {
            continue;
        };

        let team_id = team_map
            .iter()
            .find(|((name, _), _)| name == team_name)
            .map(|(_, &id)| id);
        let Some(tid) = team_id else { continue };

        if !replace {
            let has_membership = sqlx::query_scalar!(
                r#"
                SELECT id FROM org.team_memberships
                WHERE person_id = $1
                  AND (end_date IS NULL OR end_date > CURRENT_DATE)
                "#,
                pid,
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(Error::from)?;

            if has_membership.is_some() {
                continue;
            }
        }

        sqlx::query!(
            r#"
            UPDATE org.team_memberships
            SET end_date = CURRENT_DATE
            WHERE person_id = $1 AND (end_date IS NULL OR end_date > CURRENT_DATE)
            "#,
            pid,
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;

        let mem_id = Uuid::now_v7();
        sqlx::query!(
            r#"
            INSERT INTO org.team_memberships (id, person_id, team_id, start_date)
            VALUES ($1, $2, $3, CURRENT_DATE)
            "#,
            mem_id,
            pid,
            tid,
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;
    }
    Ok(())
}

async fn import_github_mappings(
    tx: &mut sqlx::PgConnection,
    ordered_teams: &[&ExportTeam],
    team_map: &HashMap<(String, String), Uuid>,
    result: &mut OrgImportResult,
) -> Result<(), Error> {
    for team in ordered_teams {
        let key = (team.name.clone(), team.org_name.clone());
        let Some(&team_id) = team_map.get(&key) else {
            continue;
        };
        for gh in &team.github_teams {
            let gh_team_id = sqlx::query_scalar!(
                "SELECT id FROM org.github_teams WHERE github_org = $1 AND slug = $2",
                gh.github_org,
                gh.slug,
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(Error::from)?;

            if let Some(gid) = gh_team_id {
                let rows = sqlx::query!(
                    r#"
                    INSERT INTO org.team_github_team_mappings (team_id, github_team_id)
                    VALUES ($1, $2)
                    ON CONFLICT DO NOTHING
                    "#,
                    team_id,
                    gid,
                )
                .execute(&mut *tx)
                .await
                .map_err(Error::from)?;

                if rows.rows_affected() > 0 {
                    result.github_mappings_created += 1;
                }
            } else {
                result.github_mappings_skipped += 1;
                result.warnings.push(format!(
                    "GitHub team '{}/{}' not found — mapping skipped for team '{}'",
                    gh.github_org, gh.slug, team.name
                ));
            }
        }
    }
    Ok(())
}

fn parse_team_type(s: &str) -> TeamType {
    match s {
        "org" => TeamType::Org,
        "group" => TeamType::Group,
        "squad" => TeamType::Squad,
        _ => TeamType::Team,
    }
}

/// Sort teams so that parents appear before children.
fn topological_sort_teams(teams: &[ExportTeam]) -> Vec<&ExportTeam> {
    let mut sorted: Vec<&ExportTeam> = Vec::with_capacity(teams.len());
    let mut remaining: Vec<&ExportTeam> = teams.iter().collect();

    // Iteratively add teams whose parent is already in sorted (or has no parent).
    let max_iterations = remaining.len() + 1;
    for _ in 0..max_iterations {
        if remaining.is_empty() {
            break;
        }
        let added_names: Vec<(String, String)> = sorted
            .iter()
            .map(|t| (t.name.clone(), t.org_name.clone()))
            .collect();

        let (ready, not_ready): (Vec<_>, Vec<_>) = remaining.into_iter().partition(|t| {
            t.parent_team.is_none()
                || t.parent_team
                    .as_ref()
                    .is_some_and(|p| added_names.contains(&(p.clone(), t.org_name.clone())))
        });

        sorted.extend(ready);
        remaining = not_ready;
    }

    // Any remaining have broken parent references — add them at the end.
    sorted.extend(remaining);
    sorted
}
