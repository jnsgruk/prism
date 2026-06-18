use std::collections::{HashMap, HashSet};

use crate::Error;
use crate::models::TeamType;
use sqlx::postgres::PgConnection;
use uuid::Uuid;

use super::{ImportIdentity, ImportRecord, ImportResult, OrgRepo, StalePersonRow};

/// Maximum fraction of import-managed people that may be deactivated as stale
/// in a single import. Above this, deactivation is skipped on the assumption
/// the file is partial or truncated, protecting against mass-deactivation.
const STALE_DEACTIVATION_MAX_FRACTION: f64 = 0.2;

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
    /// - People are matched to existing rows by `directory_id` (JSON imports)
    ///   or by email (HTML imports), so re-importing updates in place rather
    ///   than creating duplicates.
    /// - People with an existing active membership are **not** reassigned.
    /// - Teams are resolved by leader (`lead_id`), not by auto-generated name.
    /// - `last_import_at` is set for every person seen in this import.
    /// - Stale people (import-managed but absent from this file) are reported,
    ///   and deactivated when `deactivate_stale` is set and the safety guard
    ///   passes.
    pub async fn import_records(
        &self,
        records: &[ImportRecord],
        deactivate_stale: bool,
    ) -> Result<ImportResult, Error> {
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

        // Leavers: active, import-managed people not touched by this run.
        // `now()` is constant within the transaction, and every person seen in
        // this import had `last_import_at` set to it, so a strictly-earlier
        // `last_import_at` marks a person absent from this file.
        let stale_people = find_stale_people(&mut tx).await?;

        let (people_deactivated, deactivation_skipped_guard) =
            if deactivate_stale && !stale_people.is_empty() {
                let managed = count_active_import_managed(&mut tx).await?;
                // Counts are at most a few thousand — far inside f64's exact-integer
                // range — so this cast cannot lose precision.
                #[allow(clippy::cast_precision_loss)]
                let fraction = stale_people.len() as f64 / f64::from(managed.max(1));
                if fraction > STALE_DEACTIVATION_MAX_FRACTION {
                    state.warnings.push(format!(
                        "skipped deactivating {} stale people: {:.0}% of import-managed people \
                     exceeds the {:.0}% safety threshold (possible partial or truncated file)",
                        stale_people.len(),
                        fraction * 100.0,
                        STALE_DEACTIVATION_MAX_FRACTION * 100.0,
                    ));
                    (0, true)
                } else {
                    for sp in &stale_people {
                        deactivate_person_in_tx(&mut tx, sp.id).await?;
                    }
                    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                    (stale_people.len() as i32, false)
                }
            } else {
                (0, false)
            };

        let unassigned_count = count_unassigned_people(&mut tx).await?;

        tx.commit().await.map_err(Error::from)?;

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let stale_people_count = stale_people.len() as i32;

        Ok(ImportResult {
            people_imported: state.people_imported,
            people_updated: state.people_updated,
            teams_created: state.teams_created,
            identities_mapped: state.identities_mapped,
            warnings: state.warnings,
            stale_people_count,
            unassigned_count,
            stale_people,
            people_deactivated,
            deactivation_skipped_guard,
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

/// Upsert a single person, matching an existing row by `directory_id` (JSON
/// imports) or by email (HTML imports, which carry no `directory_id`).
///
/// Matching in place is what makes re-import safe: without it, every HTML
/// record would insert a fresh row, duplicating the entire org on each upload.
async fn upsert_person(
    tx: &mut PgConnection,
    record: &ImportRecord,
    state: &mut ImportState,
) -> Result<Uuid, Error> {
    // 1. Match by directory_id when present (stable id from JSON imports).
    if let Some(dir_id) = &record.directory_id {
        let existing =
            sqlx::query_scalar!("SELECT id FROM org.people WHERE directory_id = $1", dir_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(Error::from)?;

        if let Some(existing_id) = existing {
            return update_existing_person(tx, record, existing_id, state).await;
        }

        let person_id = Uuid::now_v7();
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
        return Ok(person_id);
    }

    // 2. No directory_id (HTML import): match by email so re-imports update in
    //    place. Matching is case-insensitive; on the (constraint-free) chance
    //    of duplicate emails, the oldest row wins for determinism.
    if let Some(email) = record.email.as_deref().filter(|e| !e.is_empty()) {
        let existing = sqlx::query_scalar!(
            "SELECT id FROM org.people WHERE lower(email) = lower($1) ORDER BY created_at LIMIT 1",
            email,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::from)?;

        if let Some(existing_id) = existing {
            return update_existing_person(tx, record, existing_id, state).await;
        }
    }

    // 3. No match — a genuine new joiner.
    let person_id = Uuid::now_v7();
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

/// Update an existing person's directory-sourced fields and stamp
/// `last_import_at` so this run claims them as seen.
async fn update_existing_person(
    tx: &mut PgConnection,
    record: &ImportRecord,
    existing_id: Uuid,
    state: &mut ImportState,
) -> Result<Uuid, Error> {
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

/// Map platform identities for a single record using batch UPSERT.
async fn map_identities(
    tx: &mut PgConnection,
    record: &ImportRecord,
    resolved_id: Uuid,
    state: &mut ImportState,
) -> Result<(), Error> {
    let valid: Vec<&ImportIdentity> = record
        .identities
        .iter()
        .filter(|i| {
            if i.platform.is_empty() || i.username.is_empty() {
                state
                    .warnings
                    .push(format!("skipping empty identity for {}", record.name));
                false
            } else {
                true
            }
        })
        .collect();

    if valid.is_empty() {
        return Ok(());
    }

    let ids: Vec<Uuid> = valid.iter().map(|_| Uuid::now_v7()).collect();
    let person_ids: Vec<Uuid> = vec![resolved_id; valid.len()];
    let platforms: Vec<&str> = valid.iter().map(|i| i.platform.as_str()).collect();
    let usernames: Vec<String> = valid.iter().map(|i| i.username.to_lowercase()).collect();

    let result = sqlx::query_scalar!(
        r#"
        INSERT INTO org.platform_identities (id, person_id, platform, platform_username)
        SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[], $4::text[])
        ON CONFLICT (platform, platform_username)
        DO UPDATE SET person_id = EXCLUDED.person_id
        RETURNING id
        "#,
        &ids,
        &person_ids,
        &platforms as &[&str],
        &usernames,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(Error::from)?;

    #[allow(clippy::cast_possible_wrap)]
    {
        state.identities_mapped += result.len() as i32;
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

/// Find active, import-managed people absent from this import batch (leavers).
///
/// "Import-managed" means `last_import_at IS NOT NULL` — set whenever a person
/// is seen by a directory import. Manually-added people (never imported) have
/// it `NULL` and are therefore never treated as leavers. `now()` is the
/// transaction start time, so people seen in this run (stamped with the same
/// `now()`) are excluded and only strictly-earlier rows are returned.
async fn find_stale_people(tx: &mut PgConnection) -> Result<Vec<StalePersonRow>, Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, email
        FROM org.people
        WHERE active = true
          AND last_import_at IS NOT NULL
          AND last_import_at < now()
        ORDER BY name
        "#,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(Error::from)?;

    Ok(rows
        .into_iter()
        .map(|r| StalePersonRow {
            id: r.id,
            name: r.name,
            email: r.email,
        })
        .collect())
}

/// Count active, import-managed people — the denominator for the partial-file
/// safety guard.
async fn count_active_import_managed(tx: &mut PgConnection) -> Result<i32, Error> {
    sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::int AS "count!"
        FROM org.people
        WHERE active = true AND last_import_at IS NOT NULL
        "#,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(Error::from)
}

/// Deactivate a stale person within the import transaction: clear `active` and
/// end any open team memberships. Mirrors `OrgRepo::deactivate_person` but runs
/// on the shared transaction connection.
async fn deactivate_person_in_tx(tx: &mut PgConnection, id: Uuid) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE org.people SET active = false, updated_at = now() WHERE id = $1",
        id,
    )
    .execute(&mut *tx)
    .await
    .map_err(Error::from)?;

    sqlx::query!(
        r#"
        UPDATE org.team_memberships
        SET end_date = CURRENT_DATE
        WHERE person_id = $1 AND (end_date IS NULL OR end_date > CURRENT_DATE)
        "#,
        id,
    )
    .execute(&mut *tx)
    .await
    .map_err(Error::from)?;

    Ok(())
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
