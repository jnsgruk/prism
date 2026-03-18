use std::sync::Arc;

use ps_reasoning::cost::CostTracker;
use ps_reasoning::features::enrichment;
use ps_reasoning::routing::TaskRouter;
use restate_sdk::prelude::*;
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::SharedState;

/// Default batch size per enrichment type.
const DEFAULT_BATCH_SIZE: i64 = 50;

/// Default interval between enrichment cycles (30 minutes).
const SCHEDULE_INTERVAL_SECS: u64 = 30 * 60;

pub struct EnrichmentHandlerImpl {
    pub state: SharedState,
    pub router: Arc<RwLock<TaskRouter>>,
}

#[restate_sdk::service]
pub trait EnrichmentHandler {
    /// Run a single enrichment cycle: process un-enriched contributions for all types.
    async fn run_cycle() -> Result<(), TerminalError>;

    /// Start the recurring enrichment schedule.
    /// Runs one cycle, then schedules the next via delayed self-invocation.
    async fn start_schedule() -> Result<(), TerminalError>;
}

impl EnrichmentHandler for EnrichmentHandlerImpl {
    async fn run_cycle(&self, _ctx: Context<'_>) -> Result<(), TerminalError> {
        let router = self.router.read().await;
        let cost_tracker = CostTracker::new(self.state.repos.reasoning.clone());

        let results = enrichment::run_enrichment_cycle(
            &router,
            &self.state.repos.reasoning,
            &cost_tracker,
            DEFAULT_BATCH_SIZE,
        )
        .await;

        let total_processed: usize = results.iter().map(|r| r.processed).sum();
        let total_errors: usize = results.iter().map(|r| r.errors).sum();

        if total_errors > 0 {
            warn!(
                processed = total_processed,
                errors = total_errors,
                "enrichment cycle completed with errors"
            );
        } else {
            info!(processed = total_processed, "enrichment cycle complete");
        }

        Ok(())
    }

    async fn start_schedule(&self, ctx: Context<'_>) -> Result<(), TerminalError> {
        // Run the enrichment cycle
        let router = self.router.read().await;
        let cost_tracker = CostTracker::new(self.state.repos.reasoning.clone());

        let results = enrichment::run_enrichment_cycle(
            &router,
            &self.state.repos.reasoning,
            &cost_tracker,
            DEFAULT_BATCH_SIZE,
        )
        .await;

        let total_processed: usize = results.iter().map(|r| r.processed).sum();
        let total_errors: usize = results.iter().map(|r| r.errors).sum();

        info!(
            processed = total_processed,
            errors = total_errors,
            "enrichment schedule cycle complete"
        );

        // Schedule the next cycle via delayed self-invocation
        ctx.service_client::<EnrichmentHandlerClient>()
            .start_schedule()
            .send_after(std::time::Duration::from_secs(SCHEDULE_INTERVAL_SECS));

        Ok(())
    }
}
