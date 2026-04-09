use ps_core::repo::Repos;
use ps_core::repo::reasoning::Conversation;
use tracing::{info, warn};
use uuid::Uuid;

/// Resolve an existing `OpenCode` session or create a new one.
///
/// If the conversation already has an `opencode_session_id` and the session
/// is still alive, reuse it. If it's dead (404), clear the stale reference
/// and create a fresh session. If no session exists, create one.
pub async fn resolve_or_create_session(
    repos: &Repos,
    client: &ps_agent::opencode_sdk::Client,
    conversation_id: Uuid,
    conv: &Conversation,
    question: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(ref oc_sid) = conv.opencode_session_id {
        // Verify the session is still alive.
        match client.messages().list(oc_sid).await {
            Ok(_) => {
                info!(session_id = %oc_sid, "reusing existing OpenCode session");
                return Ok(oc_sid.clone());
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("404") {
                    warn!("OpenCode session is dead (404), creating a new one");
                    // Clear stale session reference.
                    repos
                        .reasoning
                        .update_container_status(
                            conversation_id,
                            conv.container_pod_name.as_deref(),
                            "active",
                            None,
                            None,
                        )
                        .await
                        .map_err(|e| format!("failed to clear stale session: {e}"))?;
                } else {
                    warn!(error = %e, "failed to check session, creating new one");
                }
            }
        }
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
            None,
        )
        .await?;
    Ok(session.id)
}

/// Send the user's question to `OpenCode`, or trigger session compaction for
/// the `/compact` command.
pub async fn send_prompt_or_compact(
    http_client: &reqwest::Client,
    client: &ps_agent::opencode_sdk::Client,
    opencode_session_id: &str,
    conv: &Conversation,
    pod_ip: &str,
    question: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let is_compact = question.trim().eq_ignore_ascii_case("/compact");

    if is_compact {
        let model_name: &str = if conv.model_name.is_empty() {
            "google/gemini-2.5-flash"
        } else {
            &conv.model_name
        };
        let (provider_id, model_id) = model_name.split_once('/').unwrap_or(("google", model_name));
        info!(provider_id, model_id, "triggering session compaction");
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
