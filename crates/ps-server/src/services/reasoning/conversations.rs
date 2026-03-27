use ps_proto::canonical::prism::v1::{
    DeleteConversationRequest, DeleteConversationResponse, GetArtifactDownloadUrlRequest,
    GetArtifactDownloadUrlResponse, GetConversationRequest, GetConversationResponse,
    ListConversationsRequest, ListConversationsResponse, RenameConversationRequest,
    RenameConversationResponse, SaveInsightFromConversationRequest,
    SaveInsightFromConversationResponse,
};
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};
use uuid::Uuid;

use super::super::common::{db_err, require_auth, to_timestamp};
use super::ReasoningServiceImpl;

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
            total_estimated_cost_usd: c.total_estimated_cost_usd,
            message_count: c.message_count.try_into().unwrap_or(0),
            artifact_count: c.artifact_count.try_into().unwrap_or(0),
            created_at: Some(to_timestamp(c.created_at)),
            last_activity_at: Some(to_timestamp(c.last_activity_at)),
            query_status: c.query_status,
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

    let (messages_list, artifacts_list) = tokio::try_join!(
        async {
            svc.repos
                .reasoning
                .list_messages(conv_id)
                .await
                .map_err(db_err)
        },
        async {
            svc.repos
                .reasoning
                .list_artifacts(conv_id)
                .await
                .map_err(db_err)
        },
    )?;

    let summary = ps_proto::canonical::prism::v1::ConversationSummary {
        id: conv.id.to_string(),
        title: conv.title,
        status: conv.status,
        model_name: conv.model_name,
        container_status: conv.container_status,
        total_tool_calls: conv.total_tool_calls,
        total_estimated_cost_usd: conv.total_estimated_cost_usd,
        message_count: messages_list.len().try_into().unwrap_or(0),
        artifact_count: artifacts_list.len().try_into().unwrap_or(0),
        created_at: Some(to_timestamp(conv.created_at)),
        last_activity_at: Some(to_timestamp(conv.last_activity_at)),
        query_status: conv.query_status,
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
        })
        .collect();

    let artifacts = artifacts_list
        .into_iter()
        .map(|a| ps_proto::canonical::prism::v1::ConversationArtifact {
            id: a.id.to_string(),
            message_id: a.message_id.map(|id| id.to_string()),
            artifact_key: a.artifact_key,
            display_name: a.display_name,
            content_type: a.content_type,
            size_bytes: a.size_bytes,
            created_at: Some(to_timestamp(a.created_at)),
        })
        .collect();

    Ok(Response::new(GetConversationResponse {
        conversation: Some(summary),
        messages,
        artifacts,
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

pub async fn get_artifact_download_url(
    svc: &ReasoningServiceImpl,
    request: Request<GetArtifactDownloadUrlRequest>,
) -> Result<Response<GetArtifactDownloadUrlResponse>, Status> {
    use base64::Engine;
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let artifact_id: Uuid = req
        .artifact_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid artifact_id"))?;

    let artifact = svc
        .repos
        .reasoning
        .get_artifact(artifact_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("artifact not found"))?;

    let store = svc
        .artifact_store
        .as_ref()
        .ok_or_else(|| Status::unavailable("artifact storage not configured"))?;

    let key = ps_core::artifact_store::ArtifactKey::new(
        ps_core::artifact_store::ArtifactCategory::Conversations,
        &artifact.artifact_key,
    );

    // Proxy the download: read bytes from S3 and return as a data URL.
    // Presigned URLs don't work because the internal S3 hostname isn't
    // reachable from the browser.
    let data = store.get(&key).await.map_err(|e| {
        error!(error = %e, "Failed to read artifact from S3");
        Status::internal("failed to read artifact")
    })?;

    let content_type = artifact
        .content_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    let download_url = format!("data:{content_type};base64,{b64}");

    Ok(Response::new(GetArtifactDownloadUrlResponse {
        download_url,
        expires_in_seconds: 0,
    }))
}
