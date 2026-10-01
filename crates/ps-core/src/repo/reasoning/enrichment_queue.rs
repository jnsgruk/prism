use uuid::Uuid;

use super::{
    ReasoningRepo,
    enrichments::{
        EnrichmentQueueEntry, QueueContributionTypeCount, QueueStats, QueuedContribution,
    },
};
use crate::{Error, models::EnrichmentType};

impl ReasoningRepo {
    // -----------------------------------------------------------------------
    // Enrichment queue
    // -----------------------------------------------------------------------

    /// Bulk insert enrichment queue entries, refreshing content on conflict.
    ///
    /// Uses UNNEST for batch performance. `ON CONFLICT (contribution_id)`
    /// updates content and hash if the content has changed.
    pub async fn bulk_enqueue_enrichments(
        &self,
        entries: &[EnrichmentQueueEntry],
    ) -> Result<u64, Error> {
        if entries.is_empty() {
            return Ok(0);
        }

        let contribution_ids: Vec<Uuid> = entries.iter().map(|e| e.contribution_id).collect();
        let contents: Vec<&serde_json::Value> = entries.iter().map(|e| &e.content).collect();
        let hashes: Vec<&str> = entries.iter().map(|e| e.content_hash.as_str()).collect();

        let result = sqlx::query!(
            r#"
            INSERT INTO reasoning.enrichment_queue (contribution_id, content, content_hash)
            SELECT input.contribution_id, input.content, input.content_hash
            FROM UNNEST($1::uuid[], $2::jsonb[], $3::text[])
                AS input(contribution_id, content, content_hash)
            ORDER BY input.contribution_id
            ON CONFLICT (contribution_id)
            DO UPDATE SET
                content = EXCLUDED.content,
                content_hash = EXCLUDED.content_hash,
                updated_at = now()
            WHERE reasoning.enrichment_queue.content_hash != EXCLUDED.content_hash
            "#,
            &contribution_ids,
            &contents as &[&serde_json::Value],
            &hashes as &[&str],
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected())
    }

    /// Find queued contributions that are missing a specific enrichment type.
    ///
    /// JOINs the queue with contributions and LEFT JOINs enrichments to find
    /// entries that haven't been enriched yet for this type.
    pub async fn find_queued_for_enrichment(
        &self,
        enrichment_type: EnrichmentType,
        limit: i64,
    ) -> Result<Vec<QueuedContribution>, Error> {
        let type_filter = enrichment_type.contribution_type_filter().as_str();

        let rows = sqlx::query!(
            r#"
            SELECT
                eq.id,
                eq.contribution_id,
                c.contribution_type,
                eq.content,
                eq.content_hash
            FROM reasoning.enrichment_queue eq
            JOIN activity.contributions c ON c.id = eq.contribution_id
            LEFT JOIN reasoning.enrichments e
                ON e.contribution_id = eq.contribution_id
                AND e.enrichment_type = $1
                AND e.source_content_hash = eq.content_hash
            WHERE e.id IS NULL
              AND c.contribution_type = $2
            ORDER BY eq.created_at
            LIMIT $3
            "#,
            enrichment_type.as_str(),
            type_filter,
            limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| QueuedContribution {
                id: r.id,
                contribution_id: r.contribution_id,
                contribution_type: r.contribution_type,
                content: r.content,
                content_hash: r.content_hash,
            })
            .collect())
    }

    /// Delete queue entries where all applicable enrichment types are satisfied.
    ///
    /// A queue row is fully enriched when every enrichment type that applies to
    /// its contribution type has a corresponding enrichment record.
    pub async fn delete_fully_enriched_entries(&self) -> Result<u64, Error> {
        // Enrichment type -> contribution type mapping:
        //   review_depth, sentiment -> pr_review
        //   significance -> pull_request
        //   topic -> discourse_topic
        let mut tx = self.pool.begin().await?;
        let ids = sqlx::query_scalar!(
            r#"
            SELECT eq.contribution_id FROM reasoning.enrichment_queue eq
            WHERE EXISTS (
                SELECT 1 FROM activity.contributions c
                WHERE c.id = eq.contribution_id
                AND CASE c.contribution_type
                    WHEN 'pr_review' THEN
                        EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'review_depth' AND e.source_content_hash = eq.content_hash)
                        AND EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'sentiment' AND e.source_content_hash = eq.content_hash)
                    WHEN 'pull_request' THEN
                        -- Either already enriched, or ineligible (<=50 lines changed)
                        EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'significance' AND e.source_content_hash = eq.content_hash)
                        OR COALESCE((c.metrics->>'additions')::int + (c.metrics->>'deletions')::int, 0) <= 50
                    WHEN 'discourse_topic' THEN
                        EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'topic' AND e.source_content_hash = eq.content_hash)
                    ELSE TRUE
                END
            )
            ORDER BY eq.contribution_id
            "#,
        ).fetch_all(&mut *tx).await?;
        if ids.is_empty() {
            tx.commit().await?;
            return Ok(0);
        }
        crate::repo::ActivityRepo::lock_existing_contribution_keys(&mut tx, &ids).await?;
        sqlx::query!(
            "SELECT id FROM activity.contributions WHERE id = ANY($1) ORDER BY id FOR SHARE",
            &ids,
        )
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query!(
            "SELECT contribution_id FROM reasoning.enrichment_queue WHERE contribution_id = ANY($1) ORDER BY contribution_id FOR UPDATE",
            &ids,
        ).fetch_all(&mut *tx).await?;

        // After waiting for ingestion, re-evaluate eligibility and hashes with
        // a fresh statement snapshot. The old <=50-line shortcut is not proof
        // that a concurrently replaced PR queue is still ineligible.
        let result = sqlx::query!(
            r#"
            DELETE FROM reasoning.enrichment_queue eq
            WHERE eq.contribution_id = ANY($1) AND EXISTS (
                SELECT 1 FROM activity.contributions c
                WHERE c.id = eq.contribution_id
                AND CASE c.contribution_type
                    WHEN 'pr_review' THEN
                        EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'review_depth' AND e.source_content_hash = eq.content_hash)
                        AND EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'sentiment' AND e.source_content_hash = eq.content_hash)
                    WHEN 'pull_request' THEN
                        -- Either already enriched, or ineligible (<=50 lines changed)
                        EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'significance' AND e.source_content_hash = eq.content_hash)
                        OR COALESCE((c.metrics->>'additions')::int + (c.metrics->>'deletions')::int, 0) <= 50
                    WHEN 'discourse_topic' THEN
                        EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'topic' AND e.source_content_hash = eq.content_hash)
                    ELSE TRUE
                END
            )
            "#,
            &ids,
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;

        tx.commit().await?;
        Ok(result.rows_affected())
    }

    /// Get queue depth statistics for the status UI.
    pub async fn get_queue_stats(&self) -> Result<QueueStats, Error> {
        let total = sqlx::query_scalar!(
            r#"SELECT COUNT(*)::bigint as "count!: i64" FROM reasoning.enrichment_queue"#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        let by_type = sqlx::query!(
            r#"
            SELECT
                c.contribution_type,
                COUNT(*)::bigint as "count!: i64"
            FROM reasoning.enrichment_queue eq
            JOIN activity.contributions c ON c.id = eq.contribution_id
            GROUP BY c.contribution_type
            ORDER BY c.contribution_type
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(QueueStats {
            total_pending: total,
            by_contribution_type: by_type
                .into_iter()
                .map(|r| QueueContributionTypeCount {
                    contribution_type: r.contribution_type,
                    count: r.count,
                })
                .collect(),
        })
    }
}
