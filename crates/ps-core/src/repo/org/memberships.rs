use crate::Error;
use uuid::Uuid;

use super::{OrgRepo, PersonRow};

impl OrgRepo {
    /// Get active members of a team.
    pub async fn get_team_members(&self, team_id: Uuid) -> Result<Vec<PersonRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT p.id, p.name, p.email, p.level, p.active,
                   tm.team_id AS "team_id?", t.name AS "team_name?"
            FROM org.people p
            JOIN org.team_memberships tm ON tm.person_id = p.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.teams t ON t.id = tm.team_id
            WHERE tm.team_id = $1
              AND p.active = true
            ORDER BY p.name
            "#,
            team_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|p| PersonRow {
                id: p.id,
                name: p.name,
                email: p.email,
                level: p.level,
                active: p.active,
                team_id: p.team_id,
                team_name: p.team_name,
            })
            .collect())
    }

    /// Assign a person to a team: end any active membership first, then create a new one.
    ///
    /// The start date defaults to the person's earliest contribution date so that
    /// historical metrics include their work. Falls back to `CURRENT_DATE` if the
    /// person has no contributions yet.
    pub async fn assign_person_to_team(&self, person_id: Uuid, team_id: Uuid) -> Result<(), Error> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        // End all current active memberships for this person.
        sqlx::query!(
            r#"
            UPDATE org.team_memberships
            SET end_date = CURRENT_DATE
            WHERE person_id = $1 AND (end_date IS NULL OR end_date > CURRENT_DATE)
            "#,
            person_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        // Create new membership with start_date = earliest contribution (or today).
        let membership_id = Uuid::now_v7();
        sqlx::query!(
            r#"
            INSERT INTO org.team_memberships (id, person_id, team_id, start_date)
            VALUES (
                $1, $2, $3,
                COALESCE(
                    (SELECT MIN(created_at)::date FROM activity.contributions WHERE person_id = $2),
                    CURRENT_DATE
                )
            )
            "#,
            membership_id,
            person_id,
            team_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Remove a person from a specific team (end the membership).
    pub async fn remove_person_from_team(
        &self,
        person_id: Uuid,
        team_id: Uuid,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE org.team_memberships
            SET end_date = CURRENT_DATE
            WHERE person_id = $1 AND team_id = $2
              AND (end_date IS NULL OR end_date > CURRENT_DATE)
            "#,
            person_id,
            team_id,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// List active people with no active team membership.
    pub async fn list_unassigned_people(&self) -> Result<Vec<PersonRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT p.id, p.name, p.email, p.level, p.active
            FROM org.people p
            WHERE p.active = true
              AND NOT EXISTS (
                  SELECT 1 FROM org.team_memberships tm
                  WHERE tm.person_id = p.id
                    AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
              )
            ORDER BY p.name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|p| PersonRow {
                id: p.id,
                name: p.name,
                email: p.email,
                level: p.level,
                active: p.active,
                team_id: None,
                team_name: None,
            })
            .collect())
    }
}
