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
    /// `true` when the loop exited because the SSE deadline elapsed.
    pub timed_out: bool,
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
    let mut timed_out = false;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }

        let event = match tokio::time::timeout(remaining, subscription.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(_) => {
                warn!("SSE stream timed out");
                timed_out = true;
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
        timed_out,
    }
}

/// Process a single mapped proto event: write it to the database, send to the
/// gRPC stream, and update running counters.
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
            handle_tool_call_started(repos, conversation_id, s, registry, tx).await;
        }
        Event::ToolCallCompleted(c) => {
            *tool_calls += 1;
            handle_tool_call_completed(repos, conversation_id, c, registry, tx).await;
        }
        Event::PartialAnswer(a) => {
            answer_text.clone_from(&a.text);
            handle_partial_answer(repos, conversation_id, a, tx).await;
        }
        Event::Thinking(t) => {
            handle_thinking(repos, conversation_id, t, registry, tx).await;
        }
        Event::TokenUsage(t) => {
            handle_token_usage(repos, conversation_id, t, tx).await;
        }
        Event::Error(e) => {
            handle_error(repos, conversation_id, e, tx).await;
        }
        _ => {}
    }
}

async fn handle_tool_call_started(
    repos: &Repos,
    conversation_id: Uuid,
    s: &ps_proto::canonical::prism::v1::AgentToolCallStarted,
    registry: &mut StepRegistry,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, tonic::Status>>,
) {
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
            event: Some(ask_question_response::Event::ToolCallStarted(
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

async fn handle_tool_call_completed(
    repos: &Repos,
    conversation_id: Uuid,
    c: &ps_proto::canonical::prism::v1::AgentToolCallCompleted,
    registry: &mut StepRegistry,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, tonic::Status>>,
) {
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
            event: Some(ask_question_response::Event::ToolCallCompleted(
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

async fn handle_partial_answer(
    repos: &Repos,
    conversation_id: Uuid,
    a: &ps_proto::canonical::prism::v1::AgentPartialAnswer,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, tonic::Status>>,
) {
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
            event: Some(ask_question_response::Event::PartialAnswer(a.clone())),
        }))
        .await;
}

async fn handle_thinking(
    repos: &Repos,
    conversation_id: Uuid,
    t: &ps_proto::canonical::prism::v1::AgentThinking,
    registry: &mut StepRegistry,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, tonic::Status>>,
) {
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
            event: Some(ask_question_response::Event::Thinking(
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

async fn handle_token_usage(
    repos: &Repos,
    conversation_id: Uuid,
    t: &ps_proto::canonical::prism::v1::AgentTokenUsage,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, tonic::Status>>,
) {
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
            event: Some(ask_question_response::Event::TokenUsage(*t)),
        }))
        .await;
}

async fn handle_error(
    repos: &Repos,
    conversation_id: Uuid,
    e: &ps_proto::canonical::prism::v1::AgentError,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, tonic::Status>>,
) {
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
            event: Some(ask_question_response::Event::Error(e.clone())),
        }))
        .await;
}
