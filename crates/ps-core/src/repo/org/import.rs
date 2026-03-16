use std::collections::{HashMap, HashSet};

use crate::Error;
use crate::models::TeamType;
use sqlx::postgres::PgConnection;
use uuid::Uuid;

use super::{ImportRecord, ImportResult, OrgRepo};

/// Mutable counters and lookup maps shared across import passes.
struct ImportState {
    people_imported: i32,
    people_updated: i32,
    teams_created: i32,
    identities_mapped: i32,
    warnings: Vec<String>,
    person_name_to_id: HashMap<String, Uuid>,
    team_name_to_id: HashMap<String, Uuid>,
    has_active_membership: HashSet<Uuid>,
}

impl OrgRepo {
    /// Import directory records within a transaction.
    ///
    /// Safe re-import behaviour:
    /// - People with an existing active membership are **not** reassigned.
    /// - Teams are resolved by leader (`lead_id`), not by auto-generated name.
    /// - `last_import_at` is set for every person seen in this import.
    /// - Stale people (previously imported but absent from this file) are counted.
    pub async fn import_records(&self, records: &[ImportRecord]) -> Result<ImportResult, Error> {
        let mut state = ImportState {
            people_imported: 0,
            people_updated: 0,
            teams_created: 0,
            identities_mapped: 0,
            warnings: Vec::new(),
            person_name_to_id: HashMap::new(),
            team_name_to_id: HashMap::new(),
            has_active_membership: HashSet::new(),
        };

        let mut tx = self.pool.begin().await.map_err(Error::from)?;

        ensure_group_teams(&mut tx, records, &mut state).await?;
        upsert_people_and_teams(&mut tx, records, &mut state).await?;
        wire_team_leads(&mut tx, records, &state).await?;
        wire_parent_teams(&mut tx, records, &state).await?;

        let stale_people_count = count_stale_people(&mut tx).await?;
        let unassigned_count = count_unassigned_people(&mut tx).await?;

        tx.commit().await.map_err(Error::from)?;

        Ok(ImportResult {
            people_imported: state.people_imported,
            people_updated: state.people_updated,
            teams_created: state.teams_created,
            identities_mapped: state.identities_mapped,
            warnings: state.warnings,
            stale_people_count,
            unassigned_count,
        })
    }
}

