use ps_proto::canonical::prism::v1::{
    DeleteEnrichmentsByTypeRequest, DeleteEnrichmentsByTypeResponse, EnrichmentTypeCount,
    GetEnrichmentPipelineStatusRequest, GetEnrichmentPipelineStatusResponse,
    GetEnrichmentsByContributionsRequest, GetEnrichmentsByContributionsResponse,
    GetEnrichmentsRequest, GetEnrichmentsResponse,
};
use tonic::{Request, Response, Status};
use tracing::info;
use uuid::Uuid;

use super::super::common::{
    db_err, enrichment_type_to_proto, proto_to_enrichment_type_str, require_auth, to_timestamp,
};
use super::ReasoningServiceImpl;
use super::convert::enrichment_to_proto;

pub async fn get_enrichments(
    svc: &ReasoningServiceImpl,
    request: Request<GetEnrichmentsRequest>,
) -> Result<Response<GetEnrichmentsResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let contribution_id: Uuid = req
        .contribution_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid contribution_id"))?;

    let enrichments = svc
        .repos
        .reasoning
        .get_enrichments_for_contribution(contribution_id)
        .await
        .map_err(db_err)?;

    Ok(Response::new(GetEnrichmentsResponse {
        enrichments: enrichments.into_iter().map(enrichment_to_proto).collect(),
    }))
}

pub async fn get_enrichments_by_contributions(
    svc: &ReasoningServiceImpl,
    request: Request<GetEnrichmentsByContributionsRequest>,
) -> Result<Response<GetEnrichmentsByContributionsResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let ids: Vec<Uuid> = req
        .contribution_ids
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();

    if ids.is_empty() {
        return Ok(Response::new(GetEnrichmentsByContributionsResponse {
            enrichments: vec![],
        }));
    }

    let enrichments = svc
        .repos
        .reasoning
        .get_enrichments_for_contributions(&ids)
        .await
        .map_err(db_err)?;

    Ok(Response::new(GetEnrichmentsByContributionsResponse {
        enrichments: enrichments.into_iter().map(enrichment_to_proto).collect(),
    }))
}

pub async fn get_enrichment_pipeline_status(
    svc: &ReasoningServiceImpl,
    request: Request<GetEnrichmentPipelineStatusRequest>,
) -> Result<Response<GetEnrichmentPipelineStatusResponse>, Status> {
    let _ctx = require_auth(&request)?;

    let status = svc
        .repos
        .reasoning
        .get_enrichment_status()
        .await
        .map_err(db_err)?;

    Ok(Response::new(GetEnrichmentPipelineStatusResponse {
        pending_count: status.pending_count,
        total_enrichments: status.total_enrichments,
        last_enrichment_at: status.last_enrichment_at.map(to_timestamp),
        by_type: status
            .by_type
            .into_iter()
            .map(|t| EnrichmentTypeCount {
                enrichment_type: enrichment_type_to_proto(&t.enrichment_type),
                count: t.total_count,
            })
            .collect(),
    }))
}

pub async fn delete_enrichments_by_type(
    svc: &ReasoningServiceImpl,
    request: Request<DeleteEnrichmentsByTypeRequest>,
) -> Result<Response<DeleteEnrichmentsByTypeResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let enrichment_type_str = proto_to_enrichment_type_str(req.enrichment_type)
        .ok_or_else(|| Status::invalid_argument("enrichment_type is required"))?;

    let deleted = svc
        .repos
        .reasoning
        .delete_enrichments_by_type(&enrichment_type_str)
        .await
        .map_err(db_err)?;

    info!(enrichment_type = %enrichment_type_str, deleted, "enrichments deleted for re-enrichment");

    Ok(Response::new(DeleteEnrichmentsByTypeResponse {
        #[allow(clippy::cast_possible_wrap)]
        deleted_count: deleted as i64,
    }))
}
