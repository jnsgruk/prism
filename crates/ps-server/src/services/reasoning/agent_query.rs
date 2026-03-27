use ps_proto::canonical::prism::v1::{
    AgentContainerStatus, AgentConversationCreated, AgentError, AgentFinalAnswer,
    AgentPartialAnswer, AgentThinking, AgentToolCallCompleted, AgentToolCallStarted,
    AskQuestionRequest, AskQuestionResponse, ResumeStreamRequest, ResumeStreamResponse,
    ask_question_response,
};
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

    // Create or resume conversation.
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

    // Set query_status to pending.
    svc.repos
        .reasoning
        .update_query_status(conversation_id, "pending")
        .await
        .map_err(db_err)?;

    // Fire-and-forget: trigger the Restate handler.
    let (model_name, provider_keys) = {
        let router = svc.router.read().await;
        let config = router.config();
        let model = format!(
            "{}/{}",
            config.tasks.agentic.provider.as_str(),
            config.tasks.agentic.model
        );
        let keys = router.provider_env_vars();
        (model, keys)
    };

    let trigger_request = serde_json::json!({
        "conversation_id": conversation_id.to_string(),
        "user_id": ctx.user_id.to_string(),
        "question": req.question,
        "model": model_name,
        "small_model": model_name,
        "provider_keys": provider_keys,
    });

    let restate_url = svc.restate_url.clone();
    let cid_str = conversation_id.to_string();
    let http_client = svc.http_client.clone();

    // Trigger handler in background — don't block the stream.
    tokio::spawn(async move {
        let url = format!("{restate_url}/AgenticQueryHandler/{cid_str}/run_query/send",);
        match http_client
            .post(&url)
            .header("content-type", "application/json")
            .body(serde_json::to_string(&trigger_request).unwrap_or_default())
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                info!(conversation_id = %cid_str, "triggered AgenticQueryHandler");
            }
            Ok(resp) => {
                let status = resp.status();
                warn!(conversation_id = %cid_str, %status, "failed to trigger handler");
            }
            Err(e) => {
                warn!(conversation_id = %cid_str, error = %e, "failed to trigger handler");
            }
        }
    });

    // Poll-and-stream: read events from the DB and stream to the client.
    let repos = svc.repos.clone();
    let conv_id = conversation_id;
    let (tx, rx) = tokio::sync::mpsc::channel(64);

    // Send conversation_created as the first event so the client can update
    // the URL and sidebar immediately.
    let conv_title = req.question.chars().take(100).collect::<String>();
    let _ = tx
        .send(Ok(AskQuestionResponse {
            event: Some(ask_question_response::Event::ConversationCreated(
                AgentConversationCreated {
                    conversation_id: conversation_id.to_string(),
                    title: conv_title,
                },
            )),
        }))
        .await;

    tokio::spawn(async move {
        let mut cursor: i64 = 0;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);

        loop {
            if tokio::time::Instant::now() >= deadline {
                let _ = tx
                    .send(Ok(AskQuestionResponse {
                        event: Some(ask_question_response::Event::Error(AgentError {
                            message: "Query timed out".into(),
                            retryable: true,
                        })),
                    }))
                    .await;
                return;
            }
            // Poll for new events.
            match repos.reasoning.poll_events(conv_id, cursor).await {
                Ok(events) => {
                    for event in events {
                        cursor = event.id;

                        let proto_event = map_db_event_to_proto(&event);
                        if let Some(response) = proto_event {
                            let is_terminal = matches!(
                                response.event,
                                Some(
                                    ask_question_response::Event::FinalAnswer(_)
                                        | ask_question_response::Event::Error(_)
                                )
                            );

                            if tx.send(Ok(response)).await.is_err() {
                                return; // Client disconnected.
                            }

                            if is_terminal {
                                return;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "failed to poll conversation events");
                    let _ = tx
                        .send(Ok(AskQuestionResponse {
                            event: Some(ask_question_response::Event::Error(AgentError {
                                message: "Internal error polling events".into(),
                                retryable: true,
                            })),
                        }))
                        .await;
                    return;
                }
            }

            // Check if query reached a terminal status without us seeing the event
            // (can happen when cleanup_events deletes the final_answer before we poll it).
            if let Ok(Some(conv)) = repos.reasoning.get_conversation(conv_id).await {
                match conv.query_status.as_str() {
                    "cancelled" | "failed" => return,
                    "completed" => {
                        // Synthesize a final_answer from the stored assistant message.
                        let answer = repos
                            .reasoning
                            .list_messages(conv_id)
                            .await
                            .ok()
                            .and_then(|msgs| {
                                msgs.into_iter()
                                    .rev()
                                    .find(|m| m.role == "assistant")
                                    .map(|m| m.content)
                            })
                            .unwrap_or_default();
                        let _ = tx
                            .send(Ok(AskQuestionResponse {
                                event: Some(ask_question_response::Event::FinalAnswer(
                                    AgentFinalAnswer {
                                        answer,
                                        conversation_id: conv_id.to_string(),
                                        tool_call_count: conv.total_tool_calls,
                                        ..Default::default()
                                    },
                                )),
                            }))
                            .await;
                        return;
                    }
                    _ => {}
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });

    Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
        rx,
    )))
}

