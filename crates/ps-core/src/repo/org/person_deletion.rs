use super::OrgRepo;
use crate::Error;
use uuid::Uuid;

impl OrgRepo {
    /// Permanently remove an inactive person, retaining unattributed source activity.
    pub async fn delete_person(&self, id: Uuid) -> Result<(), Error> {
        let mut tx = self.pool.begin().await.map_err(Error::from)?;

        // Serialize deletion with reactivation, identity edits and scoped ingestion.
        let active =
            sqlx::query_scalar!("SELECT active FROM org.people WHERE id = $1 FOR UPDATE", id,)
                .fetch_optional(&mut *tx)
                .await
                .map_err(Error::from)?
                .ok_or_else(|| Error::NotFound("person not found".to_owned()))?;

        if active {
            return Err(Error::Conflict(
                "deactivate this person before deleting them".to_owned(),
            ));
        }

        sqlx::query!("DELETE FROM org.team_memberships WHERE person_id = $1", id)
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!(
            "DELETE FROM org.platform_identities WHERE person_id = $1",
            id
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;

        // Foreign keys clear attribution/lead/login links and remove derived profiles.
        sqlx::query!("DELETE FROM org.people WHERE id = $1", id)
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        tx.commit().await.map_err(Error::from)?;
        Ok(())
    }
}
