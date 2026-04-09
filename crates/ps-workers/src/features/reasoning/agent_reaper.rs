use restate_sdk::prelude::*;
use tracing::info;

use crate::infra::SharedState;
use crate::infra::run_lifecycle::journaled;

pub struct AgentPodReaperHandlerImpl {
    pub state: SharedState,
}

/// Fixed object key — ensures a single reaper chain via Restate's per-key
/// exclusive access guarantee. Duplicate sends queue rather than fork.
pub const REAPER_KEY: &str = "singleton";

#[restate_sdk::object]
pub trait AgentPodReaperHandler {
    /// Reap idle/expired agent pods and schedule the next run.
    async fn reap() -> Result<(), TerminalError>;
}

impl AgentPodReaperHandler for AgentPodReaperHandlerImpl {
    async fn reap(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        info!("starting agent pod reaper");

        if let Some(ref cm) = self.state.container_manager {
            let reaped_pods = cm.reap_idle_pods().await;

            if !reaped_pods.is_empty() {
                info!(count = reaped_pods.len(), "reaped idle agent pods");
            }

            // Clean up auth sessions for reaped pods (journaled).
            let repos = &self.state.repos;
            let reaped_token_sessions: Vec<String> = reaped_pods
                .iter()
                .map(|p| p.token_session_id.clone())
                .collect();
            journaled!(ctx, "cleanup_sessions", [repos, reaped_token_sessions], {
                for sid in &reaped_token_sessions {
                    if let Ok(uuid) = sid.parse::<uuid::Uuid>()
                        && let Err(e) = repos.auth.delete_session(uuid).await
                    {
                        tracing::warn!(
                            session_id = %sid,
                            error = %e,
                            "failed to delete reaped agent token"
                        );
                    }
                }
            });

            // Mark reaped conversations so the frontend shows inactive status.
            let conv_ids: Vec<uuid::Uuid> = reaped_pods
                .iter()
                .filter_map(|p| p.session_id.parse::<uuid::Uuid>().ok())
                .collect();
            if !conv_ids.is_empty() {
                journaled!(ctx, "mark_conversations_reaped", [repos, conv_ids], {
                    if let Err(e) = repos.reasoning.mark_conversations_reaped(&conv_ids).await {
                        tracing::warn!(error = %e, "failed to mark conversations as reaped");
                    }
                });
            }
        } else {
            info!("no container manager configured, skipping reap");
        }

        // Schedule next run in 60 seconds (same key — serialized, no forks).
        ctx.object_client::<AgentPodReaperHandlerClient>(REAPER_KEY)
            .reap()
            .send_after(std::time::Duration::from_secs(60));

        info!("agent pod reaper complete, next run in 60s");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reaper_handler_compiles() {
        // Smoke test: the trait + impl compiles correctly with the Restate SDK.
        let _ = std::any::type_name::<super::AgentPodReaperHandlerImpl>();
    }
}
