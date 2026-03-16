use ps_core::models::PeriodType;
use restate_sdk::prelude::*;
use time::OffsetDateTime;
use tracing::{error, info};
use uuid::Uuid;

use super::SharedState;

pub struct MetricsComputeHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::service]
pub trait MetricsComputeHandler {
    /// Recompute metric snapshots for all teams across current periods
    /// (current week, month, and quarter).
    async fn compute_current_periods() -> Result<(), TerminalError>;
}

impl MetricsComputeHandler for MetricsComputeHandlerImpl {
    async fn compute_current_periods(&self, ctx: Context<'_>) -> Result<(), TerminalError> {
        let run_id = self.create_run(&ctx).await?;

        let today = OffsetDateTime::now_utc().date();
        let mut total = 0i32;

        for period_type in &[PeriodType::Week, PeriodType::Month, PeriodType::Quarter] {
            match self.compute_period(*period_type, today).await {
                Ok(count) => {
                    total += count;
                    info!(%period_type, count, "recomputed snapshots");
                }
                Err(e) => {
                    let err_msg = format!("failed to compute {period_type} snapshots: {e}");
                    error!(%period_type, error = %e, "failed to compute snapshots");
                    self.fail_run(&ctx, run_id, &err_msg).await;
                    return Err(TerminalError::new(err_msg));
                }
            }
        }

        self.complete_run(&ctx, run_id, total).await;

        info!(total, "metrics compute complete");
        Ok(())
    }
}

impl MetricsComputeHandlerImpl {
    async fn create_run(&self, ctx: &Context<'_>) -> Result<Uuid, TerminalError> {
        let repos = self.state.repos.clone();
        ctx.run(|| {
            let repos = repos.clone();
            async move {
                let id = Uuid::now_v7();
                repos
                    .activity
                    .create_run(
                        id,
                        "_system",
                        "MetricsComputeHandler",
                        "compute_current_periods",
                    )
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                Ok(Json::from(id.to_string()))
            }
        })
        .name("create_run")
        .await?
        .into_inner()
        .parse()
        .map_err(|e| TerminalError::new(format!("invalid run_id: {e}")))
    }

    async fn compute_period(
        &self,
        period_type: PeriodType,
        today: time::Date,
    ) -> Result<i32, ps_core::Error> {
        let (start, end) = ps_metrics::period_boundaries(today, period_type);
        ps_metrics::compute_all_snapshots(&self.state.repos, start, end, period_type).await
    }

    async fn complete_run(&self, ctx: &Context<'_>, run_id: Uuid, items: i32) {
        let repos = self.state.repos.clone();
        let result = ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    repos
                        .activity
                        .complete_run(run_id, items)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("complete_run")
            .await;

        if let Err(e) = result {
            error!("failed to update run status: {e}");
        }
    }

    async fn fail_run(&self, ctx: &Context<'_>, run_id: Uuid, error_msg: &str) {
        let repos = self.state.repos.clone();
        let err = error_msg.to_string();
        let result = ctx
            .run(|| {
                let repos = repos.clone();
                let err = err.clone();
                async move {
                    repos
                        .activity
                        .fail_run(run_id, &err)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("fail_run")
            .await;

        if let Err(e) = result {
            error!("failed to update run status: {e}");
        }
    }
}
