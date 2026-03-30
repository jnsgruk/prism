use restate_sdk::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, warn};

use crate::infra::SharedState;
use crate::infra::run_lifecycle::journaled;

/// Run orphan PVC cleanup every N reaper invocations (~10 minutes at 60s intervals).
const ORPHAN_CHECK_INTERVAL: u64 = 10;

static REAP_COUNTER: AtomicU64 = AtomicU64::new(0);

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
            let reaped_sessions = cm.reap_idle_pods().await;

            if !reaped_sessions.is_empty() {
                info!(count = reaped_sessions.len(), "reaped idle agent pods");
            }

            // Clean up auth sessions for reaped pods (journaled).
            let repos = &self.state.repos;
            journaled!(ctx, "cleanup_sessions", [repos, reaped_sessions], {
                for sid in &reaped_sessions {
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
            // Periodically check for orphaned workspace PVCs.
            let count = REAP_COUNTER.fetch_add(1, Ordering::Relaxed);
            if count.is_multiple_of(ORPHAN_CHECK_INTERVAL) {
                self.cleanup_orphaned_pvcs(cm).await;
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

impl AgentPodReaperHandlerImpl {
    /// Delete workspace PVCs whose conversations no longer exist.
    async fn cleanup_orphaned_pvcs(&self, cm: &ps_agent::ContainerManager) {
        let pvcs = match cm.list_workspace_pvcs().await {
            Ok(pvcs) if !pvcs.is_empty() => pvcs,
            Ok(_) => return,
            Err(e) => {
                warn!(error = %e, "failed to list workspace PVCs for orphan cleanup");
                return;
            }
        };

        for (pvc_name, session_id) in &pvcs {
            let Ok(conv_id) = session_id.parse::<uuid::Uuid>() else {
                continue;
            };

            match self
                .state
                .repos
                .reasoning
                .conversation_exists(conv_id)
                .await
            {
                Ok(false) => {
                    info!(pvc_name, session_id, "deleting orphaned workspace PVC");
                    if let Err(e) = cm.delete_pvc(session_id).await {
                        warn!(pvc_name, error = %e, "failed to delete orphaned PVC");
                    }
                }
                Err(e) => {
                    warn!(session_id, error = %e, "failed to check conversation existence");
                }
                Ok(true) => {}
            }
        }
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
