use crate::Error;
use crate::models::TeamType;
use sqlx::PgConnection;
use uuid::Uuid;

use super::ImportRecord;
use super::import::ImportState;

/// Match the stable team lead before falling back to a directory-generated name.
/// Also track aliases for hierarchy wiring without renaming existing teams.
pub(super) async fn resolve_team(
    tx: &mut PgConnection,
    record: &ImportRecord,
    state: &mut ImportState,
    create: bool,
) -> Result<Option<Uuid>, Error> {
    let Some(team_name) = &record.team else {
        return Ok(None);
    };
    if let Some(&id) = state.team_name_to_id.get(team_name) {
        return Ok(Some(id));
    }

    let org_name = record.org.as_deref().unwrap_or("default");
    let lead_name = if record.team_type == Some(TeamType::Group) {
        None
    } else if record.has_reports {
        Some(record.name.as_str())
    } else {
        record.manager_name.as_deref()
    };
    if let Some(lead_id) = lead_name.and_then(|name| state.person_name_to_id.get(name)) {
        let candidates = sqlx::query!(
            "SELECT id, name FROM org.teams WHERE lead_id = $1 AND org_name = $2",
            lead_id,
            org_name,
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(Error::from)?;
        let matched = match candidates.as_slice() {
            [team] => Some(team.id),
            [] => None,
            teams => {
                let exact = teams
                    .iter()
                    .filter(|t| t.name == *team_name)
                    .collect::<Vec<_>>();
                if let [team] = exact.as_slice() {
                    Some(team.id)
                } else {
                    if create {
                        state.warnings.push(format!(
                            "Multiple teams are led by {} in {org_name} — {} left unassigned; select a team manually",
                            lead_name.unwrap_or_default(), record.name,
                        ));
                    }
                    return Ok(None);
                }
            }
        };
        if let Some(id) = matched {
            state.team_name_to_id.insert(team_name.clone(), id);
            return Ok(Some(id));
        }
    }

    let existing = sqlx::query_scalar!(
        "SELECT id FROM org.teams WHERE name = $1 AND org_name = $2",
        team_name,
        org_name,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(Error::from)?;
    let team_id = if let Some(id) = existing {
        id
    } else if create {
        let id = Uuid::now_v7();
        let team_type = record.team_type.unwrap_or(TeamType::Group);
        sqlx::query!(
            r#"
            INSERT INTO org.teams (id, name, org_name, team_type)
            VALUES ($1, $2, $3, $4::org.team_type)
            "#,
            id,
            team_name,
            org_name,
            team_type as TeamType,
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;
        state.teams_created += 1;
        id
    } else {
        return Ok(None);
    };

    state.team_name_to_id.insert(team_name.clone(), team_id);
    Ok(Some(team_id))
}
