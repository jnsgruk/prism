use std::collections::{HashMap, HashSet};

use ps_proto::canonical::prism::v1::handlers_service_server::HandlersService;
use ps_proto::canonical::prism::v1::{
    ActiveRun, CancelHandlerRunRequest, CancelHandlerRunResponse, CancelPipelineRequest,
    CancelPipelineResponse, CancelRunRequest, CancelRunResponse, GetPipelineStatusRequest,
    GetPipelineStatusResponse, GetStatusRequest, GetStatusResponse, HandlerRun,
    ListHandlersRequest, ListHandlersResponse, ListPipelineRunsRequest, ListPipelineRunsResponse,
    ListRunsRequest, ListRunsResponse, SourceStatus, TriggerBackfillRequest,
    TriggerBackfillResponse, TriggerHandlerRequest, TriggerHandlerResponse, TriggerPipelineRequest,
    TriggerPipelineResponse, TriggerRunRequest, TriggerRunResponse, TriggerTeamSyncRequest,
    TriggerTeamSyncResponse,
};
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use super::{
    HANDLER_DEFS, HandlersServiceImpl, derive_state, known_handlers, validate_restate_identifier,
};
use crate::common::{db_err, platform_to_proto, require_auth, run_status_to_proto, to_timestamp};

pub(super) fn run_to_proto(r: ps_core::repo::activity::IngestionRunRow) -> HandlerRun {
    HandlerRun {
        id: r.id.to_string(),
        source_name: r.source_name,
        started_at: Some(to_timestamp(r.started_at)),
        completed_at: r.completed_at.map(to_timestamp),
        status: run_status_to_proto(&r.status),
        items_collected: r.items_collected.unwrap_or(0),
        error_message: r.error_message,
        rate_limit_waits_seconds: 0,
        handler_name: r.handler_name,
        handler_method: r.handler_method,
        pipeline_id: r.pipeline_id.map(|id| id.to_string()),
        scope_kind: String::new(),
        person_id: None,
        selected_source_ids: Vec::new(),
        since_date: None,
        requested_by: None,
        progress_json: r
            .progress
            .map(|progress| progress.to_string())
            .unwrap_or_default(),
    }
}

#[tonic::async_trait]
impl HandlersService for HandlersServiceImpl {
    async fn get_status(
        &self,
        request: Request<GetStatusRequest>,
    ) -> Result<Response<GetStatusResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let mut sources = self
            .repos
            .activity
            .get_source_statuses()
            .await
            .map_err(db_err)?;

        // Reconcile any runs the DB thinks are active but Restate has
        // already finished (e.g. invocation died, was externally cancelled).
        self.reconcile_stale_runs(&mut sources).await;

        let statuses = sources
            .into_iter()
            .map(|s| {
                let state = derive_state(
                    s.has_active_run,
                    s.last_successful_run,
                    s.last_error.as_deref(),
                );

                // When actively collecting, show live progress from the running run
                let (items_collected, last_run) = if s.has_active_run {
                    (
                        s.active_run_items.unwrap_or(0),
                        s.active_run_started_at.map(to_timestamp),
                    )
                } else {
                    (
                        s.items_collected_last_run.unwrap_or(0),
                        s.last_successful_run.map(to_timestamp),
                    )
                };

                let progress_json = if s.has_active_run {
                    s.active_run_progress
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                let (source_type, _platform_instance) =
                    platform_to_proto(&s.source_type.to_string());

                SourceStatus {
                    name: s.name,
                    source_type,
                    state: state.into(),
                    last_run,
                    next_run: None, // TODO: compute from schedule_cron
                    items_collected,
                    rate_limit_info: HashMap::new(),
                    progress_json,
                }
            })
            .collect();

        Ok(Response::new(GetStatusResponse {
            sources: statuses,
            person_backfill_capabilities: Some(
                ps_proto::canonical::prism::v1::PersonBackfillCapabilities {
                    enabled: true,
                    reason: String::new(),
                },
            ),
        }))
    }

    async fn list_runs(
        &self,
        request: Request<ListRunsRequest>,
    ) -> Result<Response<ListRunsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let filter_name = req.source_name.filter(|n| !n.is_empty());
        let filter_handler = req.handler_name.filter(|n| !n.is_empty());

        let rows = self
            .repos
            .activity
            .list_runs(
                filter_name.as_deref(),
                filter_handler.as_deref(),
                req.ingestion_only,
            )
            .await
            .map_err(db_err)?;

        let runs = rows.into_iter().map(run_to_proto).collect();

