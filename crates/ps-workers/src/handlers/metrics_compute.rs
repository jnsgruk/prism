use ps_core::repo::Repos;
use restate_sdk::prelude::*;
use time::OffsetDateTime;
use tracing::{error, info};

pub struct MetricsComputeHandlerImpl {
    pub repos: Repos,
}

#[restate_sdk::service]
pub trait MetricsComputeHandler {
    /// Recompute metric snapshots for all teams across current periods
    /// (current week, month, and quarter).
    async fn compute_current_periods() -> Result<(), TerminalError>;
}

impl MetricsComputeHandler for MetricsComputeHandlerImpl {
    async fn compute_current_periods(&self, _ctx: Context<'_>) -> Result<(), TerminalError> {
        let today = OffsetDateTime::now_utc().date();
        let mut total = 0i32;

        for period_type in &["week", "month", "quarter"] {
            let (start, end) = ps_metrics::period_boundaries(today, period_type);

            match ps_metrics::compute_all_snapshots(&self.repos, start, end, period_type).await {
                Ok(count) => {
                    total += count;
                    info!(period_type, count, "recomputed snapshots");
                }
                Err(e) => {
                    error!(period_type, error = %e, "failed to compute snapshots");
                }
            }
        }

        info!(total, "metrics compute complete");
        Ok(())
    }
}
