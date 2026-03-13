use std::collections::HashMap;

use ps_core::repo::Repos;
use ps_proto::prism::v1::ingestion_service_server::IngestionService;
use ps_proto::prism::v1::{
    GetStatusRequest, GetStatusResponse, IngestionRun, ListRunsRequest, ListRunsResponse,
    SourceState, SourceStatus, TriggerBackfillRequest, TriggerBackfillResponse, TriggerRunRequest,
    TriggerRunResponse,
};
use tonic::{Request, Response, Status};
use tracing::info;

use super::common::{db_err, require_auth, to_timestamp};

pub struct IngestionServiceImpl {
    repos: Repos,
    restate_url: String,
    http_client: reqwest::Client,
}

impl IngestionServiceImpl {
    pub fn new(repos: Repos, restate_url: String) -> Self {
        Self {
            repos,
            restate_url,
            http_client: reqwest::Client::new(),
        }
    }
}

#[tonic::async_trait]
impl IngestionService for IngestionServiceImpl {
    async fn get_status(
        &self,
        request: Request<GetStatusRequest>,
    ) -> Result<Response<GetStatusResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let sources = self
            .repos
            .activity
            .get_source_statuses()
            .await
            .map_err(db_err)?;

        let statuses = sources
            .into_iter()
            .map(|s| {
                let state = derive_state(
                    s.has_active_run,
                    s.last_successful_run,
                    s.last_error.as_deref(),
                );

                SourceStatus {
                    name: s.name,
                    source_type: s.source_type,
                    state: state.into(),
                    last_run: s.last_successful_run.map(to_timestamp),
                    next_run: None, // TODO: compute from schedule_cron
                    items_collected: s.items_collected_last_run.unwrap_or(0),
                    rate_limit_info: HashMap::new(),
                }
            })
            .collect();

        Ok(Response::new(GetStatusResponse { sources: statuses }))
    }

    async fn list_runs(
        &self,
        request: Request<ListRunsRequest>,
    ) -> Result<Response<ListRunsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let filter_name = req.source_name.filter(|n| !n.is_empty());

        let rows = self
            .repos
            .activity
            .list_runs(filter_name.as_deref())
            .await
            .map_err(db_err)?;

        let runs = rows
            .into_iter()
            .map(|r| IngestionRun {
                id: r.id.to_string(),
                source_name: r.source_name,
                started_at: Some(to_timestamp(r.started_at)),
                completed_at: r.completed_at.map(to_timestamp),
                status: r.status,
                items_collected: r.items_collected.unwrap_or(0),
                error_message: r.error_message,
                rate_limit_waits_seconds: 0,
            })
            .collect();

        Ok(Response::new(ListRunsResponse { runs }))
    }

    async fn trigger_run(
        &self,
        request: Request<TriggerRunRequest>,
    ) -> Result<Response<TriggerRunResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        if req.source_name.is_empty() {
            return Err(Status::invalid_argument("source_name is required"));
        }

        // Verify source exists and is enabled
        self.repos
            .config
            .get_enabled_source_by_name(&req.source_name)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("source not found or disabled"))?;

        // Fire-and-forget send to Restate ingress
        let url = format!(
            "{}/IngestionHandler/{}/run_ingestion/send",
            self.restate_url, req.source_name,
        );

        self.http_client
            .post(&url)
            .send()
            .await
            .map_err(|e| Status::unavailable(format!("failed to reach Restate: {e}")))?;

        info!(source = %req.source_name, "triggered ingestion run via Restate");

        Ok(Response::new(TriggerRunResponse {}))
    }

    async fn trigger_backfill(
        &self,
        request: Request<TriggerBackfillRequest>,
    ) -> Result<Response<TriggerBackfillResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        if req.source_name.is_empty() {
            return Err(Status::invalid_argument("source_name is required"));
        }
        if req.since_date.is_empty() {
            return Err(Status::invalid_argument("since_date is required"));
        }

        // Verify source exists and is enabled
        self.repos
            .config
            .get_enabled_source_by_name(&req.source_name)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("source not found or disabled"))?;

        // Fire-and-forget send to Restate ingress
        let url = format!(
            "{}/IngestionHandler/{}/backfill/send",
            self.restate_url, req.source_name,
        );
        let body = serde_json::json!(req.since_date);

        self.http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| Status::unavailable(format!("failed to reach Restate: {e}")))?;

        info!(source = %req.source_name, since = %req.since_date, "triggered backfill via Restate");

        Ok(Response::new(TriggerBackfillResponse {}))
    }
}

/// Derive the current source state from run data and watermarks.
fn derive_state(
    has_active_run: bool,
    last_successful_run: Option<time::OffsetDateTime>,
    last_error: Option<&str>,
) -> SourceState {
    if has_active_run {
        return SourceState::Collecting;
    }
    match (last_successful_run, last_error) {
        (_, Some(_)) => SourceState::Error,
        (Some(_), None) => SourceState::Idle,
        (None, None) => SourceState::Waiting,
    }
}
