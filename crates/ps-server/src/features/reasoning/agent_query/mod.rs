mod event_loop;
mod event_mapping;
mod resume;
mod session;
mod step_registry;
mod trace;

use ps_proto::canonical::prism::v1::{
    AgentConversationCreated, AgentError, AgentFinalAnswer, AskQuestionRequest,
    AskQuestionResponse, ask_question_response,
};
use tonic::{Request, Response, Status};
use tracing::{error, info};
use uuid::Uuid;

use super::ReasoningServiceImpl;
use crate::common::{db_err, require_auth};

/// Maximum time the gRPC stream stays open (client-facing).
/// 10 minutes — agents often compile code, install packages, or run
/// multi-step pipelines that need more than a few minutes.
const STREAM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// Maximum time to wait for Restate `prepare_query` to return the pod IP.
/// Must be < `STREAM_TIMEOUT` to leave budget for SSE streaming.
const PREPARE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

pub type AskQuestionStream =
    tokio_stream::wrappers::ReceiverStream<Result<AskQuestionResponse, Status>>;

pub use resume::resume_stream;

pub async fn ask_question(
    svc: &ReasoningServiceImpl,
    request: Request<AskQuestionRequest>,
) -> Result<Response<AskQuestionStream>, Status> {
    let ctx = require_auth(&request)?;
    let req = request.into_inner();

    // Validate question.
    if req.question.trim().is_empty() {
        return Err(Status::invalid_argument("question must not be empty"));
    }
    if req.question.len() > 4000 {
        return Err(Status::invalid_argument(
            "question must be at most 4000 characters",
        ));
    }

    let (model_name, provider_keys, default_image_model) = resolve_model_config(svc, &req).await;
    let conversation_id = setup_conversation(svc, ctx.user_id, &req, &model_name).await?;

    // Per-query image_model takes priority over admin default.
    let effective_image_model = req.image_model.or(default_image_model);

    let trigger_request = serde_json::json!({
        "conversation_id": conversation_id.to_string(),
        "user_id": ctx.user_id.to_string(),
        "question": req.question,
        "model": model_name,
        "small_model": model_name,
        "provider_keys": provider_keys,
        "image_model": effective_image_model,
    });

    let restate_url = svc.restate_url.clone();
    let cid_str = conversation_id.to_string();
    let http_client = svc.http_client.clone();
    let repos = svc.repos.clone();
    let question = req.question.clone();
    let model_for_usage = model_name.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(64);

    // Send conversation_created as the first event.
    let conv_title = req.question.chars().take(100).collect::<String>();
    let _ = tx
        .send(Ok(AskQuestionResponse {
            event: Some(ask_question_response::Event::ConversationCreated(
                AgentConversationCreated {
                    conversation_id: conversation_id.to_string(),
                    title: conv_title,
                },
            )),
        }))
        .await;

    // Spawn the streaming task.
    tokio::spawn(async move {
        if let Err(e) = run_query_stream(
            &repos,
            &http_client,
            &restate_url,
            &cid_str,
            &trigger_request,
            &question,
            &model_for_usage,
            &tx,
        )
        .await
        {
            handle_stream_failure(&repos, conversation_id, &cid_str, &*e, &tx).await;
        }
    });

    Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
        rx,
    )))
}

/// Resolve the model name, provider API keys, and default image model from
/// the AI router config.
async fn resolve_model_config(
    svc: &ReasoningServiceImpl,
    req: &AskQuestionRequest,
) -> (String, Vec<(String, String)>, Option<String>) {
    let router = svc.router.read().await;
    let config = router.config();
    let model = match req.model_override.as_deref() {
        Some(ovr) if !ovr.is_empty() && ovr.contains('/') => ovr.to_owned(),
        _ => format!(
            "{}/{}",
            config.tasks.agentic.provider.as_str(),
            config.tasks.agentic.model
        ),
    };
    let keys = router.provider_env_vars();
    let img_model = config
        .image_generation
        .as_ref()
        .map(|tc| format!("{}/{}", tc.provider.as_str(), tc.model));
    (model, keys, img_model)
}

