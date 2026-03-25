use ps_proto::canonical::prism::v1::{AskQuestionRequest, AskQuestionResponse};
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};
use uuid::Uuid;

use super::super::common::{db_err, require_auth};
use super::ReasoningServiceImpl;

pub type AskQuestionStream =
    tokio_stream::wrappers::ReceiverStream<Result<AskQuestionResponse, Status>>;

#[allow(clippy::too_many_lines)]
pub async fn ask_question(
    svc: &ReasoningServiceImpl,
    request: Request<AskQuestionRequest>,
) -> Result<Response<AskQuestionStream>, Status> {
    use ps_agent::event_mapper;
    use ps_core::repo::reasoning::{CreateConversationParams, CreateMessageParams};

    let ctx = require_auth(&request)?;
    let req = request.into_inner();

    // Validate question.
    if req.question.trim().is_empty() {
        return Err(Status::invalid_argument("question must not be empty"));
    }
    if req.question.len() > 4000 {
        return Err(Status::invalid_argument(
            "question must be at most 4000 characters",
        ));
    }

    let cm = svc
        .container_manager
        .as_ref()
        .ok_or_else(|| Status::unavailable("agent containers not configured"))?
        .clone();

    // Create or resume conversation — fetch existing if conversation_id provided.
    let existing_conv = if let Some(ref id) = req.conversation_id {
        let conv_id = id
            .parse::<Uuid>()
            .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;
        svc.repos
            .reasoning
            .get_conversation(conv_id)
            .await
            .map_err(db_err)?
    } else {
        None
    };

    let conversation_id = if let Some(ref conv) = existing_conv {
        conv.id
    } else {
        let model_name = {
            let router = svc.router.read().await;
            let config = router.config();
            format!(
                "{}/{}",
                config.tasks.agentic.provider.as_str(),
                config.tasks.agentic.model
            )
        };
        let conv = svc
            .repos
            .reasoning
            .create_conversation(&CreateConversationParams {
                user_id: ctx.user_id,
                title: Some(&req.question.chars().take(100).collect::<String>()),
                model_name: &model_name,
            })
            .await
            .map_err(db_err)?;
        conv.id
    };

    // Store the user message.
    svc.repos
        .reasoning
        .create_message(&CreateMessageParams {
            conversation_id,
            role: "user",
            content: &req.question,
            reasoning_trace: None,
            supporting_data: None,
            prompt_tokens: 0,
            completion_tokens: 0,
        })
        .await
        .map_err(db_err)?;

    let repos = svc.repos.clone();
    let conv_id = conversation_id;

    // Extract existing pod/session info for session reuse.
    let existing_pod_name = existing_conv
        .as_ref()
        .and_then(|c| c.container_pod_name.clone());
    let existing_opencode_session = existing_conv
        .as_ref()
        .and_then(|c| c.opencode_session_id.clone());

    // Build per-pod overrides: service token + model config + provider keys.
    let service_token = ps_core::auth::generate_token();
    let token_hash = ps_core::auth::hash_token(&service_token);
    let token_session_id = Uuid::now_v7();
    svc.repos
        .auth
        .create_session(
            token_session_id,
            ctx.user_id,
            &token_hash,
            "agent_service",
            Some(time::OffsetDateTime::now_utc() + time::Duration::hours(3)),
            Some("agent-container"),
        )
        .await
        .map_err(db_err)?;

    let pod_overrides = {
        let router = svc.router.read().await;
        let config = router.config();
        let model = format!(
            "{}/{}",
            config.tasks.agentic.provider.as_str(),
            config.tasks.agentic.model
        );
        ps_agent::PodOverrides {
            service_token,
            token_session_id: token_session_id.to_string(),
            model: model.clone(),
            small_model: model,
            provider_keys: router.provider_env_vars(),
        }
    };

    let (tx, rx) = tokio::sync::mpsc::channel(64);

    // Spawn the streaming orchestration task.
    tokio::spawn(async move {
        // 1. Ensure container is running.
        let _ = tx
            .send(Ok(event_mapper::container_status_event(
                "creating",
                "Starting agent container...",
            )))
            .await;

        if let Err(e) = cm.ensure_pod(&conv_id.to_string(), &pod_overrides).await {
            error!(error = %e, "Failed to create agent pod");
            let _ = tx
                .send(Ok(event_mapper::container_status_event(
                    "error",
                    &format!("Failed to create container: {e}"),
                )))
                .await;
            return;
        }

        // 2. Wait for pod to become ready (poll for up to 60s).
        let Some((pod_ip, pod_name)) = wait_for_pod_ready(&cm, &conv_id.to_string(), &tx).await
        else {
            return; // Error already sent on channel.
        };

        let _ = tx
            .send(Ok(event_mapper::container_status_event(
                "ready",
                "Agent ready",
            )))
            .await;

        // Detect stale session: if the pod was recreated (name changed),
        // the old OpenCode session is invalid — force a new one.
        let mut reusable_session = existing_opencode_session.clone();
        if let Some(ref old_name) = existing_pod_name
            && *old_name != pod_name
        {
            info!(
                old_pod = %old_name,
                new_pod = %pod_name,
                "Pod recreated, clearing stale OpenCode session"
            );
            reusable_session = None;
            // Clear the stale session ID in the DB.
            let _ = repos
                .reasoning
                .update_container_status(conv_id, Some(&pod_name), "active", None)
                .await;
        }

        // 3. Connect to OpenCode and stream.
        info!(pod_ip = %pod_ip, "Connecting to OpenCode");
        let client = match ps_agent::ContainerManager::opencode_client(&pod_ip) {
            Ok(c) => c,
            Err(e) => {
                error!(error = %e, "Failed to create OpenCode client");
                let _ = tx
                    .send(Ok(event_mapper::container_status_event(
                        "error",
                        &format!("Failed to connect to agent: {e}"),
                    )))
                    .await;
                return;
            }
        };

        // Reuse existing OpenCode session for follow-up questions, or create new.
        let opencode_session_id = if let Some(ref oc_sid) = reusable_session {
            info!(session_id = %oc_sid, "Reusing existing OpenCode session");
            oc_sid.clone()
        } else {
            info!("Creating new OpenCode session");
            match client.create_session_with_title(&req.question).await {
                Ok(s) => {
                    info!(session_id = %s.id, "OpenCode session created");
                    // Store the session ID so follow-up questions reuse it.
                    let _ = repos
                        .reasoning
                        .update_container_status(conv_id, Some(&pod_name), "active", Some(&s.id))
                        .await;
                    s.id
                }
                Err(e) => {
                    error!(error = %e, "Failed to create OpenCode session");
                    let _ = tx
                        .send(Ok(event_mapper::container_status_event(
                            "error",
                            &format!("Failed to create agent session: {e}"),
                        )))
                        .await;
                    return;
                }
            }
        };

        // 4. Subscribe to global events (not session-filtered) before sending prompt.
        // Session-filtered subscription via subscribe_session can miss events
        // that arrive before the router fully initialises.
        info!("Subscribing to OpenCode events");
        let mut subscription = match client.subscribe().await {
            Ok(s) => {
                info!("SSE subscription established");
                s
            }
            Err(e) => {
                error!(error = %e, "Failed to subscribe to OpenCode events");
                let _ = tx
                    .send(Ok(event_mapper::container_status_event(
                        "error",
                        &format!("Failed to subscribe to agent events: {e}"),
                    )))
                    .await;
                return;
            }
        };

        // 5. Send the question, specifying the "prism" agent so OpenCode
        //    uses our custom system prompt and MCP tool configuration.
        info!("Sending question to OpenCode");
        let prompt = ps_agent::opencode_sdk::types::message::PromptRequest::text(&req.question)
            .with_agent("prism");
        if let Err(e) = client
            .messages()
            .prompt_async(&opencode_session_id, &prompt)
            .await
        {
            error!(error = %e, "Failed to send prompt to OpenCode");
            let _ = tx
                .send(Ok(event_mapper::container_status_event(
                    "error",
                    &format!("Failed to send question: {e}"),
                )))
                .await;
            return;
        }
        info!("Question sent, streaming events");

        // 6. Stream events until idle or timeout (5 minutes — long scripts may need it).
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
        let mut answer_text = String::new();
        let mut tool_calls = 0i32;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            let event = match tokio::time::timeout(remaining, subscription.recv()).await {
                Ok(Some(event)) => event,
                Ok(None) => {
                    let stats = subscription.stats();
                    info!(
                        events_in = stats.events_in,
                        events_out = stats.events_out,
                        reconnects = stats.reconnects,
                        "SSE subscription closed (None)"
                    );
                    break;
                }
                Err(_) => {
                    let stats = subscription.stats();
                    warn!(
                        events_in = stats.events_in,
                        events_out = stats.events_out,
                        reconnects = stats.reconnects,
                        "SSE stream timed out"
                    );
                    break;
                }
            };

            // Check for idle/completion.
            if matches!(
                event,
                ps_agent::opencode_sdk::types::event::Event::SessionIdle { .. }
            ) {
                info!("Session idle, finishing");
                break;
            }

            // Intercept artifact uploads: when the upload_artifact MCP tool
            // completes, register the artifact in the DB and emit an event.
            if let ps_agent::opencode_sdk::types::event::Event::MessagePartUpdated { properties } =
                &event
                && let Some(ps_agent::opencode_sdk::types::message::Part::Tool {
                    tool,
                    state:
                        Some(ps_agent::opencode_sdk::types::message::ToolState::Completed(completed)),
                    ..
                }) = properties.part.as_ref()
                && tool == "prism_upload_artifact"
                && let Ok(result) = serde_json::from_str::<serde_json::Value>(&completed.output)
            {
                // The MCP tool returns keys like "conversations/{session}/{file}"
                // but ArtifactKey::new(Conversations, path) already prepends
                // "conversations/", so strip the prefix to avoid doubling it.
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
                    .create_artifact(&ps_core::repo::reasoning::CreateArtifactParams {
                        conversation_id: conv_id,
                        message_id: None,
                        artifact_key,
                        display_name,
                        content_type,
                        size_bytes,
                    })
                    .await
                {
                    Ok(artifact) => {
                        let uploaded = ps_proto::canonical::prism::v1::AgentArtifactUploaded {
                            artifact_id: artifact.id.to_string(),
                            display_name: display_name.to_string(),
                            content_type: content_type
                                .unwrap_or("application/octet-stream")
                                .to_string(),
                            size_bytes,
                            download_url: String::new(),
                        };
                        let _ = tx
                            .send(Ok(AskQuestionResponse {
                                event: Some(
                                    ps_proto::canonical::prism::v1::ask_question_response::Event::ArtifactUploaded(uploaded),
                                ),
                            }))
                            .await;
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to register artifact in DB");
                    }
                }
            }

            // Map to proto and send.
            if let Some(proto_event) = event_mapper::map_event(&event) {
                // Track answer text and tool calls.
                if let Some(ref evt) = proto_event.event {
                    match evt {
                        ps_proto::canonical::prism::v1::ask_question_response::Event::PartialAnswer(a) => {
                            answer_text.clone_from(&a.text);
                        }
                        ps_proto::canonical::prism::v1::ask_question_response::Event::ToolCallCompleted(_) => {
                            tool_calls += 1;
                        }
                        _ => {}
                    }
                }
                if tx.send(Ok(proto_event)).await.is_err() {
                    break; // Client disconnected.
                }
            }
        }

        // Update pod activity to prevent premature reaping.
        if let Err(e) = cm.update_activity(&conv_id.to_string()).await {
            warn!(error = %e, "Failed to update pod activity");
        }

        // 7. Store assistant message and update totals.
        let trace = serde_json::json!({
            "tool_call_count": tool_calls,
        });
        let _ = repos
            .reasoning
            .create_message(&CreateMessageParams {
                conversation_id: conv_id,
                role: "assistant",
                content: &answer_text,
                reasoning_trace: Some(&trace),
                supporting_data: None,
                prompt_tokens: 0,
                completion_tokens: 0,
            })
            .await;
        let _ = repos
            .reasoning
            .update_conversation_totals(conv_id, tool_calls, 0, 0, 0.0)
            .await;

        // 8. Send final answer.
        let _ = tx
            .send(Ok(AskQuestionResponse {
                event: Some(
                    ps_proto::canonical::prism::v1::ask_question_response::Event::FinalAnswer(
                        ps_proto::canonical::prism::v1::AgentFinalAnswer {
                            answer: answer_text,
                            conversation_id: conv_id.to_string(),
                            supporting_data_json: String::new(),
                            prompt_tokens: 0,
                            completion_tokens: 0,
                            estimated_cost_usd: 0.0,
                            tool_call_count: tool_calls,
                            duration_ms: 0,
                            artifacts: vec![],
                        },
                    ),
                ),
            }))
            .await;
    });

    Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
        rx,
    )))
}

