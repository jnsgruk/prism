use ps_proto::canonical::prism::v1::{
    DeleteConversationRequest, DeleteConversationResponse, GetConversationRequest,
    GetConversationResponse, ListConversationsRequest, ListConversationsResponse,
    RenameConversationRequest, RenameConversationResponse, SaveInsightFromConversationRequest,
    SaveInsightFromConversationResponse,
};
use tonic::{Request, Response, Status};
use tracing::{info, warn};
use uuid::Uuid;

use super::ReasoningServiceImpl;
use crate::common::{db_err, require_auth, to_timestamp};

pub async fn list_conversations(
    svc: &ReasoningServiceImpl,
    request: Request<ListConversationsRequest>,
) -> Result<Response<ListConversationsResponse>, Status> {
    let ctx = require_auth(&request)?;
    let req = request.into_inner();

    let page_size = i64::from(req.page_size.clamp(1, 100));
    let offset = i64::from((req.page - 1).max(0)) * page_size;

    let (convs, total) = svc
        .repos
        .reasoning
        .list_conversations(ctx.user_id, page_size, offset)
        .await
        .map_err(db_err)?;

    let conversations = convs
        .into_iter()
        .map(|c| ps_proto::canonical::prism::v1::ConversationSummary {
            id: c.id.to_string(),
            title: c.title,
            status: c.status,
            model_name: c.model_name,
            container_status: c.container_status,
            total_tool_calls: c.total_tool_calls,
            message_count: c.message_count.try_into().unwrap_or(0),
            created_at: Some(to_timestamp(c.created_at)),
            last_activity_at: Some(to_timestamp(c.last_activity_at)),
            query_status: c.query_status,
            total_prompt_tokens: c.total_prompt_tokens,
            total_completion_tokens: c.total_completion_tokens,
            container_pod_name: c.container_pod_name,
            container_pod_ip: c.container_pod_ip,
        })
        .collect();

    Ok(Response::new(ListConversationsResponse {
        conversations,
        total_count: total.try_into().unwrap_or(0),
    }))
}

pub async fn get_conversation(
    svc: &ReasoningServiceImpl,
    request: Request<GetConversationRequest>,
) -> Result<Response<GetConversationResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let conv_id: Uuid = req
        .conversation_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

    let conv = svc
        .repos
        .reasoning
        .get_conversation(conv_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("conversation not found"))?;

    let messages_list = svc
        .repos
        .reasoning
        .list_messages(conv_id)
        .await
        .map_err(db_err)?;

    let summary = ps_proto::canonical::prism::v1::ConversationSummary {
        id: conv.id.to_string(),
        title: conv.title,
        status: conv.status,
        model_name: conv.model_name,
        container_status: conv.container_status,
        total_tool_calls: conv.total_tool_calls,
        message_count: messages_list.len().try_into().unwrap_or(0),
        created_at: Some(to_timestamp(conv.created_at)),
        last_activity_at: Some(to_timestamp(conv.last_activity_at)),
        query_status: conv.query_status,
        total_prompt_tokens: conv.total_prompt_tokens,
        total_completion_tokens: conv.total_completion_tokens,
        container_pod_name: conv.container_pod_name,
        container_pod_ip: conv.container_pod_ip,
    };

    let messages = messages_list
        .into_iter()
        .map(|m| ps_proto::canonical::prism::v1::ConversationMessage {
            id: m.id.to_string(),
            role: m.role,
            content: m.content,
            reasoning_trace_json: m.reasoning_trace.map(|v| v.to_string()),
            supporting_data_json: m.supporting_data.map(|v| v.to_string()),
            prompt_tokens: m.prompt_tokens,
            completion_tokens: m.completion_tokens,
            created_at: Some(to_timestamp(m.created_at)),
            attached_files: m.attached_files,
        })
        .collect();

    Ok(Response::new(GetConversationResponse {
        conversation: Some(summary),
        messages,
    }))
}

pub async fn delete_conversation(
    svc: &ReasoningServiceImpl,
    request: Request<DeleteConversationRequest>,
) -> Result<Response<DeleteConversationResponse>, Status> {
    let ctx = require_auth(&request)?;
    let req = request.into_inner();

    let conv_id: Uuid = req
        .conversation_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

    let pod_name = svc
        .repos
        .reasoning
        .delete_conversation(conv_id, ctx.user_id)
        .await
        .map_err(db_err)?;

    // Fire-and-forget: cancel any in-flight query and clean up pod + workspace PVC.
    {
        let restate_url = svc.restate_url.clone();
        let client = svc.http_client.clone();
        tokio::spawn(async move {
            // Cancel running query if a pod exists.
            if pod_name.is_some() {
                let cancel_url =
                    format!("{restate_url}/AgenticQueryHandler/{conv_id}/cancel/send",);
                if let Err(e) = client
                    .post(&cancel_url)
                    .header("content-type", "application/json")
                    .body("{}")
                    .send()
                    .await
                {
                    warn!(error = %e, "failed to send cancel to Restate for deleted conversation");
                }
            }

            // Clean up pod and workspace PVC.
            let cleanup_url =
                format!("{restate_url}/AgenticQueryHandler/{conv_id}/cleanup_storage/send",);
            if let Err(e) = client
                .post(&cleanup_url)
                .header("content-type", "application/json")
                .body("{}")
                .send()
                .await
            {
                warn!(error = %e, "failed to send cleanup_storage to Restate for deleted conversation");
            }
        });
    }

    info!(conversation_id = %conv_id, "conversation deleted");
    Ok(Response::new(DeleteConversationResponse {}))
}

pub async fn rename_conversation(
    svc: &ReasoningServiceImpl,
    request: Request<RenameConversationRequest>,
) -> Result<Response<RenameConversationResponse>, Status> {
    let ctx = require_auth(&request)?;
    let req = request.into_inner();

    let conv_id: Uuid = req
        .conversation_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

    if req.title.is_empty() {
        return Err(Status::invalid_argument("title must not be empty"));
    }
    if req.title.len() > 200 {
        return Err(Status::invalid_argument(
            "title must be 200 characters or less",
        ));
    }

    svc.repos
        .reasoning
        .rename_conversation(conv_id, ctx.user_id, &req.title)
        .await
        .map_err(db_err)?;

    info!(conversation_id = %conv_id, title = %req.title, "conversation renamed");
    Ok(Response::new(RenameConversationResponse {}))
}

pub async fn save_insight_from_conversation(
    _svc: &ReasoningServiceImpl,
    _request: Request<SaveInsightFromConversationRequest>,
) -> Result<Response<SaveInsightFromConversationResponse>, Status> {
    // This requires the insights repo integration which is a deeper
    // integration — stub for now until the insight creation flow is defined.
    Err(Status::unimplemented(
        "SaveInsightFromConversation will be available in a future update",
    ))
}
