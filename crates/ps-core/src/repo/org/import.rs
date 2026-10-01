use std::collections::{HashMap, HashSet};

use crate::Error;
use crate::models::{Management, TeamType};
use sqlx::postgres::PgConnection;
use uuid::Uuid;

use super::import_people::{map_identities, upsert_person};
use super::import_stale::{
    count_active_import_managed, count_unassigned_people, deactivate_person_in_tx,
    find_stale_people,
};
use super::{ImportRecord, ImportResult, OrgRepo};

/// Maximum fraction of import-managed people that may be deactivated as stale
/// in a single import. Above this, deactivation is skipped on the assumption
/// the file is partial or truncated, protecting against mass-deactivation.
const STALE_DEACTIVATION_MAX_FRACTION: f64 = 0.2;

/// Mutable counters and lookup maps shared across import passes.
pub(super) struct ImportState {
    pub(super) people_imported: i32,
    pub(super) people_updated: i32,
    teams_created: i32,
    pub(super) identities_mapped: i32,
    pub(super) warnings: Vec<String>,
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

        let Some(resolved_id) = upsert_person(tx, record, state).await? else {
            continue;
        };
        state
            .person_name_to_id
            .insert(record.name.clone(), resolved_id);

        assign_team_if_needed(tx, record, resolved_id, state).await?;
        track_team_name(tx, record, state).await?;
        map_identities(tx, record, resolved_id, state).await?;
    }
    Ok(())
}

/// Check if a person has an active membership; if not, assign to their import-derived team.
async fn assign_team_if_needed(
    tx: &mut PgConnection,
    record: &ImportRecord,
    resolved_id: Uuid,
    state: &mut ImportState,
) -> Result<(), Error> {
    let management = sqlx::query_scalar!(
        r#"
        SELECT membership_management AS "membership_management: Management"
        FROM org.people
        WHERE id = $1
        FOR UPDATE
        "#,
        resolved_id,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(Error::from)?;

    if management == Management::Manual {
        return Ok(());
    }

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