pub type ResumeStreamStream =
    tokio_stream::wrappers::ReceiverStream<Result<ResumeStreamResponse, Status>>;

pub async fn resume_stream(
    svc: &ReasoningServiceImpl,
    request: Request<ResumeStreamRequest>,
) -> Result<Response<ResumeStreamStream>, Status> {
    let ctx = require_auth(&request)?;
    let req = request.into_inner();

    let conv_id = req
        .conversation_id
        .parse::<Uuid>()
        .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

    // Verify conversation exists and belongs to this user.
    let conv = svc
        .repos
        .reasoning
        .get_conversation(conv_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("conversation not found"))?;

    if conv.user_id != ctx.user_id {
        return Err(Status::not_found("conversation not found"));
    }

    let (tx, rx) = tokio::sync::mpsc::channel(64);

    // If query is already terminal, close immediately.
    if matches!(
        conv.query_status.as_str(),
        "completed" | "failed" | "cancelled" | "idle"
    ) {
        drop(tx);
        return Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )));
    }

    // Start the shared poll loop from the requested cursor.
    let repos = svc.repos.clone();
    let cursor = req.last_event_id;

    tokio::spawn(async move {
        stream_resume_events(repos, conv_id, cursor, tx).await;
    });

    Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
        rx,
    )))
}

