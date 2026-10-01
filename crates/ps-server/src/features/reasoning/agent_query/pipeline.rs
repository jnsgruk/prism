//! Prepare the pod, resolve the session and connect streaming within one budget.

use ps_proto::canonical::prism::v1::{AskQuestionResponse, ask_question_response};
use tonic::Status;
use tracing::info;
use uuid::Uuid;

use super::prompts::build_conversation_recap;
use super::{
    STARTUP_TIMEOUT, STREAM_TIMEOUT, event_loop, event_mapping, finalize_query, session, startup,
};

/// The core streaming pipeline: prepare pod → connect SSE → stream → finalize.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_query_stream(
    repos: &ps_core::repo::Repos,
    http_client: &reqwest::Client,
    restate_url: &str,
    cid_str: &str,
    trigger_request: &serde_json::Value,
    question: &str,
    system_hint: Option<&str>,
    model_name: &str,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, Status>>,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conversation_id: Uuid = cid_str.parse()?;
    let stream_start = tokio::time::Instant::now();

    let startup_deadline = stream_start + STARTUP_TIMEOUT;
    let startup_fut = async {
        // Phase 1: Call Restate prepare_query synchronously.
        let prepare_start = tokio::time::Instant::now();
        let (pod_ip, pod_name, pod_uid) = prepare_and_poll(
            repos,
            http_client,
            restate_url,
            cid_str,
            trigger_request,
            conversation_id,
            tx,
        )
        .await?;

        info!(conversation_id = %cid_str, elapsed_ms = prepare_start.elapsed().as_millis(), "Agent pod preparation complete");

        // Store pod details so the frontend can display them.
        repos
            .reasoning
            .update_container_status(
                conversation_id,
                Some(&pod_name),
                "active",
                None,
                Some(&pod_ip),
            )
            .await?;

        // Phase 2: Connect to OpenCode and stream events.
        let conv = repos
            .reasoning
            .get_conversation(conversation_id)
            .await?
            .ok_or("conversation not found")?;

        let client = ps_agent::opencode_sdk::ClientBuilder::new()
            .base_url(format!("http://{pod_ip}:{}", ps_agent::OPENCODE_PORT))
            .directory("/home/agent")
            .timeout_secs(120)
            .build()?;

        startup_progress(
            repos,
            conversation_id,
            tx,
            "Waiting for agent application...",
        )
        .await?;
        let startup = startup::SessionStartup::new(
            format!("http://{pod_ip}:{}", ps_agent::OPENCODE_PORT),
            startup_deadline,
        )?;
        startup.ready().await?;
        startup_progress(repos, conversation_id, tx, "Resolving agent session...").await?;
        let session_start = tokio::time::Instant::now();
        let session_result =
            session::resolve_or_create_session(repos, &startup, &pod_uid, conversation_id, &conv)
                .await?;
        info!(conversation_id = %cid_str, elapsed_ms = session_start.elapsed().as_millis(), "Agent session resolution complete");
        let opencode_session_id = session_result.session_id;

        // When a new session was created for an existing conversation, the prior
        // turns are lost. Build a recap from the DB and prepend it to the system
        // hint so the agent can resolve references like "they" or "their".
        let merged_hint = if session_result.is_new {
            let messages = repos.reasoning.list_messages(conversation_id).await?;
            match (build_conversation_recap(&messages), system_hint) {
                (Some(recap), Some(hint)) => Some(format!("{recap}\n\n{hint}")),
                (Some(recap), None) => Some(recap),
                (None, Some(hint)) => Some(hint.to_owned()),
                (None, None) => None,
            }
        } else {
            system_hint.map(str::to_owned)
        };

        startup_progress(
            repos,
            conversation_id,
            tx,
            "Connecting agent event stream...",
        )
        .await?;
        let sse_start = tokio::time::Instant::now();
        info!("subscribing to OpenCode events");
        let subscription = session::subscribe_to_events(&client).await?;
        info!(conversation_id = %cid_str, elapsed_ms = sse_start.elapsed().as_millis(), startup_ms = stream_start.elapsed().as_millis(), "SSE subscription established");

        Ok::<_, startup::StartupError>((
            pod_ip,
            client,
            conv,
            opencode_session_id,
            merged_hint,
            subscription,
        ))
    };
    let (pod_ip, client, conv, opencode_session_id, merged_hint, mut subscription) = tokio::select! {
        result = tokio::time::timeout_at(startup_deadline, startup_fut) => {
            result.map_err(|_| "agent startup budget exhausted")??
        }
        _ = cancel_rx.changed() => {
            info!(conversation_id = %cid_str, "Agent startup cancelled");
            return Ok(());
        }
    };

    if *cancel_rx.borrow() {
        subscription.close();
        return Ok(());
    }
    tokio::select! {
        result = session::send_prompt_or_compact(
            http_client,
            &client,
            &opencode_session_id,
            &conv,
            &pod_ip,
            question,
            merged_hint.as_deref(),
        ) => result?,
        _ = cancel_rx.changed() => {
            subscription.close();
            return Ok(());
        }
    }

    // Use actual elapsed time for prepare phase, not the worst-case STARTUP_TIMEOUT.
    // This gives the SSE phase the full remaining budget from STREAM_TIMEOUT.
    let elapsed = stream_start.elapsed();
    let sse_timeout = STREAM_TIMEOUT
        .checked_sub(elapsed)
        .unwrap_or(std::time::Duration::from_mins(1));

    let loop_result = event_loop::run_event_loop(
        repos,
        &mut subscription,
        conversation_id,
        sse_timeout,
        tx,
        cancel_rx,
    )
    .await;

    // Phase 3: Finalize.
    finalize_query(
        repos,
        conversation_id,
        cid_str,
        model_name,
        question,
        &loop_result,
        tx,
    )
    .await
}

