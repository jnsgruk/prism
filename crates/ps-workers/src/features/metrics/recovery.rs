//! Recover committed historical work after failures and cancellations.

use restate_sdk::prelude::*;
use std::time::Duration;

use super::historical::recompute_period;
use crate::infra::{SharedState, run_lifecycle::journaled_value};

pub const SNAPSHOT_REFRESH_KEY: &str = "singleton";

pub struct SnapshotRefreshHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::object]
pub trait SnapshotRefreshHandler {
    async fn recover() -> Result<(), TerminalError>;
}

impl SnapshotRefreshHandler for SnapshotRefreshHandlerImpl {
    async fn recover(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let repos = &self.state.repos;
        let mut full_batch = false;
        for insights in [false, true] {
            let work = journaled_value!(ctx, format!("recover_periods_{insights}"), [repos], {
                repos
                    .activity
                    .pending_snapshot_invalidations(None, insights, 8)
                    .await?
            });
            full_batch |= work.len() == 8;
            for period in work {
                tracing::info!(period_type = %period.period_type, period_start = %period.period_start,
                    insights, invalidations = period.invalidation_ids.len(), "recovering historical snapshots");
                recompute_period!(ctx, repos, period, insights, None);
            }
        }
        // Bounded journals; backlog runs continue promptly. Unavailable AI inputs
        // remain durable and are revisited without pretending they were enriched.
        let delay = if full_batch { 1 } else { 60 };
        ctx.object_client::<SnapshotRefreshHandlerClient>(SNAPSHOT_REFRESH_KEY)
            .recover()
            .send_after(Duration::from_secs(delay));
        Ok(())
    }
}
