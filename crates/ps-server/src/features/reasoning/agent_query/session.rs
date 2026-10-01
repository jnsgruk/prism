use ps_core::repo::Repos;
use ps_core::repo::reasoning::Conversation;
use tracing::{info, warn};
use uuid::Uuid;

/// Result of resolving or creating an `OpenCode` session.
pub struct SessionResult {
    /// The `OpenCode` session ID.
    pub session_id: String,
    /// Whether a new session was created (vs reusing an existing one).
    pub is_new: bool,
}

/// Wait for the SSE connection before sending a prompt, so the initial empty
/// parts arrive before their text deltas.
pub async fn subscribe_to_events(
    client: &ps_agent::opencode_sdk::Client,
) -> Result<ps_agent::opencode_sdk::sse::RawSseSubscription, Box<dyn std::error::Error + Send + Sync>>
{
    let connected = async {
        let mut subscription = client.subscribe_raw().await?;
        while let Some(frame) = subscription.recv().await {
            let event: ps_agent::opencode_sdk::types::event::Event =
                serde_json::from_str(&frame.data)?;
            if matches!(
                event,
                ps_agent::opencode_sdk::types::event::Event::ServerConnected { .. }
            ) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(subscription);
            }
        }
        Err("agent event stream closed before connecting".into())
    };
    tokio::time::timeout(std::time::Duration::from_secs(15), connected).await?
}

/// Resolve an existing `OpenCode` session or create a new one.
///
/// If the conversation already has an `opencode_session_id` and the session
/// is still alive, reuse it. Only a 404 allows replacement. A pending creation
/// is reconciled without another POST until its pod is replaced.
pub async fn resolve_or_create_session(
    repos: &Repos,
    startup: &super::startup::SessionStartup,
    pod_uid: &str,
    conversation_id: Uuid,
    conv: &Conversation,
) -> Result<SessionResult, Box<dyn std::error::Error + Send + Sync>> {
    let pending = format!("pending:{pod_uid}");
    let title = format!("Prism conversation {conversation_id}");
    let mut reconcile_only = false;
    if let Some(ref oc_sid) = conv.opencode_session_id {
        if oc_sid.starts_with("pending:") {
            // A new Kubernetes UID proves the old process cannot finish its POST.
            reconcile_only = oc_sid == &pending;
        } else if startup.get(oc_sid).await?.is_some() {
            info!(session_id = %oc_sid, "reusing existing OpenCode session");
            return Ok(SessionResult {
                session_id: oc_sid.clone(),
                is_new: false,
            });
        } else {
            warn!(session_id = %oc_sid, "OpenCode session is gone (404)");
        }
    }

    // Recover a committed POST whose response or DB update was lost.
    let session = if let Some(session) = startup.find(&title).await? {
        session
    } else {
        // Persist intent BEFORE POST. On failure, a later query only reconciles
        // on this pod; it cannot accidentally send a duplicate create request.
        repos
            .reasoning
            .update_container_status(
                conversation_id,
                conv.container_pod_name.as_deref(),
                "active",
                Some(&pending),
                None,
            )
            .await?;
        startup.create_or_reconcile(&title, !reconcile_only).await?
    };
    info!(session_id = %session.id, "OpenCode session resolved");
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
    Ok(SessionResult {
        session_id: session.id,
        is_new: true,
    })
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
    system_hint: Option<&str>,
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
        let mut prompt = ps_agent::opencode_sdk::types::message::PromptRequest::text(question)
            .with_agent("prism");
        if let Some(hint) = system_hint {
            prompt = prompt.with_system(hint);
        }
        client
            .messages()
            .prompt_async(opencode_session_id, &prompt)
            .await?;
        info!("question sent, streaming events");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn subscription_is_connected_and_keeps_subsequent_frames() {
        let server = MockServer::start().await;
        let body = concat!(
            "data: {\"type\":\"server.connected\",\"properties\":{}}\n\n",
            "data: {\"type\":\"message.part.delta\",\"properties\":{\"partID\":\"p1\",\"field\":\"text\",\"delta\":\"Hello\"}}\n\n",
        );
        Mock::given(method("GET"))
            .and(path("/event"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .expect(1)
            .mount(&server)
            .await;
        let client = ps_agent::opencode_sdk::ClientBuilder::new()
            .base_url(server.uri())
            .build()
            .unwrap();

        let mut subscription = subscribe_to_events(&client).await.unwrap();
        let frame = tokio::time::timeout(std::time::Duration::from_secs(1), subscription.recv())
            .await
            .unwrap()
            .unwrap();
        let event: serde_json::Value = serde_json::from_str(&frame.data).unwrap();
        assert_eq!(event.get("type").unwrap(), "message.part.delta");
        assert_eq!(event.pointer("/properties/delta").unwrap(), "Hello");
        subscription.close();
        server.verify().await;
    }
}
