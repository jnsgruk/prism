use crate::Error;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Repository for the `reasoning` schema: API usage tracking, cost management,
/// and AI enrichments.
#[derive(Clone)]
pub struct ReasoningRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// API usage types
// ---------------------------------------------------------------------------

/// A single API usage record.
pub struct ApiUsageRecord {
    pub id: Uuid,
    pub provider: String,
    pub model: String,
    pub task_type: String,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub estimated_cost_usd: f32,
    pub created_at: OffsetDateTime,
}

/// Aggregated spend for a task type.
pub struct TaskSpend {
    pub task_type: String,
    pub total_cost_usd: f64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub request_count: i64,
}

/// Aggregated spend for a provider + model combination.
pub struct ModelSpend {
    pub provider: String,
    pub model: String,
    pub task_type: String,
    pub total_cost_usd: f64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub request_count: i64,
}

/// Daily spend summary.
pub struct DailySpend {
    pub date: time::Date,
    pub total_cost_usd: f64,
    pub request_count: i64,
}

// ---------------------------------------------------------------------------
// Enrichment types
// ---------------------------------------------------------------------------

/// A stored enrichment record with full provenance.
pub struct EnrichmentRecord {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub enrichment_type: String,
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
    pub enrichment_type: String,
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
#[derive(serde::Serialize, serde::Deserialize)]
pub struct QueuedContribution {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub contribution_type: String,
    pub content: serde_json::Value,
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

// ---------------------------------------------------------------------------
// Embedding types
// ---------------------------------------------------------------------------

/// An entry to insert into the embedding queue.
pub struct EmbeddingQueueEntry {
    pub contribution_id: Uuid,
    pub content_hash: String,
}

/// A contribution queued for embedding, with its content and enrichments pre-loaded.
///
/// Derives Serialize/Deserialize so it can be passed through Restate's
/// `ctx.run()` journal.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct QueuedEmbedding {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub content_hash: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub contribution_type: String,
    pub platform: String,
    pub enrichments: Vec<QueuedEnrichmentData>,
}

/// A single enrichment attached to a queued embedding item.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct QueuedEnrichmentData {
    pub enrichment_type: String,
    pub value: serde_json::Value,
}

/// A similar contribution result from a vector search.
pub struct SimilarContribution {
    pub contribution_id: Uuid,
    pub title: Option<String>,
    pub platform: String,
    pub contribution_type: String,
    pub state: Option<String>,
    pub author_name: Option<String>,
    pub external_url: Option<String>,
    pub distance: f64,
    pub created_at: OffsetDateTime,
}

/// Embedding pipeline status.
pub struct EmbeddingStatus {
    pub queued_count: i64,
    pub embedded_count: i64,
    pub total_eligible: i64,
    pub last_embedded_at: Option<OffsetDateTime>,
}

/// Compute a SHA-256 content hash for change detection.
pub fn content_hash(content: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(content).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    format!("{digest:x}")
}

/// Parameters for upserting an enrichment record.
pub struct UpsertEnrichmentParams<'a> {
    pub contribution_id: Uuid,
    pub enrichment_type: &'a str,
    pub value: &'a serde_json::Value,
    pub model_name: &'a str,
    pub confidence: Option<f32>,
    pub input_hash: Option<&'a str>,
    pub input_preview: Option<&'a str>,
}

/// A single enrichment result ready for bulk upsert.
pub struct EnrichmentResult {
    pub contribution_id: Uuid,
    pub enrichment_type: String,
    pub value: serde_json::Value,
    pub confidence: f32,
    pub input_hash: String,
    pub input_preview: String,
}

