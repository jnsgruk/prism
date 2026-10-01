use crate::Error;

use crate::models::EnrichmentType;
use time::OffsetDateTime;
use uuid::Uuid;

use super::ReasoningRepo;

// ---------------------------------------------------------------------------
// Enrichment types
// ---------------------------------------------------------------------------

/// A stored enrichment record with full provenance.
pub struct EnrichmentRecord {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub enrichment_type: EnrichmentType,
    pub value: serde_json::Value,
    pub model_name: String,
    pub confidence: Option<f32>,
    pub input_hash: Option<String>,
    pub input_preview: Option<String>,
    pub created_at: OffsetDateTime,
}

/// A contribution that is eligible for enrichment (no existing enrichment row).
///
/// Derives Serialize/Deserialize so it can be passed through Restate's
/// `ctx.run()` journal as `Json<Vec<UnenrichedContribution>>`.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct UnenrichedContribution {
    pub id: Uuid,
    pub contribution_type: String,
    pub platform: String,
    pub title: Option<String>,
    pub content: Option<String>,
    pub metrics: serde_json::Value,
}

/// Pipeline status counters for a single enrichment type.
pub struct EnrichmentPipelineStatus {
    pub enrichment_type: EnrichmentType,
    pub total_count: i64,
}

/// Overall enrichment pipeline status.
pub struct EnrichmentStatus {
    pub pending_count: i64,
    pub total_enrichments: i64,
    pub last_enrichment_at: Option<OffsetDateTime>,
    pub by_type: Vec<EnrichmentPipelineStatus>,
    /// Number of contributions in the enrichment queue awaiting processing.
    pub queue_depth: i64,
}

// ---------------------------------------------------------------------------
// Enrichment queue types
// ---------------------------------------------------------------------------

/// An entry to insert into the enrichment queue during ingestion store.
pub struct EnrichmentQueueEntry {
    pub contribution_id: Uuid,
    pub content: serde_json::Value,
    pub content_hash: String,
}

/// A queued contribution ready for enrichment processing.
///
/// Derives Serialize/Deserialize so it can be passed through Restate's
/// `ctx.run()` journal as `Json<Vec<QueuedContribution>>`.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct QueuedContribution {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub contribution_type: String,
    pub content: serde_json::Value,
    /// Missing in old journals: unknown provenance must not authorize a write.
    #[serde(default)]
    pub content_hash: String,
}

/// Queue depth statistics for the enrichment status UI.
pub struct QueueStats {
    pub total_pending: i64,
    pub by_contribution_type: Vec<QueueContributionTypeCount>,
}

/// Count of queued entries for a single contribution type.
pub struct QueueContributionTypeCount {
    pub contribution_type: String,
    pub count: i64,
}

/// Parameters for upserting an enrichment record.
pub struct UpsertEnrichmentParams<'a> {
    pub contribution_id: Uuid,
    pub enrichment_type: EnrichmentType,
    pub value: &'a serde_json::Value,
    pub model_name: &'a str,
    pub confidence: Option<f32>,
    pub input_hash: Option<&'a str>,
    pub input_preview: Option<&'a str>,
    pub source_content_hash: Option<&'a str>,
}

/// A single enrichment result ready for bulk upsert.
pub struct EnrichmentResult {
    pub contribution_id: Uuid,
    pub enrichment_type: EnrichmentType,
    pub value: serde_json::Value,
    pub confidence: f32,
    pub input_hash: String,
    pub input_preview: String,
}

impl ReasoningRepo {
    // -----------------------------------------------------------------------
    // Enrichments
    // -----------------------------------------------------------------------

    /// Store a new enrichment (or replace an existing one for the same
    /// contribution + type via the UNIQUE constraint).
    pub async fn upsert_enrichment(
        &self,
        params: &UpsertEnrichmentParams<'_>,
    ) -> Result<Uuid, Error> {
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO reasoning.enrichments
                (contribution_id, enrichment_type, value, model_name, confidence, input_hash, input_preview, source_content_hash)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (contribution_id, enrichment_type)
            DO UPDATE SET
                value = EXCLUDED.value,
                model_name = EXCLUDED.model_name,
                confidence = EXCLUDED.confidence,
                input_hash = EXCLUDED.input_hash,
                input_preview = EXCLUDED.input_preview,
                source_content_hash = EXCLUDED.source_content_hash,
                created_at = now()
            RETURNING id
            "#,
            params.contribution_id,
            params.enrichment_type.as_str(),
            params.value,
            params.model_name,
            params.confidence,
            params.input_hash,
            params.input_preview,
            params.source_content_hash,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(id)
    }

