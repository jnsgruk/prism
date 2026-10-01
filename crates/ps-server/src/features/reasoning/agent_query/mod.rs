mod event_loop;
mod event_mapping;
mod pipeline;
mod prompts;
mod resume;
mod session;
mod startup;
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
use prompts::{build_system_hint, mentions_to_json};

/// Maximum time the gRPC stream stays open (client-facing).
/// 10 minutes — agents often compile code, install packages, or run
/// multi-step pipelines that need more than a few minutes.
const STREAM_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(10);

/// Overall startup budget: Restate preparation, application health, session, SSE.
/// Must be < `STREAM_TIMEOUT` to leave budget for SSE streaming.
const STARTUP_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(2);

pub type AskQuestionStream =
    tokio_stream::wrappers::ReceiverStream<Result<AskQuestionResponse, Status>>;

pub use resume::resume_stream;

pub async fn ask_question(
    svc: &ReasoningServiceImpl,
    request: Request<AskQuestionRequest>,
) -> Result<Response<AskQuestionStream>, Status> {
    let ctx = require_auth(&request)?;
    let req = request.into_inner();

    // Validate question — allow empty text when files or mentions are attached.
    if req.question.trim().is_empty() && req.attached_files.is_empty() && req.mentions.is_empty() {
        return Err(Status::invalid_argument("question must not be empty"));
    }
    if req.question.len() > 4000 {
        return Err(Status::invalid_argument(
            "question must be at most 4000 characters",
        ));
    }

    let (model_name, provider_keys, default_image_model) = resolve_model_config(svc, &req).await;
    let mentions_json = mentions_to_json(&req.mentions);
    let system_hint = build_system_hint(&req.attached_files, &req.mentions);
    let conversation_id =
        setup_conversation(svc, ctx.user_id, &req, &model_name, &mentions_json).await?;

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

    // Create a cancellation channel for this query.
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let active_queries = svc.active_queries.clone();
    {
        let mut map = active_queries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.insert(conversation_id, cancel_tx.clone());
    }

    // Spawn the streaming task.
    let aq = active_queries.clone();
    tokio::spawn(async move {
        if let Err(e) = pipeline::run_query_stream(
            &repos,
            &http_client,
            &restate_url,
            &cid_str,
            &trigger_request,
            &question,
            system_hint.as_deref(),
            &model_for_usage,
            &tx,
            cancel_rx,
        )
        .await
        {
            handle_stream_failure(&repos, conversation_id, &cid_str, &*e, &tx).await;
        }
        // Deregister from active queries.
        let mut map = aq.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if map
            .get(&conversation_id)
            .is_some_and(|current| current.same_channel(&cancel_tx))
        {
            map.remove(&conversation_id);
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
    mentions_json: &serde_json::Value,
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
        // When the client provides a conversation_id that doesn't exist yet
        // (e.g. for file uploads before asking), adopt that ID so uploaded
        // files in the workspace directory match the conversation.
        let requested_id = req
            .conversation_id
            .as_ref()
            .and_then(|id| id.parse::<Uuid>().ok());
        svc.repos
            .reasoning
            .create_conversation(&CreateConversationParams {
                id: requested_id,
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
            attached_files: &req.attached_files,
            mentions: mentions_json,
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
            attached_files: &[],
            mentions: &serde_json::json!([]),
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