impl ReasoningRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Log an API usage record.
    pub async fn log_api_usage(
        &self,
        provider: &str,
        model: &str,
        task_type: &str,
        prompt_tokens: i32,
        completion_tokens: i32,
        estimated_cost_usd: f32,
    ) -> Result<Uuid, Error> {
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO reasoning.api_usage
                (provider, model, task_type, prompt_tokens, completion_tokens, estimated_cost_usd)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
            "#,
            provider,
            model,
            task_type,
            prompt_tokens,
            completion_tokens,
            estimated_cost_usd,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(id)
    }

    /// Get total spend for a given day (UTC).
    pub async fn get_daily_spend(&self, date: time::Date) -> Result<f64, Error> {
        let cost = sqlx::query_scalar!(
            r#"
            SELECT COALESCE(SUM(estimated_cost_usd::double precision), 0.0) as "cost!: f64"
            FROM reasoning.api_usage
            WHERE created_at::date = $1
            "#,
            date,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(cost)
    }

    /// Get spend breakdown by task type for a given day.
    pub async fn get_daily_spend_by_task(&self, date: time::Date) -> Result<Vec<TaskSpend>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                task_type,
                COALESCE(SUM(estimated_cost_usd::double precision), 0.0) as "total_cost_usd!: f64",
                COALESCE(SUM(prompt_tokens::bigint), 0) as "total_prompt_tokens!: i64",
                COALESCE(SUM(completion_tokens::bigint), 0) as "total_completion_tokens!: i64",
                COUNT(*) as "request_count!: i64"
            FROM reasoning.api_usage
            WHERE created_at::date = $1
            GROUP BY task_type
            ORDER BY 2 DESC
            "#,
            date,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| TaskSpend {
                task_type: r.task_type,
                total_cost_usd: r.total_cost_usd,
                total_prompt_tokens: r.total_prompt_tokens,
                total_completion_tokens: r.total_completion_tokens,
                request_count: r.request_count,
            })
            .collect())
    }

    /// Get spend breakdown by provider/model/task for a date range.
    pub async fn get_spend_summary(
        &self,
        since: OffsetDateTime,
        until: OffsetDateTime,
    ) -> Result<Vec<ModelSpend>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                provider,
                model,
                task_type,
                COALESCE(SUM(estimated_cost_usd::double precision), 0.0) as "total_cost_usd!: f64",
                COALESCE(SUM(prompt_tokens::bigint), 0) as "total_prompt_tokens!: i64",
                COALESCE(SUM(completion_tokens::bigint), 0) as "total_completion_tokens!: i64",
                COUNT(*) as "request_count!: i64"
            FROM reasoning.api_usage
            WHERE created_at >= $1 AND created_at < $2
            GROUP BY provider, model, task_type
            ORDER BY 4 DESC
            "#,
            since,
            until,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| ModelSpend {
                provider: r.provider,
                model: r.model,
                task_type: r.task_type,
                total_cost_usd: r.total_cost_usd,
                total_prompt_tokens: r.total_prompt_tokens,
                total_completion_tokens: r.total_completion_tokens,
                request_count: r.request_count,
            })
            .collect())
    }

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
                (contribution_id, enrichment_type, value, model_name, confidence, input_hash, input_preview)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (contribution_id, enrichment_type)
            DO UPDATE SET
                value = EXCLUDED.value,
                model_name = EXCLUDED.model_name,
                confidence = EXCLUDED.confidence,
                input_hash = EXCLUDED.input_hash,
                input_preview = EXCLUDED.input_preview,
                created_at = now()
            RETURNING id
            "#,
            params.contribution_id,
            params.enrichment_type,
            params.value,
            params.model_name,
            params.confidence,
            params.input_hash,
            params.input_preview,
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
            .map(|r| EnrichmentRecord {
                id: r.id,
                contribution_id: r.contribution_id,
                enrichment_type: r.enrichment_type,
                value: r.value,
                model_name: r.model_name,
                confidence: r.confidence,
                input_hash: r.input_hash,
                input_preview: r.input_preview,
                created_at: r.created_at,
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
            .map(|r| EnrichmentRecord {
                id: r.id,
                contribution_id: r.contribution_id,
                enrichment_type: r.enrichment_type,
                value: r.value,
                model_name: r.model_name,
                confidence: r.confidence,
                input_hash: r.input_hash,
                input_preview: r.input_preview,
                created_at: r.created_at,
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
        enrichment_type: &str,
        limit: i64,
    ) -> Result<Vec<UnenrichedContribution>, Error> {
        // Different enrichment types target different contribution types.
        let (type_filter, extra_filter) = match enrichment_type {
            "review_depth" | "sentiment" => {
                ("pr_review", "c.content IS NOT NULL AND c.content != ''")
            }
            "significance" => (
                "pull_request",
                "(c.metrics->>'additions')::int + (c.metrics->>'deletions')::int > 50",
            ),
            "topic" => ("discourse_topic", "TRUE"),
            _ => return Ok(vec![]),
        };

        // Use a dynamic query string since the filter varies, but parameters
        // are still bound safely.
        let query = format!(
            r"
            SELECT c.id, c.contribution_type, c.platform, c.title, c.content, c.metrics
            FROM activity.contributions c
            LEFT JOIN reasoning.enrichments e
                ON e.contribution_id = c.id AND e.enrichment_type = $1
            WHERE e.id IS NULL
              AND c.contribution_type = $2
              AND {extra_filter}
            ORDER BY c.created_at DESC
            LIMIT $3
            ",
        );

        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                String,
                Option<String>,
                Option<String>,
                serde_json::Value,
            ),
        >(&query)
        .bind(enrichment_type)
        .bind(type_filter)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(
                |(id, contribution_type, platform, title, content, metrics)| {
                    UnenrichedContribution {
                        id,
                        contribution_type,
                        platform,
                        title,
                        content,
                        metrics,
                    }
                },
            )
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
                .map(|r| EnrichmentPipelineStatus {
                    enrichment_type: r.enrichment_type,
                    total_count: r.count,
                })
                .collect(),
            queue_depth,
        })
    }

    /// Delete all enrichments of a given type (for re-enrichment).
    pub async fn delete_enrichments_by_type(&self, enrichment_type: &str) -> Result<u64, Error> {
        let result = sqlx::query!(
            "DELETE FROM reasoning.enrichments WHERE enrichment_type = $1",
            enrichment_type,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected())
    }

    // -----------------------------------------------------------------------
    // Cost tracking
    // -----------------------------------------------------------------------

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
            SELECT
                unnest($1::uuid[]),
                unnest($2::jsonb[]),
                unnest($3::text[])
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
        enrichment_type: &str,
        limit: i64,
    ) -> Result<Vec<QueuedContribution>, Error> {
        let type_filter = match enrichment_type {
            "review_depth" | "sentiment" => "pr_review",
            "significance" => "pull_request",
            "topic" => "discourse_topic",
            _ => return Ok(vec![]),
        };

        let rows = sqlx::query!(
            r#"
            SELECT
                eq.id,
                eq.contribution_id,
                c.contribution_type,
                eq.content
            FROM reasoning.enrichment_queue eq
            JOIN activity.contributions c ON c.id = eq.contribution_id
            LEFT JOIN reasoning.enrichments e
                ON e.contribution_id = eq.contribution_id
                AND e.enrichment_type = $1
            WHERE e.id IS NULL
              AND c.contribution_type = $2
            ORDER BY eq.created_at
            LIMIT $3
            "#,
            enrichment_type,
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
            })
            .collect())
    }

    /// Delete queue entries where all applicable enrichment types are satisfied.
    ///
    /// A queue row is fully enriched when every enrichment type that applies to
    /// its contribution type has a corresponding enrichment record.
    pub async fn delete_fully_enriched_entries(&self) -> Result<u64, Error> {
        // Enrichment type → contribution type mapping:
        //   review_depth, sentiment → pr_review
        //   significance → pull_request
        //   topic → discourse_topic
        let result = sqlx::query!(
            r#"
            DELETE FROM reasoning.enrichment_queue eq
            WHERE EXISTS (
                SELECT 1 FROM activity.contributions c
                WHERE c.id = eq.contribution_id
                AND CASE c.contribution_type
                    WHEN 'pr_review' THEN
                        EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'review_depth')
                        AND EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'sentiment')
                    WHEN 'pull_request' THEN
                        -- Either already enriched, or ineligible (<=50 lines changed)
                        EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'significance')
                        OR COALESCE((c.metrics->>'additions')::int + (c.metrics->>'deletions')::int, 0) <= 50
                    WHEN 'discourse_topic' THEN
                        EXISTS (SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id AND e.enrichment_type = 'topic')
                    ELSE TRUE
                END
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

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

    /// Get daily spend totals for a date range (for charts).
    pub async fn get_daily_spend_series(
        &self,
        since: time::Date,
        until: time::Date,
    ) -> Result<Vec<DailySpend>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                created_at::date as "date!: time::Date",
                COALESCE(SUM(estimated_cost_usd::double precision), 0.0) as "total_cost_usd!: f64",
                COUNT(*) as "request_count!: i64"
            FROM reasoning.api_usage
            WHERE created_at::date >= $1 AND created_at::date <= $2
            GROUP BY created_at::date
            ORDER BY created_at::date
            "#,
            since,
            until,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| DailySpend {
                date: r.date,
                total_cost_usd: r.total_cost_usd,
                request_count: r.request_count,
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // Embedding queue
    // -----------------------------------------------------------------------

    /// Bulk enqueue contributions for embedding. `ON CONFLICT DO NOTHING` (idempotent).
    pub async fn bulk_enqueue_embeddings(
        &self,
        entries: &[EmbeddingQueueEntry],
    ) -> Result<u64, Error> {
        if entries.is_empty() {
            return Ok(0);
        }

        let contribution_ids: Vec<Uuid> = entries.iter().map(|e| e.contribution_id).collect();
        let hashes: Vec<&str> = entries.iter().map(|e| e.content_hash.as_str()).collect();

        // Runtime query — the embedding_queue table is new and not yet in the
        // sqlx offline cache. Will be migrated to query!() after sqlx prepare.
        let result = sqlx::query(
            r"
            INSERT INTO reasoning.embedding_queue (contribution_id, content_hash)
            SELECT unnest($1::uuid[]), unnest($2::text[])
            ON CONFLICT (contribution_id) DO NOTHING
            ",
        )
        .bind(&contribution_ids)
        .bind(&hashes as &[&str])
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected())
    }

    /// Fetch a batch of queued contributions with their content and enrichments.
    ///
    /// JOINs `activity.contributions` for title/body and LEFT JOINs
    /// `reasoning.enrichments` for rationale text.
    pub async fn find_queued_for_embedding(
        &self,
        limit: i64,
    ) -> Result<Vec<QueuedEmbedding>, Error> {
        // Step 1: Fetch queue entries joined with contribution data.
        let rows: Vec<(
            Uuid,
            Uuid,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
        )> = sqlx::query_as(
            r"
                SELECT
                    eq.id,
                    eq.contribution_id,
                    eq.content_hash,
                    c.title,
                    c.content,
                    c.contribution_type,
                    c.platform
                FROM reasoning.embedding_queue eq
                JOIN activity.contributions c ON c.id = eq.contribution_id
                ORDER BY eq.created_at
                LIMIT $1
                ",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        if rows.is_empty() {
            return Ok(vec![]);
        }

        // Step 2: Batch-fetch enrichments for all these contributions.
        let contribution_ids: Vec<Uuid> = rows.iter().map(|r| r.1).collect();
        let enrichment_rows: Vec<(Uuid, String, serde_json::Value)> = sqlx::query_as(
            r"
            SELECT contribution_id, enrichment_type, value
            FROM reasoning.enrichments
            WHERE contribution_id = ANY($1)
            ORDER BY contribution_id, enrichment_type
            ",
        )
        .bind(&contribution_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        // Group enrichments by contribution_id.
        let mut enrichment_map: std::collections::HashMap<Uuid, Vec<QueuedEnrichmentData>> =
            std::collections::HashMap::new();
        for (cid, etype, value) in enrichment_rows {
            enrichment_map
                .entry(cid)
                .or_default()
                .push(QueuedEnrichmentData {
                    enrichment_type: etype,
                    value,
                });
        }

        Ok(rows
            .into_iter()
            .map(
                |(id, contribution_id, content_hash, title, body, contribution_type, platform)| {
                    QueuedEmbedding {
                        id,
                        contribution_id,
                        content_hash,
                        title,
                        body,
                        contribution_type,
                        platform,
                        enrichments: enrichment_map.remove(&contribution_id).unwrap_or_default(),
                    }
                },
            )
            .collect())
    }

    /// Delete queue entries for contributions that now have embeddings.
    pub async fn delete_embedded_queue_entries(&self) -> Result<u64, Error> {
        let result = sqlx::query(
            r"
            DELETE FROM reasoning.embedding_queue eq
            WHERE EXISTS (
                SELECT 1 FROM reasoning.embeddings e
                WHERE e.contribution_id = eq.contribution_id
            )
            ",
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected())
    }

    // -----------------------------------------------------------------------
    // Embedding storage
    // -----------------------------------------------------------------------

    /// Store a single embedding. Upserts on (`contribution_id`, `model_name`).
    pub async fn upsert_embedding(
        &self,
        contribution_id: Uuid,
        embedding: &pgvector::Vector,
        model_name: &str,
    ) -> Result<(), Error> {
        sqlx::query(
            r"
            INSERT INTO reasoning.embeddings (contribution_id, embedding, model_name)
            VALUES ($1, $2, $3)
            ON CONFLICT (contribution_id, model_name)
            DO UPDATE SET embedding = EXCLUDED.embedding, created_at = now()
            ",
        )
        .bind(contribution_id)
        .bind(embedding)
        .bind(model_name)
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Bulk upsert embeddings. Iterates per-row since sqlx doesn't support
    /// `vector[]` UNNEST natively.
    pub async fn bulk_upsert_embeddings(
        &self,
        contribution_ids: &[Uuid],
        embeddings: &[Vec<f32>],
        model_name: &str,
    ) -> Result<u64, Error> {
        if contribution_ids.is_empty() {
            return Ok(0);
        }

        let mut count = 0u64;
        for (cid, emb) in contribution_ids.iter().zip(embeddings.iter()) {
            let vector = pgvector::Vector::from(emb.clone());
            self.upsert_embedding(*cid, &vector, model_name).await?;
            count += 1;
        }

        Ok(count)
    }

    // -----------------------------------------------------------------------
    // Similarity queries
    // -----------------------------------------------------------------------

    /// Find contributions with embeddings most similar to the given vector.
    /// Filters by optional platform. Returns top N sorted by cosine distance.
    pub async fn find_similar(
        &self,
        embedding: &[f32],
        limit: i64,
        platform_filter: Option<&str>,
        exclude_contribution_id: Option<Uuid>,
    ) -> Result<Vec<SimilarContribution>, Error> {
        let vector = pgvector::Vector::from(embedding.to_vec());

        let rows: Vec<(
            Uuid,
            Option<String>,
            String,
            String,
            Option<String>,
            Option<String>,
            OffsetDateTime,
            Option<String>,
            f64,
        )> = sqlx::query_as(
            r"
            SELECT
                c.id,
                c.title,
                c.platform,
                c.contribution_type,
                c.state,
                c.external_url,
                c.created_at,
                p.display_name,
                (e.embedding <=> $1::vector)::float8 as distance
            FROM reasoning.embeddings e
            JOIN activity.contributions c ON c.id = e.contribution_id
            LEFT JOIN org.people p ON p.id = c.person_id
            WHERE ($2::text IS NULL OR c.platform = $2)
              AND ($3::uuid IS NULL OR c.id != $3)
              AND e.embedding <=> $1::vector < 0.5
            ORDER BY e.embedding <=> $1::vector
            LIMIT $4
            ",
        )
        .bind(&vector)
        .bind(platform_filter)
        .bind(exclude_contribution_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    contribution_id,
                    title,
                    platform,
                    contribution_type,
                    state,
                    external_url,
                    created_at,
                    author_name,
                    distance,
                )| {
                    SimilarContribution {
                        contribution_id,
                        title,
                        platform,
                        contribution_type,
                        state,
                        author_name,
                        external_url,
                        distance,
                        created_at,
                    }
                },
            )
            .collect())
    }

    /// Find similar contributions to a given contribution (by ID).
    pub async fn find_similar_to_contribution(
        &self,
        contribution_id: Uuid,
        limit: i64,
        platform_filter: Option<&str>,
    ) -> Result<Vec<SimilarContribution>, Error> {
        // Look up the contribution's embedding vector.
        let vector: Option<(pgvector::Vector,)> = sqlx::query_as(
            r"
            SELECT embedding
            FROM reasoning.embeddings
            WHERE contribution_id = $1
            LIMIT 1
            ",
        )
        .bind(contribution_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        let Some((vector,)) = vector else {
            return Ok(vec![]);
        };

        self.find_similar(
            vector.as_slice(),
            limit,
            platform_filter,
            Some(contribution_id),
        )
        .await
    }

    /// Check if a contribution has an embedding.
    pub async fn has_embedding(&self, contribution_id: Uuid) -> Result<bool, Error> {
        let (exists,): (bool,) = sqlx::query_as(
            r"
            SELECT EXISTS(
                SELECT 1 FROM reasoning.embeddings WHERE contribution_id = $1
            )
            ",
        )
        .bind(contribution_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(exists)
    }

    /// Get embedding pipeline status. Runs three independent count queries
    /// in parallel for lower latency.
    pub async fn get_embedding_status(&self) -> Result<EmbeddingStatus, Error> {
        let (queued, embedded, eligible) = tokio::try_join!(
            async {
                let (count,): (i64,) =
                    sqlx::query_as(r"SELECT COUNT(*)::bigint FROM reasoning.embedding_queue")
                        .fetch_one(&self.pool)
                        .await
                        .map_err(Error::from)?;
                Ok::<_, Error>(count)
            },
            async {
                let row: (i64, Option<OffsetDateTime>) = sqlx::query_as(
                    r"SELECT COUNT(*)::bigint, MAX(created_at) FROM reasoning.embeddings",
                )
                .fetch_one(&self.pool)
                .await
                .map_err(Error::from)?;
                Ok::<_, Error>(row)
            },
            async {
                let (count,): (i64,) = sqlx::query_as(
                    r"
                    SELECT COUNT(DISTINCT contribution_id)::bigint
                    FROM (
                        SELECT contribution_id FROM reasoning.enrichments
                        UNION
                        SELECT id FROM activity.contributions
                        WHERE contribution_type = 'jira_ticket'
                          AND (title IS NOT NULL OR content IS NOT NULL)
                    ) eligible
                    ",
                )
                .fetch_one(&self.pool)
                .await
                .map_err(Error::from)?;
                Ok::<_, Error>(count)
            },
        )?;

        Ok(EmbeddingStatus {
            queued_count: queued,
            embedded_count: embedded.0,
            total_eligible: eligible,
            last_embedded_at: embedded.1,
        })
    }
}
