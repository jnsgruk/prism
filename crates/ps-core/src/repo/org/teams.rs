use crate::Error;
use crate::models::TeamType;
use uuid::Uuid;

use super::{OrgRepo, TeamWithCount};

/// Map a sqlx row with team-with-count fields into a `TeamWithCount` struct.
macro_rules! team_with_count {
    ($row:expr) => {
        TeamWithCount {
            id: $row.id,
            name: $row.name,
            org_name: $row.org_name,
            parent_team_id: $row.parent_team_id,
            lead_id: $row.lead_id,
            lead_name: $row.lead_name,
            team_type: $row.team_type,
            member_count: $row.member_count,
        }
    };
}

impl OrgRepo {
    /// List teams with active member counts, optionally filtered by parent and/or type.
    pub async fn list_teams(
        &self,
        parent_filter: Option<Uuid>,
        type_filter: Option<TeamType>,
    ) -> Result<Vec<TeamWithCount>, Error> {
        let type_str = type_filter.map(|t: TeamType| t.to_string());
        let rows = sqlx::query!(
            r#"
            SELECT t.id, t.name, t.org_name, t.parent_team_id, t.lead_id,
                   t.team_type AS "team_type: TeamType",
                   lp.name AS "lead_name?",
                   COUNT(mp.id)::int AS "member_count!"
            FROM org.teams t
            LEFT JOIN org.team_memberships tm ON tm.team_id = t.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.people mp ON mp.id = tm.person_id AND mp.active = true
            LEFT JOIN org.people lp ON lp.id = t.lead_id
            WHERE ($1::uuid IS NULL OR t.parent_team_id = $1)
              AND ($2::text IS NULL OR t.team_type::text = $2)
            GROUP BY t.id, lp.name
            ORDER BY t.name
            "#,
            parent_filter,
            type_str,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows.into_iter().map(|t| team_with_count!(t)).collect())
    }

    /// Get a single team with its active member count.
    pub async fn get_team(&self, id: Uuid) -> Result<Option<TeamWithCount>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT t.id, t.name, t.org_name, t.parent_team_id, t.lead_id,
                   t.team_type AS "team_type: TeamType",
                   lp.name AS "lead_name?",
                   COUNT(mp.id)::int AS "member_count!"
            FROM org.teams t
            LEFT JOIN org.team_memberships tm ON tm.team_id = t.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.people mp ON mp.id = tm.person_id AND mp.active = true
            LEFT JOIN org.people lp ON lp.id = t.lead_id
            WHERE t.id = $1
            GROUP BY t.id, lp.name
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row.map(|t| team_with_count!(t)))
    }

    /// Get all teams (flat list) for building a tree in memory.
    pub async fn get_all_teams(&self) -> Result<Vec<TeamWithCount>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT t.id, t.name, t.org_name, t.parent_team_id, t.lead_id,
                   t.team_type AS "team_type: TeamType",
                   lp.name AS "lead_name?",
                   COUNT(mp.id)::int AS "member_count!"
            FROM org.teams t
            LEFT JOIN org.team_memberships tm ON tm.team_id = t.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.people mp ON mp.id = tm.person_id AND mp.active = true
            LEFT JOIN org.people lp ON lp.id = t.lead_id
            GROUP BY t.id, lp.name
            ORDER BY t.name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows.into_iter().map(|t| team_with_count!(t)).collect())
    }

    /// Create a new team.
    pub async fn create_team(
        &self,
        name: &str,
        org_name: &str,
        team_type: TeamType,
        parent_team_id: Option<Uuid>,
        lead_id: Option<Uuid>,
    ) -> Result<TeamWithCount, Error> {
        let id = Uuid::now_v7();
        sqlx::query!(
            r#"
            INSERT INTO org.teams (id, name, org_name, team_type, parent_team_id, lead_id)
            VALUES ($1, $2, $3, $4::org.team_type, $5, $6)
            "#,
            id,
            name,
            org_name,
            team_type as TeamType,
            parent_team_id,
            lead_id,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        self.get_team(id)
            .await?
            .ok_or_else(|| Error::Internal("failed to read back created team".to_owned()))
    }

    /// Update an existing team.
    pub async fn update_team(
        &self,
        id: Uuid,
        name: Option<&str>,
        parent_team_id: Option<Uuid>,
        lead_id: Option<Uuid>,
    ) -> Result<TeamWithCount, Error> {
        sqlx::query!(
            r#"
            UPDATE org.teams
            SET name = COALESCE($2, name),
                parent_team_id = COALESCE($3, parent_team_id),
                lead_id = COALESCE($4, lead_id)
            WHERE id = $1
            "#,
            id,
            name,
            parent_team_id,
            lead_id,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        self.get_team(id)
            .await?
            .ok_or_else(|| Error::Internal("team not found after update".to_owned()))
    }

    /// Delete a team. Fails if it has children or active members.
    pub async fn delete_team(&self, id: Uuid) -> Result<(), Error> {
        let has_children = sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM org.teams WHERE parent_team_id = $1) AS "exists!""#,
            id,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        if has_children {
            return Err(Error::Validation(
                "cannot delete team with child teams — remove or reparent children first"
                    .to_owned(),
            ));
        }

        let has_members = sqlx::query_scalar!(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM org.team_memberships
                WHERE team_id = $1 AND (end_date IS NULL OR end_date > CURRENT_DATE)
            ) AS "exists!"
            "#,
            id,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        if has_members {
            return Err(Error::Validation(
                "cannot delete team with active members — reassign members first".to_owned(),
            ));
        }

        sqlx::query!("DELETE FROM org.teams WHERE id = $1", id)
            .execute(&self.pool)
            .await
            .map_err(Error::from)?;

        Ok(())
    }
}
