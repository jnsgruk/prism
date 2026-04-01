use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::{AskQuestionResponse, ask_question_response};
use tracing::{info, warn};
use uuid::Uuid;

use super::artifact;
use super::step_registry::StepRegistry;

/// Accumulated results from streaming the SSE event loop.
pub struct EventLoopResult {
    pub answer_text: String,
    pub tool_calls: i32,
    pub total_input: u64,
    pub total_output: u64,
}

/// Stream SSE events from the `OpenCode` subscription, writing each event to
/// the database (for `resume_stream`) and sending to the gRPC response channel.
///
/// Returns when the session becomes idle, the stream closes, the timeout
/// elapses, or the client disconnects.
pub async fn run_event_loop(
    repos: &Repos,
    subscription: &mut ps_agent::opencode_sdk::sse::SseSubscription,
    conversation_id: Uuid,
    timeout: std::time::Duration,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, tonic::Status>>,
) -> EventLoopResult {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut answer_text = String::new();
    let mut tool_calls = 0i32;
    let mut registry = StepRegistry::new();
    let mut event_mapper = ps_agent::event_mapper::EventMapper::new();
    let mut seen_work = false;

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
            if seen_work {
                info!("session idle, finishing");
                break;
            }
            continue;
        }

        seen_work = true;

        // Intercept artifact uploads.
        artifact::handle_artifact_upload(repos, conversation_id, &event).await;

        // Map event to proto and write to DB + send to client.
        if let Some(proto_event) = event_mapper.map_event(&event)
            && let Some(ref evt) = proto_event.event
        {
            write_and_send_event(
                repos,
                conversation_id,
                evt,
                &mut registry,
                &mut tool_calls,
                &mut answer_text,
                tx,
            )
            .await;
        }
    }

    let (total_input, total_output) = event_mapper.token_totals();

    EventLoopResult {
        answer_text,
        tool_calls,
        total_input,
        total_output,
    }
}

/// Process a single mapped proto event: write it to the database, send to the
/// gRPC stream, and update running counters.
#[allow(clippy::too_many_lines)]
async fn write_and_send_event(
    repos: &Repos,
    conversation_id: Uuid,
    evt: &ask_question_response::Event,
    registry: &mut StepRegistry,
    tool_calls: &mut i32,
    answer_text: &mut String,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, tonic::Status>>,
) {
    use ask_question_response::Event;
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
            let _ = tx
                .send(Ok(AskQuestionResponse {
                    event: Some(Event::ToolCallStarted(
                        ps_proto::canonical::prism::v1::AgentToolCallStarted {
                            tool_name: s.tool_name.clone(),
                            arguments_json: s.arguments_json.clone(),
                            call_id: s.call_id.clone(),
                            step_id: identity.step_id,
                            step_seq: identity.step_seq,
                        },
                    )),
                }))
                .await;
        }
        Event::ToolCallCompleted(c) => {
            *tool_calls += 1;
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
            let _ = tx
                .send(Ok(AskQuestionResponse {
                    event: Some(Event::ToolCallCompleted(
                        ps_proto::canonical::prism::v1::AgentToolCallCompleted {
                            tool_name: c.tool_name.clone(),
                            result_summary: c.result_summary.clone(),
                            duration_ms: c.duration_ms,
                            success: c.success,
                            call_id: c.call_id.clone(),
                            step_id: identity.step_id,
                            step_seq: identity.step_seq,
                        },
                    )),
                }))
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
            let _ = tx
                .send(Ok(AskQuestionResponse {
                    event: Some(Event::PartialAnswer(a.clone())),
                }))
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
            let _ = tx
                .send(Ok(AskQuestionResponse {
                    event: Some(Event::Thinking(
                        ps_proto::canonical::prism::v1::AgentThinking {
                            text: t.text.clone(),
                            part_index: t.part_index,
                            step_id: identity.step_id,
                            step_seq: identity.step_seq,
                        },
                    )),
                }))
                .await;
        }
        Event::TokenUsage(t) => {
            let _ = repos
                .reasoning
                .append_event(
                    conversation_id,
                    "token_usage",
                    &serde_json::json!({
                        "input_tokens": t.input_tokens,
                        "output_tokens": t.output_tokens,
                        "context_window": t.context_window,
                    }),
                    None,
                    None,
                )
                .await;
            let _ = tx
                .send(Ok(AskQuestionResponse {
                    event: Some(Event::TokenUsage(*t)),
                }))
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
            let _ = tx
                .send(Ok(AskQuestionResponse {
                    event: Some(Event::Error(e.clone())),
                }))
                .await;
        }
        _ => {}
    }
}
