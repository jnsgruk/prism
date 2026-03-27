use ps_core::repo::Repos;
use ps_core::repo::reasoning::{CreateArtifactParams, CreateMessageParams};
use restate_sdk::prelude::*;
use tracing::{error, info, warn};
use uuid::Uuid;

use super::SharedState;

pub struct AgenticQueryHandlerImpl {
    pub state: SharedState,
}

/// Request payload for `run_query`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticQueryRequest {
    pub conversation_id: String,
    pub user_id: String,
    pub question: String,
    pub model: String,
    pub small_model: String,
    pub provider_keys: Vec<(String, String)>,
}

#[restate_sdk::object]
pub trait AgenticQueryHandler {
    /// Run an agentic query for a conversation.
    async fn run_query(request: Json<AgenticQueryRequest>) -> Result<(), TerminalError>;

    /// Cancel a running query.
    async fn cancel() -> Result<(), TerminalError>;
}

impl AgenticQueryHandler for AgenticQueryHandlerImpl {
    #[allow(clippy::too_many_lines)]
    async fn run_query(
        &self,
        ctx: ObjectContext<'_>,
        Json(request): Json<AgenticQueryRequest>,
    ) -> Result<(), TerminalError> {
        let conv_id: Uuid = request
            .conversation_id
            .parse()
            .map_err(|e| TerminalError::new(format!("invalid conversation_id: {e}")))?;
        let user_id: Uuid = request
            .user_id
            .parse()
            .map_err(|e| TerminalError::new(format!("invalid user_id: {e}")))?;

        let span = tracing::info_span!(
            "agentic_query",
            handler = "AgenticQueryHandler",
            conversation_id = %conv_id,
        );
        let _guard = span.enter();

        info!("starting agentic query");

        // Update status to running.
        {
            let repos = self.state.repos.clone();
            let cid = conv_id;
            ctx.run(move || {
                let repos = repos.clone();
                async move {
                    repos
                        .reasoning
                        .update_query_status(cid, "running")
                        .await
                        .map_err(|e| TerminalError::new(format!("failed to update status: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("update_status_running")
            .await?;
        }

        // Ensure pod is running.
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

        {
            let repos = self.state.repos.clone();
            let tsid = token_session_id;
            let th = token_hash;
            ctx.run(move || {
                let repos = repos.clone();
                let th = th.clone();
                async move {
                    repos
                        .auth
                        .create_session(
                            tsid,
                            user_id,
                            &th,
                            "agent_service",
                            Some(time::OffsetDateTime::now_utc() + time::Duration::hours(3)),
                            Some("agent-container"),
                        )
                        .await
                        .map_err(|e| {
                            TerminalError::new(format!("failed to create agent session: {e}"))
                        })?;
                    Ok(Json::from(()))
                }
            })
            .name("create_agent_session")
            .await?;
        }

        let pod_overrides = ps_agent::PodOverrides {
            service_token,
            token_session_id: token_session_id.to_string(),
            model: request.model.clone(),
            small_model: request.small_model.clone(),
            provider_keys: request.provider_keys.clone(),
        };

        // Write container_status creating event.
        {
            let repos = self.state.repos.clone();
            let cid = conv_id;
            ctx.run(move || {
                let repos = repos.clone();
                async move {
                    repos
                        .reasoning
                        .append_event(
                            cid,
                            "container_status",
                            &serde_json::json!({"status": "creating", "message": "Starting agent container..."}),
                            None,
                            None,
                        )
                        .await
                        .map_err(|e| TerminalError::new(format!("failed to append event: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("event_container_creating")
            .await?;
        }

        // Ensure pod (journaled).
        {
            let cm_clone = cm.clone();
            let sid = conv_id.to_string();
            ctx.run(move || {
                let cm_clone = cm_clone.clone();
                let sid = sid.clone();
                async move {
                    cm_clone
                        .ensure_pod(&sid, &pod_overrides)
                        .await
                        .map_err(|e| {
                            TerminalError::new(format!("failed to create agent pod: {e}"))
                        })?;
                    Ok(Json::from(()))
                }
            })
            .name("ensure_pod")
            .await?;
        }

        // Poll for pod ready (not journaled — best effort on replay).
        let pod_ip = wait_for_pod_ready(&cm, &conv_id.to_string())
            .await
            .map_err(|e| TerminalError::new(format!("pod failed to start: {e}")))?;

        // Write container ready event.
        {
            let repos = self.state.repos.clone();
            let cid = conv_id;
            ctx.run(move || {
                let repos = repos.clone();
                async move {
                    repos
                        .reasoning
                        .append_event(
                            cid,
                            "container_status",
                            &serde_json::json!({"status": "ready", "message": "Agent ready"}),
                            None,
                            None,
                        )
                        .await
                        .map_err(|e| TerminalError::new(format!("failed to append event: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("event_container_ready")
            .await?;
        }

        // Run the core query logic (SSE streaming — not journaled).
        let result = run_agentic_query_core(
            &self.state.repos,
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
            Ok(query_result) => {
                // Store assistant message (journaled).
                {
                    let repos = self.state.repos.clone();
                    let cid = conv_id;
                    let answer = query_result.answer_text.clone();
                    let tool_calls = query_result.tool_calls;
                    let trace_steps = query_result.trace_steps;
                    ctx.run(move || {
                        let repos = repos.clone();
                        let answer = answer.clone();
                        let trace_steps = trace_steps;
                        async move {
                            let trace = serde_json::json!({
                                "tool_call_count": tool_calls,
                                "steps": trace_steps,
                            });
                            repos
                                .reasoning
                                .create_message(&CreateMessageParams {
                                    conversation_id: cid,
                                    role: "assistant",
                                    content: &answer,
                                    reasoning_trace: Some(&trace),
                                    supporting_data: None,
                                    prompt_tokens: 0,
                                    completion_tokens: 0,
                                })
                                .await
                                .map_err(|e| {
                                    TerminalError::new(format!("failed to store message: {e}"))
                                })?;
                            Ok(Json::from(()))
                        }
                    })
                    .name("store_message")
                    .await?;
                }

                // Clean up ephemeral events after message is persisted.
                {
                    let repos = self.state.repos.clone();
                    let cid = conv_id;
                    ctx.run(move || {
                        let repos = repos.clone();
                        async move {
                            let _ = repos.reasoning.delete_events(cid).await;
                            Ok(Json::from(()))
                        }
                    })
                    .name("cleanup_events")
                    .await?;
                }

                // Update conversation totals (journaled).
                {
                    let repos = self.state.repos.clone();
                    let cid = conv_id;
                    let tc = query_result.tool_calls;
                    ctx.run(move || {
                        let repos = repos.clone();
                        async move {
                            repos
                                .reasoning
                                .update_conversation_totals(cid, tc, 0, 0, 0.0)
                                .await
                                .map_err(|e| {
                                    TerminalError::new(format!("failed to update totals: {e}"))
                                })?;
                            Ok(Json::from(()))
                        }
                    })
                    .name("update_totals")
                    .await?;
                }

                // Write final_answer event and set status to completed (journaled).
                {
                    let repos = self.state.repos.clone();
                    let cid = conv_id;
                    let answer = query_result.answer_text;
                    let tc = query_result.tool_calls;
                    ctx.run(move || {
                        let repos = repos.clone();
                        let answer = answer.clone();
                        async move {
                            repos
                                .reasoning
                                .append_event(
                                    cid,
                                    "final_answer",
                                    &serde_json::json!({
                                        "answer": answer,
                                        "conversation_id": cid.to_string(),
                                        "tool_call_count": tc,
                                    }),
                                    None,
                                    None,
                                )
                                .await
                                .map_err(|e| {
                                    TerminalError::new(format!("failed to write final event: {e}"))
                                })?;
                            repos
                                .reasoning
                                .update_query_status(cid, "completed")
                                .await
                                .map_err(|e| {
                                    TerminalError::new(format!("failed to update status: {e}"))
                                })?;
                            Ok(Json::from(()))
                        }
                    })
                    .name("finalize")
                    .await?;
                }
            }
            Err(e) => {
                error!(error = %e, "agentic query failed");
                let repos = self.state.repos.clone();
                let cid = conv_id;
                let err_msg = e.to_string();
                ctx.run(move || {
                    let repos = repos.clone();
                    let err_msg = err_msg.clone();
                    async move {
                        repos
                            .reasoning
                            .append_event(
                                cid,
                                "error",
                                &serde_json::json!({"message": err_msg, "retryable": false}),
                                None,
                                None,
                            )
                            .await
                            .map_err(|e| {
                                TerminalError::new(format!("failed to write error event: {e}"))
                            })?;
                        repos
                            .reasoning
                            .update_query_status(cid, "failed")
                            .await
                            .map_err(|e| {
                                TerminalError::new(format!("failed to update status: {e}"))
                            })?;
                        Ok(Json::from(()))
                    }
                })
                .name("fail")
                .await?;
            }
        }

        info!("agentic query complete");
        Ok(())
    }

    async fn cancel(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let session_id = ctx.key().to_string();
        info!(session_id, "cancelling agentic query");

        let conv_id: Uuid = session_id
            .parse()
            .map_err(|e| TerminalError::new(format!("invalid key: {e}")))?;

        let repos = self.state.repos.clone();
        ctx.run(move || {
            let repos = repos.clone();
            async move {
                repos
                    .reasoning
                    .update_query_status(conv_id, "cancelled")
                    .await
                    .map_err(|e| TerminalError::new(format!("failed to update status: {e}")))?;
                Ok(Json::from(()))
            }
        })
        .name("update_status_cancelled")
        .await?;

        Ok(())
    }
}

/// Result of the core query execution.
pub struct QueryResult {
    pub answer_text: String,
    pub tool_calls: i32,
    pub trace_steps: Vec<serde_json::Value>,
}

/// Core agentic query logic: connect to `OpenCode`, stream events, write to DB.
///
/// Extracted as a testable function separate from `ctx.run()` wrappers.
#[allow(clippy::too_many_lines)]
pub async fn run_agentic_query_core(
    repos: &Repos,
    _http_client: &reqwest::Client,
    conversation_id: Uuid,
    pod_ip: &str,
    question: &str,
) -> Result<QueryResult, Box<dyn std::error::Error + Send + Sync>> {
    let conv = repos
        .reasoning
        .get_conversation(conversation_id)
        .await?
        .ok_or("conversation not found")?;

    let client = ps_agent::ContainerManager::opencode_client(pod_ip)?;

    let opencode_session_id = if let Some(ref oc_sid) = conv.opencode_session_id {
        info!(session_id = %oc_sid, "reusing existing OpenCode session");
        oc_sid.clone()
    } else {
        info!("creating new OpenCode session");
        let session = client.create_session_with_title(question).await?;
        info!(session_id = %session.id, "OpenCode session created");
        repos
            .reasoning
            .update_container_status(
                conversation_id,
                conv.container_pod_name.as_deref(),
                "active",
                Some(&session.id),
            )
            .await?;
        session.id
    };

    // Subscribe to events before sending the question.
    info!("subscribing to OpenCode events");
    let mut subscription = client.subscribe().await?;
    info!("SSE subscription established");

    info!("sending question to OpenCode");
    let prompt =
        ps_agent::opencode_sdk::types::message::PromptRequest::text(question).with_agent("prism");
    client
        .messages()
        .prompt_async(&opencode_session_id, &prompt)
        .await?;
    info!("question sent, streaming events");

    // Stream events until idle or timeout (5 minutes).
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
    let mut answer_text = String::new();
    let mut tool_calls = 0i32;
    let mut registry = super::step_registry::StepRegistry::new();

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        let event = match tokio::time::timeout(remaining, subscription.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(_) => {
                warn!("SSE stream timed out");
                break;
            }
        };

        if matches!(
            event,
            ps_agent::opencode_sdk::types::event::Event::SessionIdle { .. }
        ) {
            info!("session idle, finishing");
            break;
        }

        // Intercept artifact uploads.
        if let ps_agent::opencode_sdk::types::event::Event::MessagePartUpdated { properties } =
            &event
            && let Some(ps_agent::opencode_sdk::types::message::Part::Tool {
                tool,
                state: Some(ps_agent::opencode_sdk::types::message::ToolState::Completed(completed)),
                ..
            }) = properties.part.as_ref()
            && tool == "prism_upload_artifact"
            && let Ok(result) = serde_json::from_str::<serde_json::Value>(&completed.output)
        {
            let raw_key = result
                .get("artifact_key")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let artifact_key = raw_key.strip_prefix("conversations/").unwrap_or(raw_key);
            let display_name = result
                .get("display_name")
                .and_then(|v| v.as_str())
                .unwrap_or("artifact");
            let content_type = result.get("content_type").and_then(|v| v.as_str());
            let size_bytes = result
                .get("size_bytes")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);

            match repos
                .reasoning
                .create_artifact(&CreateArtifactParams {
                    conversation_id,
                    message_id: None,
                    artifact_key,
                    display_name,
                    content_type,
                    size_bytes,
                })
                .await
            {
                Ok(artifact) => {
                    let _ = repos
                        .reasoning
                        .append_event(
                            conversation_id,
                            "artifact_uploaded",
                            &serde_json::json!({
                                "artifact_id": artifact.id.to_string(),
                                "display_name": display_name,
                                "content_type": content_type.unwrap_or("application/octet-stream"),
                                "size_bytes": size_bytes,
                            }),
                            None,
                            None,
                        )
                        .await;
                }
                Err(e) => {
                    warn!(error = %e, "failed to register artifact in DB");
                }
            }
        }

        // Map event to proto and write to DB.
        if let Some(proto_event) = ps_agent::event_mapper::map_event(&event)
            && let Some(ref evt) = proto_event.event
        {
            use ps_proto::canonical::prism::v1::ask_question_response::Event;
            match evt {
                Event::ToolCallStarted(s) => {
                    let identity = registry.tool_started(&s.call_id);
                    let _ = repos
                        .reasoning
                        .append_event(
                            conversation_id,
                            "tool_call_started",
                            &serde_json::json!({
                                "tool_name": s.tool_name,
                                "arguments_json": s.arguments_json,
                                "call_id": s.call_id,
                            }),
                            Some(&identity.step_id),
                            Some(identity.step_seq),
                        )
                        .await;
                }
                Event::ToolCallCompleted(c) => {
                    tool_calls += 1;
                    let identity = registry.tool_completed(&c.call_id);
                    let _ = repos
                        .reasoning
                        .append_event(
                            conversation_id,
                            "tool_call_completed",
                            &serde_json::json!({
                                "tool_name": c.tool_name,
                                "result_summary": c.result_summary,
                                "duration_ms": c.duration_ms,
                                "success": c.success,
                                "call_id": c.call_id,
                            }),
                            Some(&identity.step_id),
                            Some(identity.step_seq),
                        )
                        .await;
                }
                Event::PartialAnswer(a) => {
                    answer_text.clone_from(&a.text);
                    let _ = repos
                        .reasoning
                        .append_event(
                            conversation_id,
                            "partial_answer",
                            &serde_json::json!({"text": a.text}),
                            None,
                            None,
                        )
                        .await;
                }
                Event::Thinking(t) => {
                    let identity = registry.thinking_step(t.part_index, &t.text);
                    let _ = repos
                        .reasoning
                        .append_event(
                            conversation_id,
                            "thinking",
                            &serde_json::json!({"text": t.text, "part_index": t.part_index}),
                            Some(&identity.step_id),
                            Some(identity.step_seq),
                        )
                        .await;
                }
                Event::Error(e) => {
                    let _ = repos
                        .reasoning
                        .append_event(
                            conversation_id,
                            "error",
                            &serde_json::json!({
                                "message": e.message,
                                "retryable": e.retryable,
                            }),
                            None,
                            None,
                        )
                        .await;
                }
                _ => {}
            }
        }
    }

    // Derive the reasoning trace from all conversation events.
    let all_events = repos
        .reasoning
        .get_all_events(conversation_id)
        .await
        .unwrap_or_default();
    let trace_steps = derive_trace_from_events(&all_events);

    Ok(QueryResult {
        answer_text,
        tool_calls,
        trace_steps,
    })
}

/// Derive the reasoning trace from conversation events.
/// This produces the same structure as the frontend's `deriveSteps()`.
fn derive_trace_from_events(
    events: &[ps_core::repo::reasoning::ConversationEvent],
) -> Vec<serde_json::Value> {
    use std::collections::BTreeMap;

    // BTreeMap keyed by step_seq for deterministic ordering.
    let mut steps: BTreeMap<i32, serde_json::Value> = BTreeMap::new();

    for event in events {
        let Some(step_seq) = event.step_seq else {
            continue;
        };
        let Some(ref step_id) = event.step_id else {
            continue;
        };

        match event.event_type.as_str() {
            "thinking" => {
                let text = event
                    .payload
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let part_index = event
                    .payload
                    .get("part_index")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                // Always overwrite — later events have more complete text.
                steps.insert(
                    step_seq,
                    serde_json::json!({
                        "kind": "reasoning",
                        "text": text,
                        "part_index": part_index,
                        "step_id": step_id,
                    }),
                );
            }
            "tool_call_started" => {
                let tool_name = event
                    .payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let call_id = event
                    .payload
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let args = event
                    .payload
                    .get("arguments_json")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                steps.entry(step_seq).or_insert_with(|| {
                    serde_json::json!({
                        "kind": "tool",
                        "tool_name": tool_name,
                        "call_id": call_id,
                        "arguments": args,
                        "step_id": step_id,
                    })
                });
            }
            "tool_call_completed" => {
                if let Some(step) = steps.get_mut(&step_seq)
                    && let Some(obj) = step.as_object_mut()
                {
                    obj.insert(
                        "result_summary".into(),
                        event
                            .payload
                            .get("result_summary")
                            .cloned()
                            .unwrap_or_default(),
                    );
                    obj.insert(
                        "duration_ms".into(),
                        event
                            .payload
                            .get("duration_ms")
                            .cloned()
                            .unwrap_or_default(),
                    );
                    obj.insert(
                        "success".into(),
                        event
                            .payload
                            .get("success")
                            .cloned()
                            .unwrap_or(serde_json::Value::Bool(true)),
                    );
                }
            }
            _ => {}
        }
    }

    steps.into_values().collect()
}

/// Poll for Pod readiness. Returns the pod IP on success.
async fn wait_for_pod_ready(
    cm: &ps_agent::ContainerManager,
    session_id: &str,
) -> Result<String, String> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

    loop {
        interval.tick().await;
        if tokio::time::Instant::now() >= deadline {
            return Err("timed out waiting for agent container".into());
        }

        match cm.get_pod_status(session_id).await {
            Ok(ps_agent::PodStatus::Running { pod_ip, .. }) => return Ok(pod_ip),
            Ok(ps_agent::PodStatus::Pending) => {}
            Ok(ps_agent::PodStatus::Gone) => {
                return Err("agent container failed to start".into());
            }
            Err(e) => {
                return Err(format!("error checking container status: {e}"));
            }
        }
    }
}