/// Create or resume a conversation, store the user message, and claim it
/// for query execution via atomic CAS.
async fn setup_conversation(
    svc: &ReasoningServiceImpl,
    user_id: Uuid,
    req: &AskQuestionRequest,
    model_name: &str,
) -> Result<Uuid, Status> {
    use ps_core::repo::reasoning::{CreateConversationParams, CreateMessageParams};

    let existing_conv = if let Some(ref id) = req.conversation_id {
        let conv_id = id
            .parse::<Uuid>()
            .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;
        svc.repos
            .reasoning
            .get_conversation(conv_id)
            .await
            .map_err(db_err)?
    } else {
        None
    };

    let conversation_id = if let Some(ref conv) = existing_conv {
        conv.id
    } else {
        svc.repos
            .reasoning
            .create_conversation(&CreateConversationParams {
                user_id,
                title: Some(&req.question.chars().take(100).collect::<String>()),
                model_name,
            })
            .await
            .map_err(db_err)?
            .id
    };

    svc.repos
        .reasoning
        .create_message(&CreateMessageParams {
            conversation_id,
            role: "user",
            content: &req.question,
            reasoning_trace: None,
            supporting_data: None,
            prompt_tokens: 0,
            completion_tokens: 0,
        })
        .await
        .map_err(db_err)?;

    let claimed = svc
        .repos
        .reasoning
        .try_claim_query(conversation_id)
        .await
        .map_err(db_err)?;
    if !claimed {
        return Err(Status::already_exists(
            "a query is already running for this conversation",
        ));
    }

    Ok(conversation_id)
}

/// Store a persistent error message, mark the query as failed, and send an
/// error event to the client stream.
async fn handle_stream_failure(
    repos: &ps_core::repo::Repos,
    conversation_id: Uuid,
    cid_str: &str,
    err: &(dyn std::error::Error + Send + Sync),
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, Status>>,
) {
    error!(conversation_id = %cid_str, error = %err, "query stream failed");
    let _ = repos
        .reasoning
        .create_message(&ps_core::repo::reasoning::CreateMessageParams {
            conversation_id,
            role: "error",
            content: &err.to_string(),
            reasoning_trace: None,
            supporting_data: None,
            prompt_tokens: 0,
            completion_tokens: 0,
        })
        .await;
    let _ = repos
        .reasoning
        .update_query_status(conversation_id, ps_core::models::QueryStatus::Failed)
        .await;
    let _ = tx
        .send(Ok(AskQuestionResponse {
            event: Some(ask_question_response::Event::Error(AgentError {
                message: err.to_string(),
                retryable: true,
            })),
        }))
        .await;
}

/// The core streaming pipeline: prepare pod → connect SSE → stream → finalize.
#[allow(clippy::too_many_arguments)]
async fn run_query_stream(
    repos: &ps_core::repo::Repos,
    http_client: &reqwest::Client,
    restate_url: &str,
    cid_str: &str,
    trigger_request: &serde_json::Value,
    question: &str,
    model_name: &str,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, Status>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conversation_id: Uuid = cid_str.parse()?;
    let stream_start = tokio::time::Instant::now();

    // Phase 1: Call Restate prepare_query synchronously.
    let pod_ip = prepare_and_poll(
        repos,
        http_client,
        restate_url,
        cid_str,
        trigger_request,
        conversation_id,
        tx,
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

    let opencode_session_id =
        session::resolve_or_create_session(repos, &client, conversation_id, &conv, question)
            .await?;

    info!("subscribing to OpenCode events");
    let mut subscription = client.subscribe().await?;
    info!("SSE subscription established");

    session::send_prompt_or_compact(
        http_client,
        &client,
        &opencode_session_id,
        &conv,
        &pod_ip,
        question,
    )
    .await?;

    // Use actual elapsed time for prepare phase, not the worst-case PREPARE_TIMEOUT.
    // This gives the SSE phase the full remaining budget from STREAM_TIMEOUT.
    let elapsed = stream_start.elapsed();
    let sse_timeout = STREAM_TIMEOUT
        .checked_sub(elapsed)
        .unwrap_or(std::time::Duration::from_secs(60));

    let loop_result =
        event_loop::run_event_loop(repos, &mut subscription, conversation_id, sse_timeout, tx)
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
async fn finalize_query(
    repos: &ps_core::repo::Repos,
    conversation_id: Uuid,
    cid_str: &str,
    model_name: &str,
    question: &str,
    loop_result: &event_loop::EventLoopResult,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, Status>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let answer = validate_answer(
        &loop_result.answer_text,
        question,
        loop_result.tool_calls,
        loop_result.timed_out,
    );
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

/// Call Restate `prepare_query` synchronously while polling for container
/// status events to forward to the client.
async fn prepare_and_poll(
    repos: &ps_core::repo::Repos,
    http_client: &reqwest::Client,
    restate_url: &str,
    cid_str: &str,
    trigger_request: &serde_json::Value,
    conversation_id: Uuid,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, Status>>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{restate_url}/AgenticQueryHandler/{cid_str}/prepare_query");
    let body = serde_json::to_string(trigger_request)?;

    let prepare_fut = async {
        let resp = http_client
            .post(&url)
            .timeout(PREPARE_TIMEOUT)
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

        Ok::<String, Box<dyn std::error::Error + Send + Sync>>(pod_ip)
    };

    let poll_fut = async {
        let mut cursor: i64 = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            if tx.is_closed() {
                break;
            }

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
