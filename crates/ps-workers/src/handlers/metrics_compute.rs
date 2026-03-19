use ps_core::models::PeriodType;
use restate_sdk::prelude::*;
use time::OffsetDateTime;
use tracing::{error, info};

use super::SharedState;
use super::run_lifecycle::{complete_run, create_run, fail_run};

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
        let run_id = create_run!(
            ctx,
            self.state.repos,
            "_system",
            "MetricsComputeHandler",
            "compute_current_periods"
        )?;

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
                    fail_run!(ctx, self.state.repos, run_id, "_system", &err_msg);
                    return Err(TerminalError::new(err_msg));
                }
            }
        }

        complete_run!(ctx, self.state.repos, run_id, "_system", total);

        info!(total, "metrics compute complete");
        Ok(())
    }
}

impl MetricsComputeHandlerImpl {
    async fn compute_period(
        &self,
        period_type: PeriodType,
        today: time::Date,
    ) -> Result<i32, ps_core::Error> {
        let (start, end) = ps_metrics::period_boundaries(today, period_type);
        ps_metrics::compute_all_snapshots(&self.state.repos, start, end, period_type).await
    }
}
