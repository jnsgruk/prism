use ps_core::repo::Repos;
use tracing::{info, warn};
use uuid::Uuid;

use super::event_loop;
use super::trace::derive_trace_from_events;

/// Result of the core query execution.
pub struct QueryResult {
    pub answer_text: String,
    pub tool_calls: i32,
    pub trace_steps: Vec<serde_json::Value>,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// Core agentic query logic: connect to `OpenCode`, stream events, write to DB.
///
/// Extracted as a testable function separate from Restate `ctx.run()` wrappers.
pub async fn run_agentic_query_core(
    repos: &Repos,
    http_client: &reqwest::Client,
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

    let opencode_session_id =
        resolve_or_create_session(repos, &client, conversation_id, &conv, question).await?;

    // Check if the question was already sent to this session (Restate replay
    // scenario). If so, skip sending to avoid confusing OpenCode.
    let already_sent = was_question_already_sent(&client, &opencode_session_id, question).await;

    // Subscribe to events before sending the question.
    info!("subscribing to OpenCode events");
    let mut subscription = client.subscribe().await?;
    info!("SSE subscription established");

    if already_sent {
        info!("question already sent to session (replay), skipping resend");
    } else {
        // Send the question or trigger compaction.
        send_prompt_or_compact(
            http_client,
            &client,
            &opencode_session_id,
            &conv,
            pod_ip,
            question,
        )
        .await?;
    }

    // Stream events until idle or timeout (5 minutes).
    // When replaying (question already sent), treat initial idle as terminal
    // since the session already finished processing.
    let loop_result = event_loop::run_event_loop(
        repos,
        &mut subscription,
        conversation_id,
        std::time::Duration::from_secs(300),
        already_sent,
    )
    .await;

    // Derive the reasoning trace from all conversation events (includes
    // events written by previous invocations before a Restate replay).
    let all_events = repos
        .reasoning
        .get_all_events(conversation_id)
        .await
        .unwrap_or_default();
    let trace_steps = derive_trace_from_events(&all_events);

    // On replay, the event loop returns empty because no new events were
    // streamed. Recover the answer from events written by the previous
    // invocation.
    let mut answer_text = loop_result.answer_text;
    let mut tool_calls = loop_result.tool_calls;
    if answer_text.is_empty() && !all_events.is_empty() {
        if let Some(last_answer) = all_events
            .iter()
            .rev()
            .filter(|e| e.event_type == "partial_answer")
            .find_map(|e| e.payload.get("text").and_then(|t| t.as_str()))
        {
            info!("recovered answer text from previous invocation events");
            answer_text = last_answer.to_string();
        }
        if tool_calls == 0 {
            tool_calls = i32::try_from(
                all_events
                    .iter()
                    .filter(|e| e.event_type == "tool_call_completed")
                    .count(),
            )
            .unwrap_or(i32::MAX);
        }
    }

    Ok(QueryResult {
        answer_text,
        tool_calls,
        trace_steps,
        input_tokens: loop_result.total_input.cast_signed(),
        output_tokens: loop_result.total_output.cast_signed(),
    })
}

/// Check whether the question was already sent to the `OpenCode` session.
///
/// On Restate replay, the handler re-executes the non-journaled SSE streaming
/// logic. Resending a question that was already processed can produce errors
/// (e.g., `NotFoundError` from `OpenCode`). This check prevents the duplicate
/// send by inspecting the session's message history.
async fn was_question_already_sent(
    client: &ps_agent::opencode_sdk::Client,
    session_id: &str,
    question: &str,
) -> bool {
    let messages = match client.messages().list(session_id).await {
        Ok(msgs) => msgs,
        Err(e) => {
            warn!(error = %e, "failed to list OpenCode messages, assuming not sent");
            return false;
        }
    };

    // Look for a user message that matches the question text.
    messages.iter().any(|m| {
        m.role() == "user"
            && m.parts.iter().any(|p| {
                matches!(p, ps_agent::opencode_sdk::types::message::Part::Text { text, .. } if text.trim() == question.trim())
            })
    })
}

/// Resolve an existing `OpenCode` session or create a new one.
async fn resolve_or_create_session(
    repos: &Repos,
    client: &ps_agent::opencode_sdk::Client,
    conversation_id: Uuid,
    conv: &ps_core::repo::reasoning::Conversation,
    question: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(ref oc_sid) = conv.opencode_session_id {
        info!(session_id = %oc_sid, "reusing existing OpenCode session");
        return Ok(oc_sid.clone());
    }

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
    Ok(session.id)
}

/// Send the user's question to `OpenCode`, or trigger session compaction for
/// the `/compact` command.
async fn send_prompt_or_compact(
    http_client: &reqwest::Client,
    client: &ps_agent::opencode_sdk::Client,
    opencode_session_id: &str,
    conv: &ps_core::repo::reasoning::Conversation,
    pod_ip: &str,
    question: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let is_compact = question.trim().eq_ignore_ascii_case("/compact");

    if is_compact {
        // Extract provider/model from the conversation's model_name (format: "provider/model_id").
        let model_name: &str = if conv.model_name.is_empty() {
            "google/gemini-2.5-flash"
        } else {
            &conv.model_name
        };
        let (provider_id, model_id) = model_name.split_once('/').unwrap_or(("google", model_name));
        info!(provider_id, model_id, "triggering session compaction");
        // Bypass opencode-sdk's SummarizeRequest which serializes as camelCase (providerId)
        // but OpenCode's API expects Go-style casing (providerID, modelID).
        let summarize_url = format!(
            "http://{pod_ip}:{port}/session/{sid}/summarize",
            port = ps_agent::OPENCODE_PORT,
            sid = opencode_session_id,
        );
        let resp = http_client
            .post(&summarize_url)
            .json(&serde_json::json!({
                "providerID": provider_id,
                "modelID": model_id,
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("compaction failed (HTTP {status}): {body}").into());
        }
        info!("compaction triggered");
    } else {
        info!("sending question to OpenCode");
        let prompt = ps_agent::opencode_sdk::types::message::PromptRequest::text(question)
            .with_agent("prism");
        client
            .messages()
            .prompt_async(opencode_session_id, &prompt)
            .await?;
        info!("question sent, streaming events");
    }

    Ok(())
}
