use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{ReasoningRepo, content_hash};
use crate::{Error, ingestion::ContributionInput, models::Platform};

impl ReasoningRepo {
    /// Enqueue alongside scoped contribution writes; a queue error rolls back
    /// the entire batch instead of being logged as a successful import.
    pub(crate) async fn enqueue_scoped_in_transaction(
        tx: &mut Transaction<'_, Postgres>,
        items: &[(&ContributionInput, Uuid)],
    ) -> Result<(), Error> {
        let enriched: Vec<_> = items
            .iter()
            .filter_map(|(item, id)| {
                item.enrichment_content
                    .as_ref()
                    .map(|content| (*id, content, content_hash(content)))
            })
            .collect();
        let ids: Vec<_> = enriched.iter().map(|(id, _, _)| *id).collect();
        let contents: Vec<_> = enriched.iter().map(|(_, content, _)| *content).collect();
        let hashes: Vec<_> = enriched.iter().map(|(_, _, hash)| hash.as_str()).collect();
        if !ids.is_empty() {
            sqlx::query!(
                r#"
            INSERT INTO reasoning.enrichment_queue (contribution_id, content, content_hash)
            SELECT * FROM UNNEST($1::uuid[], $2::jsonb[], $3::text[])
            ON CONFLICT (contribution_id) DO UPDATE SET
                content = EXCLUDED.content, content_hash = EXCLUDED.content_hash, updated_at = now()
            WHERE reasoning.enrichment_queue.content_hash != EXCLUDED.content_hash
            "#,
                &ids,
                &contents as &[&serde_json::Value],
                &hashes as &[&str],
            )
            .execute(&mut **tx)
            .await?;
        }

        let jira_ids: Vec<_> = items
            .iter()
            .filter(|(item, _)| item.platform == Platform::Jira)
            .map(|(_, id)| *id)
            .collect();
        if !jira_ids.is_empty() {
            sqlx::query!(
                r#"
            INSERT INTO reasoning.embedding_queue (contribution_id, content_hash)
            SELECT id, '' FROM UNNEST($1::uuid[]) AS input(id)
            ON CONFLICT (contribution_id) DO NOTHING
            "#,
                &jira_ids,
            )
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }
}
