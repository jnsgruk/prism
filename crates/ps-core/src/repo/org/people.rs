use crate::Error;
use uuid::Uuid;

use super::{OrgRepo, PersonRow};

impl OrgRepo {
    /// List people ordered by name, optionally filtering by active status.
    pub async fn list_people(&self, active_only: bool) -> Result<Vec<PersonRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT p.id, p.name, p.email, p.level, p.active,
                   tm.team_id AS "team_id?", t.name AS "team_name?"
            FROM org.people p
            LEFT JOIN org.team_memberships tm ON tm.person_id = p.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.teams t ON t.id = tm.team_id
            WHERE ($1::bool = false OR p.active = true)
            ORDER BY p.name
            "#,
            active_only,
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

    /// Get a single person with their current team info.
    pub async fn get_person(&self, id: Uuid) -> Result<Option<PersonRow>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT p.id, p.name, p.email, p.level, p.active,
                   tm.team_id AS "team_id?", t.name AS "team_name?"
            FROM org.people p
            LEFT JOIN org.team_memberships tm ON tm.person_id = p.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.teams t ON t.id = tm.team_id
            WHERE p.id = $1
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(row.map(|p| PersonRow {
            id: p.id,
            name: p.name,
            email: p.email,
            level: p.level,
            active: p.active,
            team_id: p.team_id,
            team_name: p.team_name,
        }))
    }

    /// Update a person's fields (COALESCE pattern — only non-NULL values change).
    pub async fn update_person(
        &self,
        id: Uuid,
        name: Option<&str>,
        email: Option<&str>,
        level: Option<&str>,
    ) -> Result<PersonRow, Error> {
        sqlx::query!(
            r#"
            UPDATE org.people
            SET name = COALESCE($2, name),
                email = COALESCE($3, email),
                level = COALESCE($4, level),
                updated_at = now()
            WHERE id = $1
            "#,
            id,
            name,
            email,
            level,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        self.get_person(id)
            .await?
            .ok_or_else(|| Error::Database("person not found after update".to_owned()))
    }

    /// Deactivate a person: set `active = false` and end all active memberships.
    pub async fn deactivate_person(&self, id: Uuid) -> Result<(), Error> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        sqlx::query!(
            "UPDATE org.people SET active = false, updated_at = now() WHERE id = $1",
            id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

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
        .map_err(|e| Error::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Reactivate a person: set `active = true`. Does not restore memberships.
    pub async fn reactivate_person(&self, id: Uuid) -> Result<(), Error> {
        sqlx::query!(
            "UPDATE org.people SET active = true, updated_at = now() WHERE id = $1",
            id,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }
}