        Ok(Response::new(ListRunsResponse { runs }))
    }

    async fn trigger_run(
        &self,
        _request: Request<TriggerRunRequest>,
    ) -> Result<Response<TriggerRunResponse>, Status> {
        Err(Status::unimplemented(
            "use TriggerPipeline instead — individual source runs are no longer supported",
        ))
    }

    async fn trigger_backfill(
        &self,
        _request: Request<TriggerBackfillRequest>,
    ) -> Result<Response<TriggerBackfillResponse>, Status> {
        Err(Status::unimplemented(
            "use TriggerPipeline with since_date instead — individual backfills are no longer supported",
        ))
    }

    async fn cancel_run(
        &self,
        request: Request<CancelRunRequest>,
    ) -> Result<Response<CancelRunResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        if req.source_name.is_empty() {
            return Err(Status::invalid_argument("source_name is required"));
        }

        self.require_unowned_source(&req.source_name).await?;

        // Look up source to get source_type for Restate queries
        let source = self
            .repos
            .config
            .get_enabled_source_by_name(&req.source_name)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("source not found or disabled"))?;
        let restate_key = source.source_type.to_string();

        // Collect invocation IDs to cancel: start with the stored one, then
        // also query Restate for any active invocations on this virtual object
        // (covers cases where the stored ID is stale or missing).
        let mut seen = HashSet::new();
        let mut ids_to_cancel = Vec::new();

        if let Some(id) = self
            .repos
            .activity
            .get_current_invocation_id(&source.name)
            .await
            .map_err(db_err)?
        {
            seen.insert(id.clone());
            ids_to_cancel.push(id);
        }

        // Query Restate admin for active invocations on this virtual object
        // (uses source_type as the Restate key)
        if let Some(active_ids) = self.query_active_invocations(&restate_key).await {
            for id in active_ids {
                if seen.insert(id.clone()) {
                    ids_to_cancel.push(id);
                }
            }
        }

        // Cancel all discovered invocations in parallel
        let cancellation_results = futures::future::join_all(
            ids_to_cancel
                .iter()
                .map(|id| self.cancel_legacy_restate_invocation(&source.name, id)),
        )
        .await;

        for result in cancellation_results {
            result?;
        }

        // Mark active runs as cancelled in the database (keyed on display name)
        self.repos
            .activity
            .cancel_active_runs(&source.name)
            .await
            .map_err(db_err)?;

        info!(source = %source.name, cancelled = ?ids_to_cancel, "cancelled ingestion run");

        Ok(Response::new(CancelRunResponse {}))
    }

    async fn trigger_team_sync(
        &self,
        request: Request<TriggerTeamSyncRequest>,
    ) -> Result<Response<TriggerTeamSyncResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        if req.source_name.is_empty() {
            return Err(Status::invalid_argument("source_name is required"));
        }

        // Look up source by display name, then use source_type as the Restate key
        let source = self
            .repos
            .config
            .get_enabled_source_by_name(&req.source_name)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("source not found or disabled"))?;
        let restate_key = source.source_type.to_string();
        validate_restate_identifier(&restate_key)?;

        // Fire-and-forget send to Restate ingress
        let url = format!(
            "{}/GithubTeamSyncHandler/{restate_key}/sync_teams/send",
            self.restate_url,
        );

        let invocation_id = self.send_to_restate(&url, None).await?;

        info!(source = %source.name, %invocation_id, "triggered team sync via Restate");

        Ok(Response::new(TriggerTeamSyncResponse {}))
    }

    async fn list_handlers(
        &self,
        request: Request<ListHandlersRequest>,
    ) -> Result<Response<ListHandlersResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let active_runs = self
            .repos
            .activity
            .get_active_handler_runs()
            .await
            .map_err(db_err)?;

        // Reconcile: verify each "active" run is actually alive in Restate
        let mut verified_runs = Vec::new();
        for run in &active_runs {
            if run.pipeline_id.is_some() || run.source_name == "_system" {
                // System handlers don't have invocation IDs — trust the DB
                verified_runs.push(run);
                continue;
            }

            // Check if the source has a stored invocation ID
            let inv_id = self
                .repos
                .activity
                .get_current_invocation_id(&run.source_name)
                .await
                .ok()
                .flatten();

            match inv_id {
                Some(id) if self.is_invocation_alive(&id).await => {
                    verified_runs.push(run);
                }
                _ => {
                    // Stale — cancel in DB
                    warn!(
                        source = %run.source_name,
                        handler = %run.handler_name,
                        "reconciling stale handler run in ListHandlers",
                    );
                    let _ = self
                        .repos
                        .activity
                        .cancel_active_runs_with_reason(
                            &run.source_name,
                            "Cancelled — invocation no longer active in Restate",
                        )
                        .await;
                }
            }
        }

        let mut handlers = known_handlers();
        for handler in &mut handlers {
            if let Some(run) = verified_runs
                .iter()
                .find(|r| r.handler_name == handler.name)
            {
                handler.active_run = Some(ActiveRun {
                    run_id: run.id.to_string(),
                    method: run.handler_method.clone(),
                    key: if run.source_name == "_system" {
                        None
                    } else {
                        Some(run.source_name.clone())
                    },
                    started_at: Some(to_timestamp(run.started_at)),
                });
            }
        }

        Ok(Response::new(ListHandlersResponse { handlers }))
    }

    async fn trigger_handler(
        &self,
        request: Request<TriggerHandlerRequest>,
    ) -> Result<Response<TriggerHandlerResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        if req.handler_name.is_empty() {
            return Err(Status::invalid_argument("handler_name is required"));
        }
        if req.method.is_empty() {
            return Err(Status::invalid_argument("method is required"));
        }
        // Validate handler + method combination against single source of truth
        let handler_def = HANDLER_DEFS.iter().find(|(name, methods, _, _)| {
            *name == req.handler_name && methods.contains(&req.method.as_str())
        });
        let Some((_, _, _, requires_key)) = handler_def else {
            return Err(Status::invalid_argument(format!(
                "unknown handler/method: {}/{}",
                req.handler_name, req.method,
            )));
        };

        // Object handlers require a key (source name); services do not
        let (url, source_name_for_db) = if *requires_key {
            if req.key.is_empty() {
                return Err(Status::invalid_argument("key (source name) is required"));
            }
            // Look up source by display name, use source_type as Restate key
            let source = self
                .repos
                .config
                .get_enabled_source_by_name(&req.key)
                .await
                .map_err(db_err)?
                .ok_or_else(|| Status::not_found("source not found or disabled"))?;
            let restate_key = source.source_type.to_string();
            validate_restate_identifier(&restate_key)?;

            (
                format!(
                    "{}/{}/{restate_key}/{}/send",
                    self.restate_url, req.handler_name, req.method,
                ),
                Some(source.name),
            )
        } else {
            // Service handler: /{handler}/{method}/send
            // Guard against duplicate runs for service handlers
            let active = self
                .repos
                .activity
                .get_active_handler_runs()
                .await
                .map_err(db_err)?;
            if active.iter().any(|r| r.handler_name == req.handler_name) {
                return Err(Status::already_exists(format!(
                    "{} already has an active run",
                    req.handler_name
                )));
            }

            (
                format!(
                    "{}/{}/{}/send",
                    self.restate_url, req.handler_name, req.method,
                ),
                None,
            )
        };

        let body = req.payload.as_deref().and_then(|p| {
            if p.is_empty() {
                None
            } else {
                serde_json::from_str::<serde_json::Value>(p).ok()
            }
        });

        let invocation_id = self.send_to_restate(&url, body.as_ref()).await?;

        // Store invocation ID for cancellation support and stale-run reconciliation.
        // Ingestion handlers key on their source name; service handlers key on
        // their well-known source name (e.g. "_enrichment").
        let invocation_key = source_name_for_db.clone().or_else(|| {
            // Map service handlers to their well-known source names
            match req.handler_name.as_str() {
                "EnrichmentHandler" => Some("_enrichment".into()),
                "EmbeddingHandler" => Some("_embedding".into()),
                "ModelCatalogueHandler" => Some("_model_catalogue".into()),
                _ => None,
            }
        });
        if let Some(ref key) = invocation_key {
            let _ = self
                .repos
                .activity
                .set_current_invocation_id(key, &invocation_id)
                .await;
        }

        info!(
            handler = %req.handler_name,
            method = %req.method,
            key = %req.key,
            %invocation_id,
            "triggered handler via Restate",
        );

        Ok(Response::new(TriggerHandlerResponse { invocation_id }))
    }

    async fn cancel_handler_run(
        &self,
        request: Request<CancelHandlerRunRequest>,
    ) -> Result<Response<CancelHandlerRunResponse>, Status> {
        self.handle_cancel_handler_run(request).await
    }

    async fn get_pipeline_status(
        &self,
        request: Request<GetPipelineStatusRequest>,
    ) -> Result<Response<GetPipelineStatusResponse>, Status> {
        self.handle_get_pipeline_status(request).await
    }

    async fn trigger_pipeline(
        &self,
        request: Request<TriggerPipelineRequest>,
    ) -> Result<Response<TriggerPipelineResponse>, Status> {
        self.handle_trigger_pipeline(request).await
    }

    async fn cancel_pipeline(
        &self,
        request: Request<CancelPipelineRequest>,
    ) -> Result<Response<CancelPipelineResponse>, Status> {
        self.handle_cancel_pipeline(request).await
    }

    async fn list_pipeline_runs(
        &self,
        request: Request<ListPipelineRunsRequest>,
    ) -> Result<Response<ListPipelineRunsResponse>, Status> {
        self.handle_list_pipeline_runs(request).await
    }
}
