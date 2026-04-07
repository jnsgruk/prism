use crate::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use super::ReasoningRepo;

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
    pub created_at: OffsetDateTime,
}

/// Aggregated usage for a task type.
pub struct TaskUsage {
    pub task_type: String,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub request_count: i64,
}

/// Aggregated usage for a provider + model combination.
pub struct ModelUsage {
    pub provider: String,
    pub model: String,
    pub task_type: String,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub request_count: i64,
}

impl ReasoningRepo {
    /// Log an API usage record.
    pub async fn log_api_usage(
        &self,
        provider: &str,
        model: &str,
        task_type: &str,
        prompt_tokens: i32,
        completion_tokens: i32,
    ) -> Result<Uuid, Error> {
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO reasoning.api_usage
                (provider, model, task_type, prompt_tokens, completion_tokens)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
            provider,
            model,
            task_type,
            prompt_tokens,
            completion_tokens,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(id)
    }

    /// Get usage breakdown by task type for a date range.
    pub async fn get_usage_by_task(
        &self,
        since: OffsetDateTime,
        until: OffsetDateTime,
    ) -> Result<Vec<TaskUsage>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                task_type,
                COALESCE(SUM(prompt_tokens::bigint), 0)::bigint as "total_prompt_tokens!: i64",
                COALESCE(SUM(completion_tokens::bigint), 0)::bigint as "total_completion_tokens!: i64",
                COUNT(*) as "request_count!: i64"
            FROM reasoning.api_usage
            WHERE created_at >= $1 AND created_at < $2
            GROUP BY task_type
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
            .map(|r| TaskUsage {
                task_type: r.task_type,
                total_prompt_tokens: r.total_prompt_tokens,
                total_completion_tokens: r.total_completion_tokens,
                request_count: r.request_count,
            })
            .collect())
    }

    /// Get usage breakdown by provider/model/task for a date range.
    pub async fn get_usage_by_model(
        &self,
        since: OffsetDateTime,
        until: OffsetDateTime,
    ) -> Result<Vec<ModelUsage>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                provider,
                model,
                task_type,
                COALESCE(SUM(prompt_tokens::bigint), 0)::bigint as "total_prompt_tokens!: i64",
                COALESCE(SUM(completion_tokens::bigint), 0)::bigint as "total_completion_tokens!: i64",
                COUNT(*) as "request_count!: i64"
            FROM reasoning.api_usage
            WHERE created_at >= $1 AND created_at < $2
            GROUP BY provider, model, task_type
            ORDER BY 6 DESC
            "#,
            since,
            until,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| ModelUsage {
                provider: r.provider,
                model: r.model,
                task_type: r.task_type,
                total_prompt_tokens: r.total_prompt_tokens,
                total_completion_tokens: r.total_completion_tokens,
                request_count: r.request_count,
            })
            .collect())
    }
}
