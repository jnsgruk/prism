use ps_core::models::PeriodType;
use restate_sdk::prelude::*;
use time::OffsetDateTime;
use tracing::{debug, error, info};

use crate::infra::SharedState;
use crate::infra::run_lifecycle::{complete_run, create_run, fail_run};

pub struct InsightsHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::service]
pub trait InsightsHandler {
    /// Recompute insight snapshots from enrichment data for all teams
    /// across current periods (week, month, quarter).
    async fn compute_current_periods() -> Result<(), TerminalError>;
}

impl InsightsHandler for InsightsHandlerImpl {
    async fn compute_current_periods(&self, ctx: Context<'_>) -> Result<(), TerminalError> {
        let start = std::time::Instant::now();

        let run_id = create_run!(
            ctx,
            self.state.repos,
            "_system",
            "InsightsHandler",
            "compute_current_periods"
        )?;

        let span = tracing::info_span!("handler", handler = "InsightsHandler", run_id = %run_id);
        let _guard = span.enter();

        info!("starting insights compute");

        let today = OffsetDateTime::now_utc().date();
        let mut total = 0i32;

        for period_type in &[PeriodType::Week, PeriodType::Month, PeriodType::Quarter] {
            match self.compute_period(*period_type, today).await {
                Ok(count) => {
                    total += count;
                    debug!(%period_type, count, "recomputed insight snapshots");
                }
                Err(e) => {
                    let err_msg = format!("failed to compute {period_type} insight snapshots: {e}");
                    error!(%period_type, error = %e, "failed to compute insight snapshots");
                    fail_run!(ctx, self.state.repos, run_id, "_system", &err_msg);
                    return Err(TerminalError::new(err_msg));
                }
            }
        }

        complete_run!(ctx, self.state.repos, run_id, "_system", total);

        info!(
            snapshots = total,
            duration_secs = start.elapsed().as_secs(),
            "complete"
        );
        Ok(())
    }
}

impl InsightsHandlerImpl {
    async fn compute_period(
        &self,
        period_type: PeriodType,
        today: time::Date,
    ) -> Result<i32, ps_core::Error> {
        let (start, end) = ps_metrics::period_boundaries(today, period_type);
        ps_reasoning::features::insights::compute_all_snapshots(
            &self.state.repos,
            start,
            end,
            period_type,
        )
        .await
    }
}
