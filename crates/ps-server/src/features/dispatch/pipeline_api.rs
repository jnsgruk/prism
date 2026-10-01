use std::collections::HashMap;

use ps_proto::canonical::prism::v1::{
    CancelPipelineRequest, CancelPipelineResponse, GetPipelineStatusRequest,
    GetPipelineStatusResponse, HandlerRun, ListPipelineRunsRequest, ListPipelineRunsResponse,
    PipelineInfo, PipelineRunSummary, TriggerPipelineRequest, TriggerPipelineResponse,
};
use tonic::{Request, Response, Status};
use tracing::warn;

use super::{HandlersServiceImpl, grpc::run_to_proto};
use crate::common::{db_err, platform_to_proto, require_admin, require_auth, to_timestamp};

fn pipeline_to_proto(p: &ps_core::models::Pipeline) -> PipelineInfo {
    let snapshot =
        serde_json::from_value::<ps_core::ingestion::PipelineRequest>(p.request_snapshot.clone())
            .ok();
    PipelineInfo {
        id: p.id.to_string(),
        status: p.status.clone(),
        current_stage: p.current_stage.clone().unwrap_or_default(),
        started_at: Some(to_timestamp(p.started_at)),
        completed_at: p.completed_at.map(to_timestamp),
        stages_json: p.stages.to_string(),
        error: p.error.clone(),
        scope_kind: snapshot
            .as_ref()
            .map_or("all", |request| request.scope.kind())
            .into(),
        person_id: snapshot
            .as_ref()
            .and_then(|request| request.scope.person_id())
            .map(|id| id.to_string()),
        selected_source_ids: snapshot
            .as_ref()
            .map(|request| {
                request
                    .sources
                    .iter()
                    .map(|source| source.source_id.to_string())
                    .collect()
            })
            .unwrap_or_default(),
        since_date: snapshot
            .as_ref()
            .and_then(|request| request.since_date.clone()),
        requested_by: p.requested_by.map(|id| id.to_string()),
        selected_sources: snapshot
            .as_ref()
            .map(|request| {
                request
                    .sources
                    .iter()
                    .map(|source| {
                        let (platform, instance) = platform_to_proto(&source.platform.to_string());
                        ps_proto::canonical::prism::v1::PipelineSourceSnapshot {
                            source_id: source.source_id.to_string(),
                            source_name: source.source_name.clone(),
                            platform,
                            instance,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

impl HandlersServiceImpl {
    pub(crate) async fn handle_get_pipeline_status(
        &self,
        request: Request<GetPipelineStatusRequest>,
    ) -> Result<Response<GetPipelineStatusResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let request = request.into_inner();
        let person_id = request
            .person_id
            .as_deref()
            .map(str::parse::<uuid::Uuid>)
            .transpose()
            .map_err(|_| Status::invalid_argument("invalid person_id"))?;
        if let Some(id) = request.pipeline_id {
            let id = id
                .parse()
                .map_err(|_| Status::invalid_argument("invalid pipeline_id"))?;
            let pipeline = self
                .repos
                .activity
                .get_pipeline(id)
                .await
                .map_err(db_err)?
                .ok_or_else(|| Status::not_found("pipeline not found"))?;
            if person_id.is_some_and(|person_id| {
                pipeline_to_proto(&pipeline).person_id.as_deref()
                    != Some(person_id.to_string().as_str())
            }) {
                return Err(Status::not_found("pipeline not found for person"));
            }
            return Ok(Response::new(GetPipelineStatusResponse {
                current: Some(pipeline_to_proto(&pipeline)),
                recent: Vec::new(),
            }));
        }
        let pipelines = if let Some(person_id) = person_id {
            self.repos
                .activity
                .list_person_pipelines(person_id, 11)
                .await
                .map_err(db_err)?
        } else {
            self.repos
                .activity
                .list_recent_pipelines(11)
                .await
                .map_err(db_err)?
        };
        let active = pipelines.iter().find(|pipeline| {
            matches!(
                pipeline.status.as_str(),
                "pending" | "running" | "cancelling"
            )
        });
        let current = active.map(pipeline_to_proto);
        let recent = pipelines
            .iter()
            .filter(|pipeline| active.is_none_or(|active| active.id != pipeline.id))
            .take(10)
            .map(pipeline_to_proto)
            .collect();

        Ok(Response::new(GetPipelineStatusResponse { current, recent }))
    }

    pub(crate) async fn handle_trigger_pipeline(
        &self,
        request: Request<TriggerPipelineRequest>,
    ) -> Result<Response<TriggerPipelineResponse>, Status> {
        let caller = require_admin(&request)?;
        let request = request.into_inner();
        let pipeline_id = request
            .submission_id
            .as_deref()
            .map(str::parse::<uuid::Uuid>)
            .transpose()
            .map_err(|_| Status::invalid_argument("invalid submission_id"))?
            .unwrap_or_else(uuid::Uuid::now_v7);
        if pipeline_id.is_nil() {
            return Err(Status::invalid_argument("invalid submission_id"));
        }
        if let Some(existing) = self
            .repos
            .activity
            .get_pipeline(pipeline_id)
            .await
            .map_err(db_err)?
        {
            if !super::admission::matches_submission(&existing, &request, caller.user_id) {
                return Err(Status::already_exists(
                    "submission_id belongs to a different request",
                ));
            }
            return Ok(Response::new(TriggerPipelineResponse {
                pipeline_id: pipeline_id.to_string(),
            }));
        }
        let resolved = self.resolve_pipeline_request(&request).await?;
        let snapshot = serde_json::to_value(resolved).map_err(db_err)?;
        if let Err(error) = self
            .repos
            .activity
            .reserve_pipeline(pipeline_id, &snapshot, caller.user_id, &caller.username)
            .await
        {
            if let Some(existing) = self
                .repos
                .activity
                .get_pipeline(pipeline_id)
                .await
                .map_err(db_err)?
            {
                if super::admission::matches_submission(&existing, &request, caller.user_id) {
                    return Ok(Response::new(TriggerPipelineResponse {
                        pipeline_id: pipeline_id.to_string(),
                    }));
                }
                return Err(Status::already_exists(
                    "submission_id belongs to a different request",
                ));
            }
            return Err(match error {
                ps_core::Error::Conflict(message) => Status::already_exists(message),
                error => db_err(error),
            });
        }
        let service = self.clone();
        tokio::spawn(async move {
            if let Err(error) = service.recover_pipeline_dispatch().await {
                warn!(%pipeline_id, %error, "initial pipeline dispatch failed; durable recovery will retry");
            }
        });
        Ok(Response::new(TriggerPipelineResponse {
            pipeline_id: pipeline_id.to_string(),
        }))
    }

    pub(crate) async fn handle_cancel_pipeline(
        &self,
        request: Request<CancelPipelineRequest>,
    ) -> Result<Response<CancelPipelineResponse>, Status> {
        let _caller = require_admin(&request)?;
        let id = request
            .into_inner()
            .pipeline_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid pipeline_id"))?;
        let pipeline = self
            .repos
            .activity
            .get_pipeline(id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("pipeline not found"))?;
        if !matches!(
            pipeline.status.as_str(),
            "pending" | "running" | "cancelling"
        ) {
            return Err(Status::failed_precondition("pipeline is not active"));
        }
        self.repos
            .activity
            .request_pipeline_cancel(id)
            .await
            .map_err(db_err)?;
        let service = self.clone();
        tokio::spawn(async move {
            if let Err(error) = service.recover_pipeline_dispatch().await {
                warn!(pipeline_id = %id, %error, "pipeline cancellation recovery failed");
            }
        });
        Ok(Response::new(CancelPipelineResponse {}))
    }

    pub(crate) async fn handle_list_pipeline_runs(
        &self,
        request: Request<ListPipelineRunsRequest>,
    ) -> Result<Response<ListPipelineRunsResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let request = request.into_inner();
        let pipelines = if let Some(person_id) = request.person_id {
            let person_id = person_id
                .parse()
                .map_err(|_| Status::invalid_argument("invalid person_id"))?;
            self.repos
                .activity
                .list_person_pipelines(person_id, 20)
                .await
                .map_err(db_err)?
        } else {
            self.repos
                .activity
                .list_recent_pipelines(20)
                .await
                .map_err(db_err)?
        };

        let pipeline_ids: Vec<uuid::Uuid> = pipelines.iter().map(|p| p.id).collect();

        let all_runs = self
            .repos
            .activity
            .list_runs_for_pipelines(&pipeline_ids)
            .await
            .map_err(db_err)?;

        // Group runs by pipeline_id
        let mut runs_by_pipeline: HashMap<uuid::Uuid, Vec<HandlerRun>> = HashMap::new();
        for run in all_runs {
            let pid = run.pipeline_id;
            let proto = run_to_proto(run);
            if let Some(pid) = pid {
                runs_by_pipeline.entry(pid).or_default().push(proto);
            }
        }

        let summaries = pipelines
            .iter()
            .map(|p| {
                let info = pipeline_to_proto(p);
                let mut runs = runs_by_pipeline.remove(&p.id).unwrap_or_default();
                for run in &mut runs {
                    run.scope_kind.clone_from(&info.scope_kind);
                    run.person_id.clone_from(&info.person_id);
                    run.selected_source_ids
                        .clone_from(&info.selected_source_ids);
                    run.since_date.clone_from(&info.since_date);
                    run.requested_by.clone_from(&info.requested_by);
                }
                PipelineRunSummary {
                    pipeline: Some(info),
                    runs,
                }
            })
            .collect();

        Ok(Response::new(ListPipelineRunsResponse {
            pipelines: summaries,
        }))
    }
}
