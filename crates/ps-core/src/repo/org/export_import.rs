use super::export::{ExportTeam, OrgExport, OrgImportResult};
use super::export_people::ResolvedPeople;
use crate::{
    Error,
    models::{Management, TeamType},
};
use std::collections::HashMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Import helper types and functions
// ---------------------------------------------------------------------------

/// Lookup maps for resolving people by email or name.
pub(super) struct PersonMaps {
    emails: HashMap<String, Uuid>,
    names: HashMap<String, Uuid>,
}

fn resolve_person_id(maps: &PersonMaps, email: Option<&str>, name: &str) -> Option<Uuid> {
    email
        .and_then(|e| maps.emails.get(&e.trim().to_lowercase()).copied())
        .or_else(|| maps.names.get(name).copied())
}

pub(super) async fn wipe_org_data(tx: &mut sqlx::PgConnection) -> Result<(), Error> {
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

pub(super) async fn import_teams(
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

/// Build person lookup maps by re-querying the DB after people have been inserted.
pub(super) async fn build_person_maps(
    tx: &mut sqlx::PgConnection,
    export: &OrgExport,
    resolved: &ResolvedPeople,
) -> Result<PersonMaps, Error> {
    let rows = sqlx::query!("SELECT id, name, email FROM org.people")
        .fetch_all(&mut *tx)
        .await
        .map_err(Error::from)?;

    let mut maps = PersonMaps {
        emails: HashMap::new(),
        names: HashMap::new(),
    };
    let mut emails: HashMap<String, Vec<Uuid>> = HashMap::new();
    let mut names: HashMap<String, Vec<Uuid>> = HashMap::new();
    for r in rows {
        if let Some(email) = r.email {
            emails
                .entry(email.trim().to_lowercase())
                .or_default()
                .push(r.id);
        }
        names.entry(r.name).or_default().push(r.id);
    }
    maps.emails = emails
        .into_iter()
        .filter_map(|(key, ids)| match ids.as_slice() {
            [id] => Some((key, *id)),
            _ => None,
        })
        .collect();
    maps.names = names
        .into_iter()
        .filter_map(|(key, ids)| match ids.as_slice() {
            [id] => Some((key, *id)),
            _ => None,
        })
        .collect();

    // A row rejected during person reconciliation must not be rediscovered
    // through a different lookup when wiring its team lead references.
    for (person, resolved) in export.people.iter().zip(resolved) {
        if resolved.is_none() {
            if let Some(email) = &person.email {
                maps.emails.remove(&email.trim().to_lowercase());
            }
            maps.names.remove(&person.name);
        }
    }

    Ok(maps)
}

pub(super) async fn wire_team_leads(
    tx: &mut sqlx::PgConnection,
    ordered_teams: &[&ExportTeam],
    team_map: &HashMap<(String, String), Uuid>,
    maps: &PersonMaps,
    warnings: &mut Vec<String>,
) -> Result<(), Error> {
    for team in ordered_teams {
        if let Some(lead_ref) = &team.lead_email {
            let key = (team.name.clone(), team.org_name.clone());
            let Some(&team_id) = team_map.get(&key) else {
                continue;
            };
            let lead_id = resolve_person_id(maps, Some(lead_ref), lead_ref);
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

pub(super) async fn import_memberships(
    tx: &mut sqlx::PgConnection,
    export: &OrgExport,
    team_map: &HashMap<(String, String), Uuid>,
    replace: bool,
    resolved: &ResolvedPeople,
) -> Result<(), Error> {
    for (person, resolved) in export.people.iter().zip(resolved) {
        let Some(team_name) = &person.team else {
            continue;
        };
        let Some(resolved) = resolved else {
            continue;
        };
        let pid = resolved.id;

        let team_id = team_map
            .iter()
            .find(|((name, _), _)| name == team_name)
            .map(|(_, &id)| id);
        let Some(tid) = team_id else {
            continue;
        };

        if !replace && !resolved.created {
            let manual = Management::Manual;
            let existing_manual = sqlx::query_scalar!(
                r#"
                SELECT membership_management = $2 AS "protected!"
                FROM org.people
                WHERE id = $1
                "#,
                pid,
                manual as Management,
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(Error::from)?;
            if existing_manual {
                continue;
            }
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
            WHERE person_id = $1
              AND (end_date IS NULL OR end_date > CURRENT_DATE)
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

pub(super) async fn import_github_mappings(
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
pub(super) fn topological_sort_teams(teams: &[ExportTeam]) -> Vec<&ExportTeam> {
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
