use restate_sdk::prelude::*;
use std::time::Duration;
use tracing::{info, warn};

use crate::infra::SharedState;
use crate::infra::run_lifecycle::{journaled_value, terminal_err};

/// Conversations stuck longer than this are reset to `failed`.
/// Covers: pod startup (~90s) + SSE timeout (300s) + margin.
const STALE_THRESHOLD_MINUTES: i32 = 10;
const WATCHDOG_INTERVAL_SECS: u64 = 60;

/// Fixed object key — ensures a single watchdog chain via Restate's per-key
/// exclusive access guarantee. Duplicate sends queue rather than fork.
pub const WATCHDOG_KEY: &str = "singleton";

pub struct QueryWatchdogHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::object]
pub trait QueryWatchdogHandler {
    /// Check for stuck conversations and reset them, then schedule next run.
    async fn check() -> Result<(), TerminalError>;
}

impl QueryWatchdogHandler for QueryWatchdogHandlerImpl {
    async fn check(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let repos = &self.state.repos;

        let stale_ids: Vec<uuid::Uuid> = journaled_value!(ctx, "reset_stale_queries", [repos], {
            repos
                .reasoning
                .reset_stale_queries(STALE_THRESHOLD_MINUTES)
                .await
                .map_err(terminal_err("failed to reset stale queries"))?
        });

        if stale_ids.is_empty() {
            info!("no stuck conversations found");
        } else {
            warn!(
                count = stale_ids.len(),
                "reset stuck conversations to failed"
            );
            for conv_id in &stale_ids {
                let _ = repos
                    .reasoning
                    .create_message(&ps_core::repo::reasoning::CreateMessageParams {
                        conversation_id: *conv_id,
                        role: "error",
                        content:
                            "This query was terminated because it stopped responding. Please retry.",
                        reasoning_trace: None,
                        supporting_data: None,
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        attached_files: &[],
                    })
                    .await;
                let _ = repos.reasoning.delete_events(*conv_id).await;
            }
        }

        // Schedule next run (same key — serialized, no forks).
        ctx.object_client::<QueryWatchdogHandlerClient>(WATCHDOG_KEY)
            .check()
            .send_after(Duration::from_secs(WATCHDOG_INTERVAL_SECS));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn watchdog_handler_compiles() {
        let _ = std::any::type_name::<super::QueryWatchdogHandlerImpl>();
    }
}
