use crate::Error;
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
            "review_depth" | "sentiment" => ("pr_review", "TRUE"),
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
        // Count of enrichable contributions that lack any enrichment
        let pending_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*)::bigint as "count!: i64"
            FROM activity.contributions c
            WHERE c.contribution_type IN ('pr_review', 'pull_request', 'discourse_topic')
              AND NOT EXISTS (
                SELECT 1 FROM reasoning.enrichments e WHERE e.contribution_id = c.id
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
}