/// Poll loop for `ResumeStream` — reads events from DB and streams to client.
async fn stream_resume_events(
    repos: ps_core::repo::Repos,
    conv_id: Uuid,
    initial_cursor: i64,
    tx: tokio::sync::mpsc::Sender<Result<ResumeStreamResponse, Status>>,
) {
    use ps_proto::canonical::prism::v1::resume_stream_response;

    let mut cursor = initial_cursor;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);

    loop {
        if tokio::time::Instant::now() >= deadline {
            let _ = tx
                .send(Ok(ResumeStreamResponse {
                    event: Some(resume_stream_response::Event::Error(AgentError {
                        message: "Stream timed out".into(),
                        retryable: true,
                    })),
                }))
                .await;
            return;
        }

        match repos.reasoning.poll_events(conv_id, cursor).await {
            Ok(events) => {
                for event in events {
                    cursor = event.id;
                    let proto_event = map_db_event_to_resume_proto(&event);
                    if let Some(response) = proto_event {
                        let is_terminal = matches!(
                            response.event,
                            Some(
                                resume_stream_response::Event::FinalAnswer(_)
                                    | resume_stream_response::Event::Error(_)
                            )
                        );
                        if tx.send(Ok(response)).await.is_err() {
                            return;
                        }
                        if is_terminal {
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "failed to poll conversation events");
                let _ = tx
                    .send(Ok(ResumeStreamResponse {
                        event: Some(resume_stream_response::Event::Error(AgentError {
                            message: "Internal error polling events".into(),
                            retryable: true,
                        })),
                    }))
                    .await;
                return;
            }
        }

        // Check if query reached a terminal status without us seeing the event.
        if let Ok(Some(conv)) = repos.reasoning.get_conversation(conv_id).await {
            match conv.query_status.as_str() {
                "cancelled" | "failed" => return,
                "completed" => {
                    use ps_proto::canonical::prism::v1::resume_stream_response;
                    let answer = repos
                        .reasoning
                        .list_messages(conv_id)
                        .await
                        .ok()
                        .and_then(|msgs| {
                            msgs.into_iter()
                                .rev()
                                .find(|m| m.role == "assistant")
                                .map(|m| m.content)
                        })
                        .unwrap_or_default();
                    let _ = tx
                        .send(Ok(ResumeStreamResponse {
                            event: Some(resume_stream_response::Event::FinalAnswer(
                                AgentFinalAnswer {
                                    answer,
                                    conversation_id: conv_id.to_string(),
                                    tool_call_count: conv.total_tool_calls,
                                    ..Default::default()
                                },
                            )),
                        }))
                        .await;
                    return;
                }
                _ => {}
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Map a database event to a `ResumeStreamResponse` proto.
fn map_db_event_to_resume_proto(
    event: &ps_core::repo::reasoning::ConversationEvent,
) -> Option<ResumeStreamResponse> {
    use ps_proto::canonical::prism::v1::resume_stream_response;
    let ask_resp = map_db_event_to_proto(event)?;
    let ask_evt = ask_resp.event?;

    let event = match ask_evt {
        ask_question_response::Event::ToolCallStarted(v) => {
            resume_stream_response::Event::ToolCallStarted(v)
        }
        ask_question_response::Event::ToolCallCompleted(v) => {
            resume_stream_response::Event::ToolCallCompleted(v)
        }
        ask_question_response::Event::PartialAnswer(v) => {
            resume_stream_response::Event::PartialAnswer(v)
        }
        ask_question_response::Event::FinalAnswer(v) => {
            resume_stream_response::Event::FinalAnswer(v)
        }
        ask_question_response::Event::Error(v) => resume_stream_response::Event::Error(v),
        ask_question_response::Event::Thinking(v) => resume_stream_response::Event::Thinking(v),
        ask_question_response::Event::ContainerStatus(v) => {
            resume_stream_response::Event::ContainerStatus(v)
        }
        ask_question_response::Event::ArtifactUploaded(v) => {
            resume_stream_response::Event::ArtifactUploaded(v)
        }
        ask_question_response::Event::ConversationCreated(_) => return None,
    };

    Some(ResumeStreamResponse { event: Some(event) })
}

/// Map a database event row to a proto `AskQuestionResponse`.
#[allow(clippy::too_many_lines)]
fn map_db_event_to_proto(
    event: &ps_core::repo::reasoning::ConversationEvent,
) -> Option<AskQuestionResponse> {
    let event_type = &event.event_type;
    let payload = &event.payload;
    let step_id = event.step_id.clone().unwrap_or_default();
    let step_seq = event.step_seq.unwrap_or(0);

    let proto_event = match event_type.as_str() {
        "container_status" => ask_question_response::Event::ContainerStatus(AgentContainerStatus {
            status: payload
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            message: payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "tool_call_started" => {
            ask_question_response::Event::ToolCallStarted(AgentToolCallStarted {
                tool_name: payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                arguments_json: payload
                    .get("arguments_json")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}")
                    .to_string(),
                call_id: payload
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                step_id: step_id.clone(),
                step_seq,
            })
        }
        "tool_call_completed" => {
            ask_question_response::Event::ToolCallCompleted(AgentToolCallCompleted {
                tool_name: payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                result_summary: payload
                    .get("result_summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                duration_ms: payload
                    .get("duration_ms")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .unwrap_or(0),
                success: payload
                    .get("success")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true),
                call_id: payload
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                step_id: step_id.clone(),
                step_seq,
            })
        }
        "partial_answer" => ask_question_response::Event::PartialAnswer(AgentPartialAnswer {
            text: payload
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "thinking" => ask_question_response::Event::Thinking(AgentThinking {
            text: payload
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            part_index: payload
                .get("part_index")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0) as i32,
            step_id,
            step_seq,
        }),
        "artifact_uploaded" => ask_question_response::Event::ArtifactUploaded(
            ps_proto::canonical::prism::v1::AgentArtifactUploaded {
                artifact_id: payload
                    .get("artifact_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                display_name: payload
                    .get("display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                content_type: payload
                    .get("content_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream")
                    .to_string(),
                size_bytes: payload
                    .get("size_bytes")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
                download_url: String::new(),
            },
        ),
        "final_answer" => ask_question_response::Event::FinalAnswer(AgentFinalAnswer {
            answer: payload
                .get("answer")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            conversation_id: payload
                .get("conversation_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            supporting_data_json: String::new(),
            prompt_tokens: 0,
            completion_tokens: 0,
            estimated_cost_usd: 0.0,
            tool_call_count: payload
                .get("tool_call_count")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(0),
            duration_ms: 0,
            artifacts: vec![],
        }),
        "error" => ask_question_response::Event::Error(AgentError {
            message: payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error")
                .to_string(),
            retryable: payload
                .get("retryable")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }),
        _ => return None,
    };

    Some(AskQuestionResponse {
        event: Some(proto_event),
    })
}
