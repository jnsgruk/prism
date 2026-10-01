//! Validate, persist, and emit completed answers through a single boundary.
pub use super::agent_query::event_loop::EventLoopResult;
use super::agent_query::trace;
use ps_proto::canonical::prism::v1::{
    AgentFinalAnswer, AskQuestionResponse, ask_question_response,
};
use tonic::Status;
use tracing::info;
use uuid::Uuid;

/// Check whether the agent produced a usable answer.
///
/// Returns the answer as-is when valid, or a user-facing explanation when
/// the answer is empty or is just the question echoed back (a known failure
/// mode when the model hits its step limit or the stream times out).
fn validate_answer(raw: &str, question: &str, tool_calls: i32, timed_out: bool) -> String {
    let trimmed = raw.trim();
    let is_empty = trimmed.is_empty();
    let is_echo =
        !trimmed.is_empty() && trimmed.len() <= question.len() + 20 && question.contains(trimmed);

    if !is_empty && !is_echo {
        return raw.to_string();
    }

    if tool_calls > 0 {
        let reason = if timed_out {
            "timed out"
        } else {
            "hit step limit"
        };
        tracing::warn!(
            tool_calls,
            answer_len = trimmed.len(),
            is_echo,
            timed_out,
            "agent produced no usable answer — likely {reason}"
        );
        if timed_out {
            format!(
                "I ran out of time before I could finish answering. \
                 I completed {tool_calls} tool calls gathering data but the \
                 request timed out before I could synthesize a response. \
                 Please try again — I'll pick up where I left off."
            )
        } else {
            format!(
                "I ran out of steps before I could finish answering. \
                 I completed {tool_calls} tool calls gathering data but wasn't \
                 able to synthesize a response. Please try again — I'll pick up \
                 where I left off, or you can ask a simpler question."
            )
        }
    } else {
        tracing::warn!("agent produced empty answer with no tool calls");
        "I wasn't able to produce an answer. Please try again.".to_string()
    }
}

/// Store assistant message, update totals, emit `final_answer`, and clean up.
///
/// Detects degenerate outcomes where the agent ran out of steps or failed to
/// produce a real answer, and surfaces a clear error to the user.
#[allow(clippy::too_many_arguments)]
pub async fn finalize_query(
    repos: &ps_core::repo::Repos,
    conversation_id: Uuid,
    cid_str: &str,
    model_name: &str,
    question: &str,
    loop_result: &EventLoopResult,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, Status>>,
    workspaces_path: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let answer = validate_answer(
        &loop_result.answer_text,
        question,
        loop_result.tool_calls,
        loop_result.timed_out,
    );
    let answer = super::answer_files::validate_file_links(
        answer,
        workspaces_path.map(std::path::Path::to_path_buf),
        conversation_id,
    )
    .await;
    let tool_calls = loop_result.tool_calls;
    let input_tok = loop_result.total_input as i32;
    let output_tok = loop_result.total_output as i32;

    let all_events = repos
        .reasoning
        .get_all_events(conversation_id)
        .await
        .unwrap_or_default();
    let trace_steps = trace::derive_trace_from_events(&all_events);

    let trace_json = serde_json::json!({
        "tool_call_count": tool_calls,
        "steps": trace_steps,
    });
    let _msg = repos
        .reasoning
        .create_message(&ps_core::repo::reasoning::CreateMessageParams {
            conversation_id,
            role: "assistant",
            content: &answer,
            reasoning_trace: Some(&trace_json),
            supporting_data: None,
            prompt_tokens: input_tok,
            completion_tokens: output_tok,
            attached_files: &[],
            mentions: &serde_json::json!([]),
        })
        .await?;

    repos
        .reasoning
        .update_conversation_totals(conversation_id, tool_calls, input_tok, output_tok)
        .await?;

    // Log usage for the admin dashboard. Extract provider from "provider/model" format.
    let provider = model_name.split('/').next().unwrap_or("google");
    let model_id = model_name.split('/').nth(1).unwrap_or(model_name);
    if let Err(e) = repos
        .reasoning
        .log_api_usage(provider, model_id, "agentic", input_tok, output_tok)
        .await
    {
        tracing::warn!(error = %e, "failed to log agentic usage");
    }

    let _ = tx
        .send(Ok(AskQuestionResponse {
            event: Some(ask_question_response::Event::FinalAnswer(
                AgentFinalAnswer {
                    answer: answer.clone(),
                    conversation_id: conversation_id.to_string(),
                    tool_call_count: tool_calls,
                    prompt_tokens: input_tok,
                    completion_tokens: output_tok,
                    ..Default::default()
                },
            )),
        }))
        .await;

    repos
        .reasoning
        .update_query_status(conversation_id, ps_core::models::QueryStatus::Completed)
        .await?;

    let _ = repos.reasoning.delete_events(conversation_id).await;

    info!(conversation_id = %cid_str, "query complete");
    Ok(())
}
