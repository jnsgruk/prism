//! Resolve saved accounts against one consistent person snapshot.
use uuid::Uuid;

use crate::Error;

use super::{IdentityRow, OrgRepo};

impl OrgRepo {
    pub async fn get_active_person_backfill_identities(
        &self,
        person_id: Uuid,
    ) -> Result<Option<Vec<IdentityRow>>, Error> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query!("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await?;
        let active = sqlx::query_scalar!("SELECT active FROM org.people WHERE id = $1", person_id,)
            .fetch_optional(&mut *transaction)
            .await?
            .unwrap_or(false);
        if !active {
            transaction.commit().await?;
            return Ok(None);
        }
        let identities = sqlx::query!(
            r#"
            SELECT id, person_id, platform, platform_username, platform_user_id,
                management AS "management!: crate::models::Management"
            FROM org.platform_identities WHERE person_id = $1
            ORDER BY id
            "#,
            person_id,
        )
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Some(
            identities
                .into_iter()
                .map(|identity| IdentityRow {
                    id: identity.id,
                    person_id: identity.person_id,
                    platform: identity.platform,
                    platform_username: identity.platform_username,
                    platform_user_id: identity.platform_user_id,
                    management: identity.management,
                })
                .collect(),
        ))
    }
}
