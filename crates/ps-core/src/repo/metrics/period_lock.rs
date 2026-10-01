use sqlx::{Postgres, Transaction};
use time::Date;

use super::MetricsRepo;
use crate::{Error, models::PeriodType};

impl MetricsRepo {
    /// Hold a transaction-scoped advisory lock for the whole computation.
    /// Queries read inputs only after acquiring it; recovery and current-period
    /// processing cannot overwrite a newer same-period result with stale reads.
    pub async fn lock_snapshot_period(
        &self,
        period_start: Date,
        period_type: PeriodType,
        insights: bool,
    ) -> Result<Transaction<'static, Postgres>, Error> {
        let kind = format!("snapshot:{insights}:{}", period_type.as_str());
        let date = period_start.to_string();
        self.lock_snapshot_computation(&kind, &date).await
    }

    /// Bound insight aggregation across periods, handlers and worker replicas.
    /// Acquire this before the period lock; waiting callers release connections.
    pub async fn lock_insight_refresh(&self) -> Result<Transaction<'static, Postgres>, Error> {
        self.lock_snapshot_computation("insight-refresh", "all-periods")
            .await
    }

    async fn lock_snapshot_computation(
        &self,
        kind: &str,
        date: &str,
    ) -> Result<Transaction<'static, Postgres>, Error> {
        loop {
            let mut guard = self.pool.begin().await?;
            let acquired = sqlx::query_scalar!(
                r#"SELECT pg_try_advisory_xact_lock(hashtext($1), hashtext($2)) AS "locked!""#,
                kind,
                date,
            )
            .fetch_one(&mut *guard)
            .await?;
            if acquired {
                return Ok(guard);
            }

            // Waiting callers release their pooled connection. The lock holder
            // still needs the pool to read inputs and write snapshots.
            guard.rollback().await?;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}