/// Call Restate `prepare_query` synchronously while polling for container
/// status events to forward to the client. Returns `(pod_ip, pod_name)`.
async fn prepare_and_poll(
    repos: &ps_core::repo::Repos,
    http_client: &reqwest::Client,
    restate_url: &str,
    cid_str: &str,
    trigger_request: &serde_json::Value,
    conversation_id: Uuid,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, Status>>,
) -> Result<(String, String, String), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{restate_url}/AgenticQueryHandler/{cid_str}/prepare_query");
    let body = serde_json::to_string(trigger_request)?;

    let prepare_fut = async {
        let resp = http_client
            .post(&url)
            .timeout(STARTUP_TIMEOUT)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await?;

        let status = resp.status();
        let resp_body = resp.text().await?;

        if !status.is_success() {
            return Err(format!("prepare_query failed (HTTP {status}): {resp_body}").into());
        }

        let response: serde_json::Value = serde_json::from_str(&resp_body)?;
        let pod_ip = response
            .get("pod_ip")
            .and_then(|v| v.as_str())
            .ok_or("prepare_query response missing pod_ip")?
            .to_string();
        let pod_name = response
            .get("pod_name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let pod_uid = response
            .get("pod_uid")
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .ok_or("prepare_query response missing pod_uid; update ps-workers before ps-server")?
            .to_string();
        Ok::<(String, String, String), Box<dyn std::error::Error + Send + Sync>>((
            pod_ip, pod_name, pod_uid,
        ))
    };

    let poll_fut = async {
        let mut cursor: i64 = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            if let Ok(events) = repos.reasoning.poll_events(conversation_id, cursor).await {
                for event in events {
                    cursor = event.id;
                    if let Some(response) = event_mapping::map_db_event_to_proto(&event)
                        && matches!(
                            response.event,
                            Some(ask_question_response::Event::ContainerStatus(_))
                        )
                    {
                        let _ = tx.send(Ok(response)).await;
                    }
                }
            }
        }
    };

    tokio::select! {
        result = prepare_fut => result,
        () = poll_fut => Err("event poll loop ended unexpectedly".into()),
    }
}

/// Persist startup progress so browser reconnection can replay it.
async fn startup_progress(
    repos: &ps_core::repo::Repos,
    conversation_id: Uuid,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, Status>>,
    message: &str,
) -> Result<(), startup::StartupError> {
    let payload = serde_json::json!({"status": "connecting", "message": message});
    let event = repos
        .reasoning
        .append_event(conversation_id, "container_status", &payload, None, None)
        .await?;
    if let Some(response) = event_mapping::map_db_event_to_proto(&event) {
        let _ = tx.send(Ok(response)).await;
    }
    Ok(())
}
