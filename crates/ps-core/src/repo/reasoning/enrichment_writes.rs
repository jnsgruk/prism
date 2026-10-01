//! Queue results commit only while their captured source input remains current.

use std::collections::HashMap;

use uuid::Uuid;

use super::{
    ReasoningRepo,
    enrichments::{EnrichmentResult, QueuedContribution},
};
use crate::Error;

impl ReasoningRepo {
    /// Serialize with contribution updates before locking queues. Scoped writes
    /// lock contributions before invalidating results and replacing queue input.
    /// Both the bulk path and individual retries use this same guarded write.
    pub async fn bulk_upsert_queued_enrichments(
        &self,
        results: &[EnrichmentResult],
        model_name: &str,
        captured: &[QueuedContribution],
    ) -> Result<Vec<Uuid>, Error> {
        if results.is_empty() {
            return Ok(Vec::new());
        }
        let captured: HashMap<_, _> = captured
            .iter()
            .map(|queued| (queued.contribution_id, queued))
            .collect();
        let results: Vec<_> = results
            .iter()
            .filter_map(|result| {
                captured
                    .get(&result.contribution_id)
                    .map(|queued| (result, *queued))
            })
            .filter(|(_, queued)| !queued.content_hash.is_empty())
            .collect();
        if results.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<_> = results
            .iter()
            .map(|(result, _)| result.contribution_id)
            .collect();
        let queue_ids: Vec<_> = results.iter().map(|(_, queued)| queued.id).collect();
        let source_hashes: Vec<_> = results
            .iter()
            .map(|(_, queued)| queued.content_hash.as_str())
            .collect();
        let types: Vec<_> = results
            .iter()
            .map(|(result, _)| result.enrichment_type.as_str())
            .collect();
        let values: Vec<_> = results.iter().map(|(result, _)| &result.value).collect();
        let confidences: Vec<_> = results
            .iter()
            .map(|(result, _)| result.confidence)
            .collect();
        let input_hashes: Vec<_> = results
            .iter()
            .map(|(result, _)| result.input_hash.as_str())
            .collect();
        let previews: Vec<_> = results
            .iter()
            .map(|(result, _)| result.input_preview.as_str())
            .collect();

        let mut tx = self.pool.begin().await?;
        crate::repo::ActivityRepo::lock_existing_contribution_keys(&mut tx, &ids).await?;
        sqlx::query!(
            r#"
            SELECT id FROM activity.contributions
            WHERE id = ANY($1)
            ORDER BY id FOR SHARE
            "#,
            &ids,
        )
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query!(
            r#"
            SELECT contribution_id FROM reasoning.enrichment_queue
            WHERE contribution_id = ANY($1)
            ORDER BY contribution_id FOR UPDATE
            "#,
            &ids,
        )
        .fetch_all(&mut *tx)
        .await?;

        let written = sqlx::query!(
            r#"
            INSERT INTO reasoning.enrichments
                (contribution_id, enrichment_type, value, model_name, confidence,
                    input_hash, input_preview, source_content_hash)
            SELECT input.contribution_id, input.enrichment_type, input.value, $9,
                input.confidence, input.input_hash, input.input_preview, input.source_hash
            FROM UNNEST($1::uuid[], $2::uuid[], $3::text[], $4::text[],
                $5::jsonb[], $6::real[], $7::text[], $8::text[])
                AS input(contribution_id, queue_id, source_hash, enrichment_type,
                    value, confidence, input_hash, input_preview)
            JOIN reasoning.enrichment_queue queue ON queue.contribution_id = input.contribution_id
                AND queue.id = input.queue_id AND queue.content_hash = input.source_hash
            ON CONFLICT (contribution_id, enrichment_type) DO UPDATE SET
                value = EXCLUDED.value, model_name = EXCLUDED.model_name,
                confidence = EXCLUDED.confidence, input_hash = EXCLUDED.input_hash,
                input_preview = EXCLUDED.input_preview,
                source_content_hash = EXCLUDED.source_content_hash, created_at = now()
            RETURNING contribution_id
            "#,
            &ids,
            &queue_ids,
            &source_hashes as &[&str],
            &types as &[&str],
            &values as &[&serde_json::Value],
            &confidences,
            &input_hashes as &[&str],
            &previews as &[&str],
            model_name,
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(written.into_iter().map(|row| row.contribution_id).collect())
    }
}