    /// Bulk upsert enrichment records using UNNEST for batch performance.
    pub async fn bulk_upsert_enrichments(
        &self,
        results: &[EnrichmentResult],
        model_name: &str,
    ) -> Result<u64, Error> {
        if results.is_empty() {
            return Ok(0);
        }

        let contribution_ids: Vec<Uuid> = results.iter().map(|r| r.contribution_id).collect();
        let enrichment_types: Vec<&str> =
            results.iter().map(|r| r.enrichment_type.as_str()).collect();
        let values: Vec<&serde_json::Value> = results.iter().map(|r| &r.value).collect();
        let confidences: Vec<f32> = results.iter().map(|r| r.confidence).collect();
        let input_hashes: Vec<&str> = results.iter().map(|r| r.input_hash.as_str()).collect();
        let input_previews: Vec<&str> = results.iter().map(|r| r.input_preview.as_str()).collect();

        let confidences_opt: Vec<Option<f32>> = confidences.iter().copied().map(Some).collect();

        let result = sqlx::query!(
            r#"
            INSERT INTO reasoning.enrichments
                (contribution_id, enrichment_type, value, model_name, confidence, input_hash, input_preview)
            SELECT * FROM UNNEST(
                $1::uuid[],
                $2::text[],
                $3::jsonb[],
                $4::text[],
                $5::real[],
                $6::text[],
                $7::text[]
            )
            ON CONFLICT (contribution_id, enrichment_type)
            DO UPDATE SET
                value = EXCLUDED.value,
                model_name = EXCLUDED.model_name,
                confidence = EXCLUDED.confidence,
                input_hash = EXCLUDED.input_hash,
                input_preview = EXCLUDED.input_preview,
                source_content_hash = NULL,
                created_at = now()
            "#,
            &contribution_ids,
            &enrichment_types as &[&str],
            &values as &[&serde_json::Value],
            &vec![model_name; results.len()] as &[&str],
            &confidences_opt as &[Option<f32>],
            &input_hashes as &[&str],
            &input_previews as &[&str],
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected())
    }

