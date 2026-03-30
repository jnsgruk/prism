use ps_core::repo::Repos;
use tracing::info;
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

    // Subscribe to events before sending the question.
    info!("subscribing to OpenCode events");
    let mut subscription = client.subscribe().await?;
    info!("SSE subscription established");

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

    // Stream events until idle or timeout (5 minutes).
    let loop_result = event_loop::run_event_loop(
        repos,
        &mut subscription,
        conversation_id,
        std::time::Duration::from_secs(300),
    )
    .await;

    // Derive the reasoning trace from all conversation events.
    let all_events = repos
        .reasoning
        .get_all_events(conversation_id)
        .await
        .unwrap_or_default();
    let trace_steps = derive_trace_from_events(&all_events);

    Ok(QueryResult {
        answer_text: loop_result.answer_text,
        tool_calls: loop_result.tool_calls,
        trace_steps,
        input_tokens: loop_result.total_input.cast_signed(),
        output_tokens: loop_result.total_output.cast_signed(),
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