/// Poll for Pod readiness with backoff, sending status events on the channel.
///
/// Returns `(pod_ip, pod_name)` on success, or `None` if the pod failed to start.
async fn wait_for_pod_ready(
    cm: &ps_agent::ContainerManager,
    session_id: &str,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, Status>>,
) -> Option<(String, String)> {
    use ps_agent::{PodStatus, event_mapper};

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

    loop {
        interval.tick().await;
        if tokio::time::Instant::now() >= deadline {
            let _ = tx
                .send(Ok(event_mapper::container_status_event(
                    "error",
                    "Timed out waiting for agent container",
                )))
                .await;
            return None;
        }

        match cm.get_pod_status(session_id).await {
            Ok(PodStatus::Running { pod_ip, pod_name }) => return Some((pod_ip, pod_name)),
            Ok(PodStatus::Pending) => {}
            Ok(PodStatus::Gone) => {
                let _ = tx
                    .send(Ok(event_mapper::container_status_event(
                        "error",
                        "Agent container failed to start",
                    )))
                    .await;
                return None;
            }
            Err(e) => {
                error!(error = %e, "Error checking pod status");
                let _ = tx
                    .send(Ok(event_mapper::container_status_event(
                        "error",
                        &format!("Error checking container status: {e}"),
                    )))
                    .await;
                return None;
            }
        }
    }
}
