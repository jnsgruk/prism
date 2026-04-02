use ps_proto::canonical::prism::v1::{
    FindSimilarRequest, FindSimilarResponse, GetEmbeddingStatusRequest, GetEmbeddingStatusResponse,
    SearchByTextRequest, SearchByTextResponse,
};
use tonic::{Request, Response, Status};
use tracing::error;
use uuid::Uuid;

use super::ReasoningServiceImpl;
use super::convert::similar_to_proto;
use crate::common::{db_err, proto_to_platform_str, require_auth, to_timestamp};

pub async fn find_similar(
    svc: &ReasoningServiceImpl,
    request: Request<FindSimilarRequest>,
) -> Result<Response<FindSimilarResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let contribution_id: Uuid = req
        .contribution_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid contribution_id"))?;

    let limit = i64::from(req.limit.clamp(1, 50));
    let platform_str = proto_to_platform_str(req.platform, req.platform_instance.as_deref());

    let results = svc
        .repos
        .reasoning
        .find_similar_to_contribution(contribution_id, limit, platform_str.as_deref())
        .await
        .map_err(db_err)?;

    Ok(Response::new(FindSimilarResponse {
        items: results.into_iter().map(similar_to_proto).collect(),
    }))
}

pub async fn search_by_text(
    svc: &ReasoningServiceImpl,
    request: Request<SearchByTextRequest>,
) -> Result<Response<SearchByTextResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    if req.query_text.is_empty() {
        return Err(Status::invalid_argument("query_text is required"));
    }

    let limit = i64::from(req.limit.clamp(1, 50));
    let platform_str = proto_to_platform_str(req.platform, req.platform_instance.as_deref());

    // Embed the query text on-the-fly. Drop the router lock before the
    // API call so concurrent UpdateAiSettings writes aren't blocked.
    let model = {
        let router = svc.router.read().await;
        router
            .embedding_model()
            .map_err(|e| Status::unavailable(format!("embedding model not available: {e}")))?
    };

    #[allow(deprecated)]
    let embedding = model.embed_text(&req.query_text).await.map_err(|e| {
        error!(error = %e, "failed to embed query text");
        Status::internal("failed to generate query embedding")
    })?;

    let truncated = ps_reasoning::features::embeddings::truncate_embedding(&embedding);

    let results = svc
        .repos
        .reasoning
        .find_similar(&truncated, limit, platform_str.as_deref(), None)
        .await
        .map_err(db_err)?;

    Ok(Response::new(SearchByTextResponse {
        items: results.into_iter().map(similar_to_proto).collect(),
    }))
}

pub async fn get_embedding_status(
    svc: &ReasoningServiceImpl,
    request: Request<GetEmbeddingStatusRequest>,
) -> Result<Response<GetEmbeddingStatusResponse>, Status> {
    let _ctx = require_auth(&request)?;

    let status = svc
        .repos
        .reasoning
        .get_embedding_status()
        .await
        .map_err(db_err)?;

    #[allow(clippy::cast_precision_loss)]
    let coverage = if status.total_eligible > 0 {
        status.embedded_count as f64 / status.total_eligible as f64 * 100.0
    } else {
        0.0
    };

    Ok(Response::new(GetEmbeddingStatusResponse {
        queued_count: status.queued_count,
        embedded_count: status.embedded_count,
        total_eligible: status.total_eligible,
        last_embedded_at: status.last_embedded_at.map(to_timestamp),
        coverage_percent: coverage,
    }))
}
