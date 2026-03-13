use ps_core::repo::Repos;
use restate_sdk::prelude::*;
use time::OffsetDateTime;
use tracing::{error, info};
use uuid::Uuid;

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
    async fn compute_current_periods(&self, ctx: Context<'_>) -> Result<(), TerminalError> {
        // Create a run record for visibility
        let repos = self.repos.clone();
        let run_id: Uuid = ctx
            .run(|| {
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
            .map_err(|e| TerminalError::new(format!("invalid run_id: {e}")))?;

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
                    // Fail the run and return error
                    let repos = self.repos.clone();
                    let err_msg = format!("failed to compute {period_type} snapshots: {e}");
                    let _ = ctx
                        .run(|| {
                            let repos = repos.clone();
                            let err_msg = err_msg.clone();
                            async move {
                                repos
                                    .activity
                                    .fail_run(run_id, &err_msg)
                                    .await
                                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                                Ok(Json::from(()))
                            }
                        })
                        .name("fail_run")
                        .await;
                    return Err(TerminalError::new(err_msg));
                }
            }
        }

        // Complete the run
        let repos = self.repos.clone();
        let _ = ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    repos
                        .activity
                        .complete_run(run_id, total)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("complete_run")
            .await;

        info!(total, "metrics compute complete");
        Ok(())
    }
}
