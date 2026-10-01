use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::ReasoningRepo;
use crate::Error;

impl ReasoningRepo {
    /// Match ingestion's lock order for both result writes and queue cleanup:
    /// natural keys, contribution rows, then queue rows, each ordered by key.
    pub(super) async fn lock_queued_contributions(
        tx: &mut Transaction<'_, Postgres>,
        ids: &[Uuid],
    ) -> Result<(), Error> {
        crate::repo::ActivityRepo::lock_existing_contribution_keys(tx, ids).await?;
        sqlx::query!(
            r#"
            SELECT id FROM activity.contributions
            WHERE id = ANY($1)
            ORDER BY id FOR SHARE
            "#,
            ids,
        )
        .fetch_all(&mut **tx)
        .await?;
        sqlx::query!(
            r#"
            SELECT contribution_id FROM reasoning.enrichment_queue
            WHERE contribution_id = ANY($1)
            ORDER BY contribution_id FOR UPDATE
            "#,
            ids,
        )
        .fetch_all(&mut **tx)
        .await?;

        Ok(())
    }
}
