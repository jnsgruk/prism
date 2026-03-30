use ps_core::repo::Repos;
use ps_core::repo::reasoning::CreateArtifactParams;
use tracing::warn;
use uuid::Uuid;

/// Inspect an SSE event for a completed `prism_upload_artifact` tool call
/// and, if found, register the artifact in the database and emit an
/// `artifact_uploaded` event.
pub async fn handle_artifact_upload(
    repos: &Repos,
    conversation_id: Uuid,
    event: &ps_agent::opencode_sdk::types::event::Event,
) {
    let ps_agent::opencode_sdk::types::event::Event::MessagePartUpdated { properties } = event
    else {
        return;
    };

    let Some(ps_agent::opencode_sdk::types::message::Part::Tool {
        tool,
        state: Some(ps_agent::opencode_sdk::types::message::ToolState::Completed(completed)),
        ..
    }) = properties.part.as_ref()
    else {
        return;
    };

    if tool != "prism_upload_artifact" {
        return;
    }

    let Ok(result) = serde_json::from_str::<serde_json::Value>(&completed.output) else {
        return;
    };

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
