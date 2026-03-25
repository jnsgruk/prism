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

impl ReasoningRepo {
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
                COALESCE(SUM(prompt_tokens::bigint), 0)::bigint as "total_prompt_tokens!: i64",
                COALESCE(SUM(completion_tokens::bigint), 0)::bigint as "total_completion_tokens!: i64",
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
                COALESCE(SUM(prompt_tokens::bigint), 0)::bigint as "total_prompt_tokens!: i64",
                COALESCE(SUM(completion_tokens::bigint), 0)::bigint as "total_completion_tokens!: i64",
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
