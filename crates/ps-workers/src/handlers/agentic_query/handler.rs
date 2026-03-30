use ps_core::repo::Repos;
use ps_core::repo::reasoning::CreateMessageParams;
use restate_sdk::prelude::*;
use tracing::{error, info, warn};
use uuid::Uuid;

use super::AgenticQueryRequest;
use super::SharedState;
use super::query_core::{self, QueryResult};
use crate::handlers::run_lifecycle::{journaled, terminal_err};

pub struct AgenticQueryHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::object]
pub trait AgenticQueryHandler {
    /// Run an agentic query for a conversation.
    async fn run_query(request: Json<AgenticQueryRequest>) -> Result<(), TerminalError>;

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
    async fn run_query(
        &self,
        ctx: ObjectContext<'_>,
        Json(request): Json<AgenticQueryRequest>,
    ) -> Result<(), TerminalError> {
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

        info!("starting agentic query");

        let repos = &self.state.repos;

        journaled!(ctx, "update_status_running", [repos], {
            repos
                .reasoning
                .update_query_status(conv_id, "running")
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
        };

        let pod_ip = start_pod(&ctx, repos, &cm, conv_id, pod_overrides).await?;

        // Run the core query logic (SSE streaming — not journaled).
        let result = query_core::run_agentic_query_core(
            repos,
            &self.state.http_client,
            conv_id,
            &pod_ip,
            &request.question,
        )
        .await;

        // Update pod activity to prevent premature reaping.
        if let Err(e) = cm.update_activity(&conv_id.to_string()).await {
            warn!(error = %e, "failed to update pod activity");
        }

        match result {
            Ok(query_result) => finalize_success(&ctx, repos, conv_id, query_result).await?,
            Err(e) => finalize_failure(&ctx, repos, conv_id, e).await?,
        }

        info!("agentic query complete");
        Ok(())
    }

    async fn cancel(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let session_id = ctx.key().to_string();
        info!(session_id, "cancelling agentic query");

        let conv_id: Uuid = session_id.parse().map_err(terminal_err("invalid key"))?;

        let repos = &self.state.repos;
        journaled!(ctx, "update_status_cancelled", [repos], {
            repos
                .reasoning
                .update_query_status(conv_id, "cancelled")
                .await
                .map_err(terminal_err("failed to update status"))?;
        });

        Ok(())
    }

    async fn cleanup_storage(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let session_id = ctx.key().to_string();
        info!(session_id, "cleaning up pod and workspace PVC");

        if let Some(ref cm) = self.state.container_manager {
            if let Err(e) = cm.delete_pod(&session_id).await {
                warn!(error = %e, "failed to delete pod during cleanup");
            }
            if let Err(e) = cm.delete_pvc(&session_id).await {
                warn!(error = %e, "failed to delete workspace PVC during cleanup");
            }
        }

        Ok(())
    }
}

/// Create the agent pod and wait for it to become ready, emitting container
/// status events along the way. Returns the pod IP.
async fn start_pod(
    ctx: &ObjectContext<'_>,
    repos: &Repos,
    cm: &ps_agent::ContainerManager,
    conv_id: Uuid,
    pod_overrides: ps_agent::PodOverrides,
) -> Result<String, TerminalError> {
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

    let ready_payload = serde_json::json!({"status": "ready", "message": "Agent ready"});
    journaled!(ctx, "event_container_ready", [repos, ready_payload], {
        repos
            .reasoning
            .append_event(conv_id, "container_status", &ready_payload, None, None)
            .await
            .map_err(terminal_err("failed to append event"))?;
    });

    Ok(pod_ip)
}

/// Store message, update totals, write final event, and clean up.
async fn finalize_success(
    ctx: &ObjectContext<'_>,
    repos: &Repos,
    conv_id: Uuid,
    query_result: QueryResult,
) -> Result<(), TerminalError> {
    let answer = query_result.answer_text.clone();
    let tool_calls = query_result.tool_calls;
    let trace_steps = query_result.trace_steps;
    let input_tok = query_result.input_tokens as i32;
    let output_tok = query_result.output_tokens as i32;

    journaled!(ctx, "store_message", [repos, answer, trace_steps], {
        let trace = serde_json::json!({
            "tool_call_count": tool_calls,
            "steps": trace_steps,
        });
        repos
            .reasoning
            .create_message(&CreateMessageParams {
                conversation_id: conv_id,
                role: "assistant",
                content: &answer,
                reasoning_trace: Some(&trace),
                supporting_data: None,
                prompt_tokens: input_tok,
                completion_tokens: output_tok,
            })
            .await
            .map_err(terminal_err("failed to store message"))?;
    });

    let tc = query_result.tool_calls;
    let pt = query_result.input_tokens as i32;
    let ct = query_result.output_tokens as i32;
    journaled!(ctx, "update_totals", [repos], {
        repos
            .reasoning
            .update_conversation_totals(conv_id, tc, pt, ct, 0.0)
            .await
            .map_err(terminal_err("failed to update totals"))?;
    });

    let answer = query_result.answer_text;
    let fa_input = query_result.input_tokens;
    let fa_output = query_result.output_tokens;
    journaled!(ctx, "finalize", [repos, answer], {
        repos
            .reasoning
            .append_event(
                conv_id,
                "final_answer",
                &serde_json::json!({
                    "answer": answer,
                    "conversation_id": conv_id.to_string(),
                    "tool_call_count": tc,
                    "prompt_tokens": fa_input,
                    "completion_tokens": fa_output,
                }),
                None,
                None,
            )
            .await
            .map_err(terminal_err("failed to write final event"))?;
        repos
            .reasoning
            .update_query_status(conv_id, "completed")
            .await
            .map_err(terminal_err("failed to update status"))?;
    });

    journaled!(ctx, "cleanup_events", [repos], {
        let _ = repos.reasoning.delete_events(conv_id).await;
    });

    Ok(())
}

/// Write an error event and mark the query as failed.
async fn finalize_failure(
    ctx: &ObjectContext<'_>,
    repos: &Repos,
    conv_id: Uuid,
    err: Box<dyn std::error::Error + Send + Sync>,
) -> Result<(), TerminalError> {
    error!(error = %err, "agentic query failed");
    let err_msg = err.to_string();
    journaled!(ctx, "fail", [repos, err_msg], {
        repos
            .reasoning
            .append_event(
                conv_id,
                "error",
                &serde_json::json!({"message": err_msg, "retryable": false}),
                None,
                None,
            )
            .await
            .map_err(terminal_err("failed to write error event"))?;
        repos
            .reasoning
            .update_query_status(conv_id, "failed")
            .await
            .map_err(terminal_err("failed to update status"))?;
    });
    Ok(())
}
