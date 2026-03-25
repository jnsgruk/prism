use restate_sdk::prelude::*;
use tracing::info;

use super::SharedState;

pub struct AgentPodReaperHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::service]
pub trait AgentPodReaperHandler {
    /// Reap idle/expired agent pods and schedule the next run.
    async fn reap() -> Result<(), TerminalError>;
}

impl AgentPodReaperHandler for AgentPodReaperHandlerImpl {
    async fn reap(&self, ctx: Context<'_>) -> Result<(), TerminalError> {
        info!("starting agent pod reaper");

        if let Some(ref cm) = self.state.container_manager {
            let reaped_sessions = cm.reap_idle_pods().await;

            if !reaped_sessions.is_empty() {
                info!(count = reaped_sessions.len(), "reaped idle agent pods");
            }

            // Clean up auth sessions for reaped pods (journaled).
            let repos = self.state.repos.clone();
            ctx.run(move || {
                let repos = repos.clone();
                let sessions = reaped_sessions.clone();
                async move {
                    for sid in &sessions {
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
                    Ok(Json::from(()))
                }
            })
            .name("cleanup_sessions")
            .await?;
        } else {
            info!("no container manager configured, skipping reap");
        }

        // Schedule next run in 60 seconds.
        ctx.service_client::<AgentPodReaperHandlerClient>()
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