/// Pre-pass: ensure Group teams exist for every unique group value.
/// Groups from the directory (e.g. "Ubuntu Engineering") may not have a
/// depth-1 leader in this import, so we create them upfront.
async fn ensure_group_teams(
    tx: &mut PgConnection,
    records: &[ImportRecord],
    state: &mut ImportState,
) -> Result<(), Error> {
    let unique_groups: HashSet<&str> = records.iter().filter_map(|r| r.group.as_deref()).collect();
    for &group_name in &unique_groups {
        let org_name = "Canonical";
        let gname = group_name.to_owned();
        let existing = sqlx::query_scalar!(
            "SELECT id FROM org.teams WHERE name = $1 AND org_name = $2",
            gname,
            org_name,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::from)?;

        let gid = if let Some(id) = existing {
            id
        } else {
            let new_id = Uuid::now_v7();
            sqlx::query!(
                r#"
                INSERT INTO org.teams (id, name, org_name, team_type)
                VALUES ($1, $2, $3, $4::org.team_type)
                "#,
                new_id,
                gname,
                org_name,
                TeamType::Group as TeamType,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
            state.teams_created += 1;
            new_id
        };
        state.team_name_to_id.insert(gname, gid);
    }
    Ok(())
}

/// Pass 1: upsert people, create teams, assign memberships, map identities.
async fn upsert_people_and_teams(
    tx: &mut PgConnection,
    records: &[ImportRecord],
    state: &mut ImportState,
) -> Result<(), Error> {
    for record in records {
        if record.name.is_empty() {
            state.warnings.push(format!(
                "skipping record with empty name (directory_id: {:?})",
                record.directory_id
            ));
            continue;
        }

        let resolved_id = upsert_person(tx, record, state).await?;
        state
            .person_name_to_id
            .insert(record.name.clone(), resolved_id);

        assign_team_if_needed(tx, record, resolved_id, state).await?;
        track_team_name(tx, record, state).await?;
        map_identities(tx, record, resolved_id, state).await?;
    }
    Ok(())
}

/// Upsert a single person by `directory_id` (if present) or insert new.
async fn upsert_person(
    tx: &mut PgConnection,
    record: &ImportRecord,
    state: &mut ImportState,
) -> Result<Uuid, Error> {
    let person_id = Uuid::now_v7();

    if let Some(dir_id) = &record.directory_id {
        let existing =
            sqlx::query_scalar!("SELECT id FROM org.people WHERE directory_id = $1", dir_id,)
                .fetch_optional(&mut *tx)
                .await
                .map_err(Error::from)?;

        if let Some(existing_id) = existing {
            sqlx::query!(
                r#"
                UPDATE org.people
                SET name = $1, email = $2, level = $3,
                    last_import_at = now(), updated_at = now()
                WHERE id = $4
                "#,
                record.name,
                record.email,
                record.level,
                existing_id,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

            state.people_updated += 1;
            Ok(existing_id)
        } else {
            sqlx::query!(
                r#"
                INSERT INTO org.people (id, name, email, level, directory_id, last_import_at)
                VALUES ($1, $2, $3, $4, $5, now())
                "#,
                person_id,
                record.name,
                record.email,
                record.level,
                dir_id,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

            state.people_imported += 1;
            Ok(person_id)
        }
    } else {
        sqlx::query!(
            r#"
            INSERT INTO org.people (id, name, email, level, last_import_at)
            VALUES ($1, $2, $3, $4, now())
            "#,
            person_id,
            record.name,
            record.email,
            record.level,
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;

        state.people_imported += 1;
        Ok(person_id)
    }
}

/// Check if a person has an active membership; if not, assign to their import-derived team.
async fn assign_team_if_needed(
    tx: &mut PgConnection,
    record: &ImportRecord,
    resolved_id: Uuid,
    state: &mut ImportState,
) -> Result<(), Error> {
    let any_membership = sqlx::query_scalar!(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM org.team_memberships
            WHERE person_id = $1
              AND (end_date IS NULL OR end_date > CURRENT_DATE)
        ) AS "exists!"
        "#,
        resolved_id,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(Error::from)?;

    if any_membership {
        state.has_active_membership.insert(resolved_id);
        return Ok(());
    }

    let Some(team_name) = &record.team else {
        return Ok(());
    };

    let org_name = record.org.as_deref().unwrap_or("default");

    let team_id = sqlx::query_scalar!(
        "SELECT id FROM org.teams WHERE name = $1 AND org_name = $2",
        team_name,
        org_name,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(Error::from)?;

    let team_id = if let Some(id) = team_id {
        id
    } else {
        let new_id = Uuid::now_v7();
        let tt = record.team_type.unwrap_or(TeamType::Group);
        sqlx::query!(
            r#"
            INSERT INTO org.teams (id, name, org_name, team_type)
            VALUES ($1, $2, $3, $4::org.team_type)
            "#,
            new_id,
            team_name,
            org_name,
            tt as TeamType,
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;

        state.teams_created += 1;
        new_id
    };

    state.team_name_to_id.insert(team_name.clone(), team_id);

    let membership_id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO org.team_memberships (id, person_id, team_id, start_date)
        VALUES ($1, $2, $3, CURRENT_DATE)
        "#,
        membership_id,
        resolved_id,
        team_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(Error::from)?;

    Ok(())
}

/// Track team name → id even if person already has membership (needed for hierarchy wiring).
async fn track_team_name(
    tx: &mut PgConnection,
    record: &ImportRecord,
    state: &mut ImportState,
) -> Result<(), Error> {
    if let Some(team_name) = &record.team
        && !state.team_name_to_id.contains_key(team_name)
    {
        let org_name = record.org.as_deref().unwrap_or("default");
        if let Some(tid) = sqlx::query_scalar!(
            "SELECT id FROM org.teams WHERE name = $1 AND org_name = $2",
            team_name,
            org_name,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::from)?
        {
            state.team_name_to_id.insert(team_name.clone(), tid);
        }
    }
    Ok(())
}

/// Map platform identities for a single record.
async fn map_identities(
    tx: &mut PgConnection,
    record: &ImportRecord,
    resolved_id: Uuid,
    state: &mut ImportState,
) -> Result<(), Error> {
    for identity in &record.identities {
        if identity.platform.is_empty() || identity.username.is_empty() {
            state
                .warnings
                .push(format!("skipping empty identity for {}", record.name));
            continue;
        }

        let existing = sqlx::query_scalar!(
            r#"
            SELECT id FROM org.platform_identities
            WHERE platform = $1 AND platform_username = $2
            "#,
            identity.platform,
            identity.username,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::from)?;

        if existing.is_some() {
            sqlx::query!(
                r#"
                UPDATE org.platform_identities
                SET person_id = $1
                WHERE platform = $2 AND platform_username = $3
                "#,
                resolved_id,
                identity.platform,
                identity.username,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        } else {
            let identity_id = Uuid::now_v7();
            sqlx::query!(
                r#"
                INSERT INTO org.platform_identities (id, person_id, platform, platform_username)
                VALUES ($1, $2, $3, $4)
                "#,
                identity_id,
                resolved_id,
                identity.platform,
                identity.username,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

            state.identities_mapped += 1;
        }
    }
    Ok(())
}

/// Pass 2a: wire `lead_id` for teams whose leader is in this import.
async fn wire_team_leads(
    tx: &mut PgConnection,
    records: &[ImportRecord],
    state: &ImportState,
) -> Result<(), Error> {
    for record in records {
        if record.has_reports
            && let Some(&person_id) = state.person_name_to_id.get(&record.name)
            && let Some(team_name) = &record.team
            && let Some(&team_id) = state.team_name_to_id.get(team_name)
        {
            sqlx::query!(
                "UPDATE org.teams SET lead_id = $1 WHERE id = $2 AND lead_id IS NULL",
                person_id,
                team_id,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        }
    }
    Ok(())
}

/// Pass 2b: wire `parent_team_id` (leads must be set first).
async fn wire_parent_teams(
    tx: &mut PgConnection,
    records: &[ImportRecord],
    state: &ImportState,
) -> Result<(), Error> {
    for record in records {
        let Some(team_name) = &record.team else {
            continue;
        };
        let Some(&team_id) = state.team_name_to_id.get(team_name) else {
            continue;
        };

        // Groups are always top-level — never wire a parent for them.
        if record.team_type == Some(TeamType::Group) {
            continue;
        }

        let parent_id = resolve_parent(tx, record, team_id, records, state).await?;

        if let Some(parent_id) = parent_id
            && parent_id != team_id
        {
            sqlx::query!(
                "UPDATE org.teams SET parent_team_id = $1 WHERE id = $2 AND parent_team_id IS NULL",
                parent_id,
                team_id,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        }
    }
    Ok(())
}

/// Resolve the parent team for a record: group-based for teams, manager-based for squads.
async fn resolve_parent(
    tx: &mut PgConnection,
    record: &ImportRecord,
    team_id: Uuid,
    records: &[ImportRecord],
    state: &ImportState,
) -> Result<Option<Uuid>, Error> {
    // For team-level records (not squads), use the group as parent.
    let is_squad = record.team_type == Some(TeamType::Squad);
    let group_parent = if is_squad {
        None
    } else {
        record
            .group
            .as_ref()
            .and_then(|g| state.team_name_to_id.get(g))
            .copied()
            .filter(|&gid| gid != team_id)
    };

    if group_parent.is_some() {
        return Ok(group_parent);
    }

    // For squads or when no group parent is available, use the manager relationship.
    let Some(manager_name) = &record.manager_name else {
        return Ok(None);
    };

    // First try: find team where lead_id = manager's person_id (survives team renames).
    let manager_person_id = state.person_name_to_id.get(manager_name).copied();
    let parent_id = if let Some(mgr_id) = manager_person_id {
        sqlx::query_scalar!("SELECT id FROM org.teams WHERE lead_id = $1", mgr_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(Error::from)?
    } else {
        None
    };

    // Fallback: name-based lookup (for first import where leads haven't been set yet).
    Ok(parent_id.or_else(|| {
        let parent_team_name = format!("{manager_name}'s Team");
        state
            .team_name_to_id
            .get(&parent_team_name)
            .or_else(|| {
                let squad_name = format!("{manager_name}'s Squad");
                state.team_name_to_id.get(&squad_name)
            })
            .or_else(|| {
                records
                    .iter()
                    .find(|r| r.name == *manager_name)
                    .and_then(|r| r.team.as_ref())
                    .and_then(|t| state.team_name_to_id.get(t))
            })
            .copied()
    }))
}

/// Count people previously imported but absent from this import batch.
async fn count_stale_people(tx: &mut PgConnection) -> Result<i32, Error> {
    sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::int AS "count!"
        FROM org.people
        WHERE active = true
          AND directory_id IS NOT NULL
          AND (last_import_at IS NULL OR last_import_at < now() - interval '1 minute')
        "#,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(Error::from)
}

/// Count active people with no active team membership.
async fn count_unassigned_people(tx: &mut PgConnection) -> Result<i32, Error> {
    sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::int AS "count!"
        FROM org.people p
        WHERE p.active = true
          AND NOT EXISTS (
              SELECT 1 FROM org.team_memberships tm
              WHERE tm.person_id = p.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
          )
        "#,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(Error::from)
}