    /// Get all enrichments for a single contribution.
    pub async fn get_enrichments_for_contribution(
        &self,
        contribution_id: Uuid,
    ) -> Result<Vec<EnrichmentRecord>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, contribution_id, enrichment_type, value,
                   model_name, confidence, input_hash, input_preview, created_at
            FROM reasoning.enrichments
            WHERE contribution_id = $1
            ORDER BY enrichment_type
            "#,
            contribution_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let etype = r.enrichment_type.parse::<EnrichmentType>().ok()?;
                Some(EnrichmentRecord {
                    id: r.id,
                    contribution_id: r.contribution_id,
                    enrichment_type: etype,
                    value: r.value,
                    model_name: r.model_name,
                    confidence: r.confidence,
                    input_hash: r.input_hash,
                    input_preview: r.input_preview,
                    created_at: r.created_at,
                })
            })
            .collect())
    }

    /// Get enrichments for multiple contributions at once (batch query).
    pub async fn get_enrichments_for_contributions(
        &self,
        contribution_ids: &[Uuid],
    ) -> Result<Vec<EnrichmentRecord>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, contribution_id, enrichment_type, value,
                   model_name, confidence, input_hash, input_preview, created_at
            FROM reasoning.enrichments
            WHERE contribution_id = ANY($1)
            ORDER BY contribution_id, enrichment_type
            "#,
            contribution_ids,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let etype = r.enrichment_type.parse::<EnrichmentType>().ok()?;
                Some(EnrichmentRecord {
                    id: r.id,
                    contribution_id: r.contribution_id,
                    enrichment_type: etype,
                    value: r.value,
                    model_name: r.model_name,
                    confidence: r.confidence,
                    input_hash: r.input_hash,
                    input_preview: r.input_preview,
                    created_at: r.created_at,
                })
            })
            .collect())
    }

    /// Find contributions eligible for a specific enrichment type that haven't
    /// been enriched yet.
    ///
    /// Applies per-type filters (e.g. only PR reviews for `review_depth`,
    /// only PRs with >50 lines changed for `significance`).
    pub async fn find_unenriched_contributions(
        &self,
        enrichment_type: EnrichmentType,
        limit: i64,
    ) -> Result<Vec<UnenrichedContribution>, Error> {
        // Different enrichment types target different contribution types.
        let type_filter = enrichment_type.contribution_type_filter().as_str();
        let rows = sqlx::query!(
            r#"
            SELECT c.id, c.contribution_type, c.platform, c.title, c.content, c.metrics
            FROM activity.contributions c
            LEFT JOIN reasoning.enrichments e
                ON e.contribution_id = c.id AND e.enrichment_type = $1
            WHERE e.id IS NULL
              AND c.contribution_type = $2
              AND CASE $1::text
                  WHEN 'review_depth' THEN c.content IS NOT NULL AND c.content != ''
                  WHEN 'sentiment' THEN c.content IS NOT NULL AND c.content != ''
                  WHEN 'significance' THEN
                      (c.metrics->>'additions')::int + (c.metrics->>'deletions')::int > 50
                  ELSE TRUE
              END
            ORDER BY c.created_at DESC
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
            .map(|row| UnenrichedContribution {
                id: row.id,
                contribution_type: row.contribution_type,
                platform: row.platform,
                title: row.title,
                content: row.content,
                metrics: row.metrics,
            })
            .collect())
    }

    /// Get enrichment pipeline status: pending count, total, last run, by-type breakdown.
    pub async fn get_enrichment_status(&self) -> Result<EnrichmentStatus, Error> {
        // Count of enrichable contributions that lack any enrichment.
        // Mirrors the eligibility filters in find_queued_for_enrichment():
        //   - pr_review: all (review_depth + sentiment)
        //   - pull_request: only >50 lines changed (significance)
        //   - discourse_topic: all (topic)
        let pending_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*)::bigint as "count!: i64"
            FROM activity.contributions c
            WHERE NOT EXISTS (
                SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id
              )
              AND (
                (c.contribution_type = 'pr_review' AND c.content IS NOT NULL AND c.content != '')
                OR c.contribution_type = 'discourse_topic'
                OR (
                  c.contribution_type = 'pull_request'
                  AND COALESCE((c.metrics->>'additions')::int + (c.metrics->>'deletions')::int, 0) > 50
                )
              )
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        // Total enrichments + last created_at
        let totals = sqlx::query!(
            r#"
            SELECT
                COUNT(*)::bigint as "total!: i64",
                MAX(created_at) as "last_at: OffsetDateTime"
            FROM reasoning.enrichments
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        // Breakdown by type
        let by_type = sqlx::query!(
            r#"
            SELECT enrichment_type, COUNT(*)::bigint as "count!: i64"
            FROM reasoning.enrichments
            GROUP BY enrichment_type
            ORDER BY enrichment_type
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        // Queue depth
        let queue_depth = sqlx::query_scalar!(
            r#"SELECT COUNT(*)::bigint as "count!: i64" FROM reasoning.enrichment_queue"#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(EnrichmentStatus {
            pending_count,
            total_enrichments: totals.total,
            last_enrichment_at: totals.last_at,
            by_type: by_type
                .into_iter()
                .filter_map(|r| {
                    let etype = r.enrichment_type.parse::<EnrichmentType>().ok()?;
                    Some(EnrichmentPipelineStatus {
                        enrichment_type: etype,
                        total_count: r.count,
                    })
                })
                .collect(),
            queue_depth,
        })
    }

    /// Delete all enrichments of a given type (for re-enrichment).
    pub async fn delete_enrichments_by_type(
        &self,
        enrichment_type: EnrichmentType,
    ) -> Result<u64, Error> {
        let result = sqlx::query!(
            "DELETE FROM reasoning.enrichments WHERE enrichment_type = $1",
            enrichment_type.as_str(),
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected())
    }
}
