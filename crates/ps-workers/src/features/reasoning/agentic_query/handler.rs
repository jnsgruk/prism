use ps_core::repo::Repos;
use restate_sdk::prelude::*;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::{AgenticQueryRequest, PrepareQueryResponse, SharedState};
use crate::infra::run_lifecycle::{journaled, terminal_err};

pub struct AgenticQueryHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::object]
pub trait AgenticQueryHandler {
    /// Prepare an agent pod for a conversation and return its IP.
    ///
    /// This is a fast operation (<90s) that handles only the durable pod
    /// lifecycle. SSE streaming is done by ps-server after this returns.
    async fn prepare_query(
        request: Json<AgenticQueryRequest>,
    ) -> Result<Json<PrepareQueryResponse>, TerminalError>;

    /// Cancel a running query.
    async fn cancel() -> Result<(), TerminalError>;

    /// Clean up pod and workspace PVC for a conversation.
    async fn cleanup_storage() -> Result<(), TerminalError>;
}

// ---------------------------------------------------------------------------
// Journaled step ordering matters — changing the sequence of `journaled!`
// calls breaks in-flight Restate invocations. See the "Journal Compatibility"
// section in CLAUDE.md before reordering.
// ---------------------------------------------------------------------------

impl AgenticQueryHandler for AgenticQueryHandlerImpl {
    async fn prepare_query(
        &self,
        ctx: ObjectContext<'_>,
        Json(request): Json<AgenticQueryRequest>,
    ) -> Result<Json<PrepareQueryResponse>, TerminalError> {
        let conv_id: Uuid = request
            .conversation_id
            .parse()
            .map_err(terminal_err("invalid conversation_id"))?;
        let user_id: Uuid = request
            .user_id
            .parse()
            .map_err(terminal_err("invalid user_id"))?;

        let span = tracing::info_span!(
            "agentic_query",
            handler = "AgenticQueryHandler",
            conversation_id = %conv_id,
        );
        let _guard = span.enter();

        info!("preparing agent pod");

        let repos = &self.state.repos;

        journaled!(ctx, "update_status_running", [repos], {
            repos
                .reasoning
                .update_query_status(conv_id, ps_core::models::QueryStatus::Running)
                .await
                .map_err(terminal_err("failed to update status"))?;
        });

        let cm = self
            .state
            .container_manager
            .as_ref()
            .ok_or_else(|| TerminalError::new("agent containers not configured"))?
            .clone();

        // Generate service token for the pod (outside ctx.run for security).
        let service_token = ps_core::auth::generate_token();
        let token_hash = ps_core::auth::hash_token(&service_token);
        let token_session_id = Uuid::now_v7();

        journaled!(ctx, "create_agent_session", [repos, token_hash], {
            repos
                .auth
                .create_session(
                    token_session_id,
                    user_id,
                    &token_hash,
                    "agent_service",
                    Some(time::OffsetDateTime::now_utc() + time::Duration::hours(3)),
                    Some("agent-container"),
                )
                .await
                .map_err(terminal_err("failed to create agent session"))?;
        });

        let pod_overrides = ps_agent::PodOverrides {
            service_token,
            token_session_id: token_session_id.to_string(),
            model: request.model.clone(),
            small_model: request.small_model.clone(),
            provider_keys: request.provider_keys.clone(),
            default_image_model: request.image_model.clone(),
        };

        let (pod_ip, pod_name) = start_pod(&ctx, repos, &cm, conv_id, pod_overrides).await?;

        info!(%pod_ip, %pod_name, "agent pod ready");
        Ok(Json(PrepareQueryResponse { pod_ip, pod_name }))
    }

    async fn cancel(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let session_id = ctx.key().to_string();
        info!(session_id, "cancelling agentic query");

        let conv_id: Uuid = session_id.parse().map_err(terminal_err("invalid key"))?;

        let repos = &self.state.repos;
        journaled!(ctx, "update_status_cancelled", [repos], {
            repos
                .reasoning
                .update_query_status(conv_id, ps_core::models::QueryStatus::Cancelled)
                .await
                .map_err(terminal_err("failed to update status"))?;
        });

        Ok(())
    }

    async fn cleanup_storage(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let session_id = ctx.key().to_string();
        info!(session_id, "cleaning up agent pod and workspace");

        if let Some(ref cm) = self.state.container_manager
            && let Err(e) = cm.delete_pod(&session_id).await
        {
            warn!(error = %e, "failed to delete pod during cleanup");
        }

        // Delete workspace directory from the shared PVC.
        // Only called on user-initiated conversation delete — pod expiry does NOT trigger this.
        // Not wrapped in ctx.run() — idempotent and must not be journaled.
        if let Some(ref ws_path) = self.state.workspaces_path {
            let dir = ws_path.join(&session_id);
            match tokio::fs::remove_dir_all(&dir).await {
                Ok(()) => info!(session_id, "deleted workspace directory"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    debug!(session_id, "workspace directory already gone");
                }
                Err(e) => warn!(session_id, error = %e, "failed to delete workspace directory"),
            }
        }

        Ok(())
    }
}

/// Create the agent pod and wait for it to become ready, emitting container
/// status events along the way. Returns `(pod_ip, pod_name)`.
async fn start_pod(
    ctx: &ObjectContext<'_>,
    repos: &Repos,
    cm: &ps_agent::ContainerManager,
    conv_id: Uuid,
    pod_overrides: ps_agent::PodOverrides,
) -> Result<(String, String), TerminalError> {
    let creating_payload =
        serde_json::json!({"status": "creating", "message": "Starting agent container..."});
    journaled!(
        ctx,
        "event_container_creating",
        [repos, creating_payload],
        {
            repos
                .reasoning
                .append_event(conv_id, "container_status", &creating_payload, None, None)
                .await
                .map_err(terminal_err("failed to append event"))?;
        }
    );

    journaled!(ctx, "ensure_pod", [cm], {
        let sid = conv_id.to_string();
        cm.ensure_pod(&sid, &pod_overrides)
            .await
            .map_err(terminal_err("failed to create agent pod"))?;
    });

    // Poll for pod ready (not journaled — best effort on replay).
    let pod_ip = cm
        .wait_for_ready(&conv_id.to_string())
        .await
        .map_err(terminal_err("pod failed to start"))?;

    // Fetch pod name now that it's running.
    let pod_name = match cm
        .get_pod_status(&conv_id.to_string())
        .await
        .map_err(terminal_err("failed to get pod status"))?
    {
        ps_agent::PodStatus::Running { pod_name, .. } => pod_name,
        _ => "unknown".to_string(),
    };

    let ready_payload = serde_json::json!({"status": "ready", "message": "Agent ready"});
    journaled!(ctx, "event_container_ready", [repos, ready_payload], {
        repos
            .reasoning
            .append_event(conv_id, "container_status", &ready_payload, None, None)
            .await
            .map_err(terminal_err("failed to append event"))?;
    });

    Ok((pod_ip, pod_name))
}
