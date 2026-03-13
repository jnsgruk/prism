use std::collections::HashMap;

use ps_core::repo::Repos;
use ps_core::repo::activity::SourceStatusRow;
use ps_proto::prism::v1::ingestion_service_server::IngestionService;
use ps_proto::prism::v1::{
    CancelRunRequest, CancelRunResponse, GetStatusRequest, GetStatusResponse, IngestionRun,
    ListRunsRequest, ListRunsResponse, SourceState, SourceStatus, TriggerBackfillRequest,
    TriggerBackfillResponse, TriggerRunRequest, TriggerRunResponse,
};
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use super::common::{db_err, require_auth, to_timestamp};

pub struct IngestionServiceImpl {
    repos: Repos,
    restate_url: String,
    restate_admin_url: String,
    http_client: reqwest::Client,
}

impl IngestionServiceImpl {
    pub fn new(repos: Repos, restate_url: String, restate_admin_url: String) -> Self {
        Self {
            repos,
            restate_url,
            restate_admin_url,
            http_client: reqwest::Client::new(),
        }
    }

    /// Check whether a Restate invocation is still alive.
    /// Returns `true` if the invocation is actively running/suspended, `false`
    /// if it has completed, been cancelled, or doesn't exist.
    async fn is_invocation_alive(&self, invocation_id: &str) -> bool {
        let url = format!(
            "{}/restate/invocations/{}",
            self.restate_admin_url, invocation_id,
        );

        let resp = match self.http_client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                // If we can't reach Restate, assume alive to avoid false reconciliation
                warn!(error = %e, "failed to reach Restate admin for reconciliation");
                return true;
            }
        };

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return false;
        }

        if !resp.status().is_success() {
            // Unexpected error — assume alive
            return true;
        }

        let body: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(_) => return true,
        };

        // Restate invocation status: "pending", "ready", "running",
        // "suspended", "backing-off", "completed"
        !matches!(
            body.get("status").and_then(|s| s.as_str()),
            Some("completed")
        )
    }

    /// Reconcile sources that the DB thinks are active but whose Restate
    /// invocations are no longer running. Mutates the slice in-place so
    /// callers see corrected `has_active_run` values.
    async fn reconcile_stale_runs(&self, sources: &mut [SourceStatusRow]) {
        for source in sources.iter_mut() {
            if !source.has_active_run {
                continue;
            }

            let invocation_id = match &source.current_invocation_id {
                Some(id) if !id.is_empty() => id.clone(),
                _ => continue, // No invocation ID to check
            };

            if !self.is_invocation_alive(&invocation_id).await {
                warn!(
                    source = %source.name,
                    %invocation_id,
                    "reconciling stale run — Restate invocation no longer active",
                );

                if let Err(e) = self
                    .repos
                    .activity
                    .cancel_active_runs_with_reason(
                        &source.name,
                        "Cancelled — invocation no longer active in Restate",
                    )
                    .await
                {
                    warn!(source = %source.name, error = %e, "failed to reconcile stale run");
                    continue;
                }

                // Update in-memory state so this request returns correct data
                source.has_active_run = false;
                source.active_run_items = None;
                source.active_run_started_at = None;
                source.current_invocation_id = None;
            }
        }
    }

    /// Send a fire-and-forget request to Restate and return the invocation ID.
    async fn send_to_restate(
        &self,
        url: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<String, Status> {
        let mut req = self.http_client.post(url);
        if let Some(body) = body {
            req = req.header("Content-Type", "application/json").json(body);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| Status::unavailable(format!("failed to reach Restate: {e}")))?;

        let resp_body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Status::internal(format!("failed to parse Restate response: {e}")))?;

        resp_body
            .get("invocationId")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| Status::internal("Restate response missing invocationId"))
    }
}

#[tonic::async_trait]
impl IngestionService for IngestionServiceImpl {
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

                SourceStatus {
                    name: s.name,
                    source_type: s.source_type,
                    state: state.into(),
                    last_run,
                    next_run: None, // TODO: compute from schedule_cron
                    items_collected,
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

        let invocation_id = self.send_to_restate(&url, None).await?;

        // Store the invocation ID for cancellation
        self.repos
            .activity
            .set_current_invocation_id(&req.source_name, &invocation_id)
            .await
            .map_err(db_err)?;

        info!(source = %req.source_name, %invocation_id, "triggered ingestion run via Restate");

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

        let invocation_id = self.send_to_restate(&url, Some(&body)).await?;

        // Store the invocation ID for cancellation
        self.repos
            .activity
            .set_current_invocation_id(&req.source_name, &invocation_id)
            .await
            .map_err(db_err)?;

        info!(source = %req.source_name, since = %req.since_date, %invocation_id, "triggered backfill via Restate");

        Ok(Response::new(TriggerBackfillResponse {}))
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

        // Get the current invocation ID
        let invocation_id = self
            .repos
            .activity
            .get_current_invocation_id(&req.source_name)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("no active invocation found for this source"))?;

        // Cancel the Restate invocation via admin API
        let url = format!(
            "{}/restate/invocations/{}",
            self.restate_admin_url, invocation_id,
        );

        let resp = self
            .http_client
            .delete(&url)
            .send()
            .await
            .map_err(|e| Status::unavailable(format!("failed to reach Restate admin: {e}")))?;

        if !resp.status().is_success() {
            let status_code = resp.status();
            let body = resp.text().await.unwrap_or_default();
            info!(source = %req.source_name, %status_code, %body, "Restate cancel response");
            // Don't fail — the invocation may have already completed
        }

        // Mark active runs as cancelled in the database
        self.repos
            .activity
            .cancel_active_runs(&req.source_name)
            .await
            .map_err(db_err)?;

        info!(source = %req.source_name, %invocation_id, "cancelled ingestion run");

        Ok(Response::new(CancelRunResponse {}))
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
