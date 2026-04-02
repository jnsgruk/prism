use ps_proto::canonical::prism::v1::{
    AgentError, AgentFinalAnswer, ResumeStreamRequest, ResumeStreamResponse,
};
use tonic::{Request, Response, Status};
use tracing::error;
use uuid::Uuid;

use super::super::super::common::{db_err, require_auth};
use super::super::ReasoningServiceImpl;
use super::STREAM_TIMEOUT;
use super::event_mapping;

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
    if conv
        .query_status
        .parse::<ps_core::models::QueryStatus>()
        .is_ok_and(ps_core::models::QueryStatus::is_terminal)
    {
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
    let deadline = tokio::time::Instant::now() + STREAM_TIMEOUT;

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
                    let proto_event = event_mapping::map_db_event_to_resume_proto(&event);
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
            match conv.query_status.parse::<ps_core::models::QueryStatus>() {
                Ok(
                    ps_core::models::QueryStatus::Cancelled | ps_core::models::QueryStatus::Failed,
                ) => return,
                Ok(ps_core::models::QueryStatus::Completed) => {
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
