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

    /// Check whether a Restate invocation is still alive via the SQL introspection API.
    /// Returns `true` if the invocation is actively running/suspended, `false`
    /// if it has completed, been cancelled, or doesn't exist.
    async fn is_invocation_alive(&self, invocation_id: &str) -> bool {
        let url = format!("{}/query", self.restate_admin_url);
        let query = format!("SELECT status FROM sys_invocation WHERE id = '{invocation_id}'",);

        let resp = match self
            .http_client
            .post(&url)
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                // If we can't reach Restate, assume alive to avoid false reconciliation
                warn!(error = %e, "failed to reach Restate admin for reconciliation");
                return true;
            }
        };

        if !resp.status().is_success() {
            return true;
        }

        let body: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(_) => return true,
        };

        // If no rows, invocation doesn't exist
        let Some(rows) = body.get("rows").and_then(|r| r.as_array()) else {
            return false;
        };

        // If empty, invocation not found — treat as not alive
        let Some(row) = rows.first() else {
            return false;
        };

        // "pending", "ready", "running", "suspended", "backing-off", "completed"
        !matches!(
            row.get("status").and_then(|s| s.as_str()),
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

    /// Query Restate admin SQL API for active invocations on the given virtual object.
    /// Returns `None` if the query fails (best-effort).
    async fn query_active_invocations(&self, source_name: &str) -> Option<Vec<String>> {
        let url = format!("{}/query", self.restate_admin_url);
        let query = format!(
            "SELECT id FROM sys_invocation \
             WHERE target_service_name = 'IngestionHandler' \
             AND target_service_key = '{source_name}' \
             AND status != 'completed'",
        );

        let resp = match self
            .http_client
            .post(&url)
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(source = %source_name, error = %e, "failed to query Restate invocations");
                return None;
            }
        };

        if !resp.status().is_success() {
            warn!(source = %source_name, status = %resp.status(), "Restate invocation query failed");
            return None;
        }

        let body: serde_json::Value = resp.json().await.ok()?;
        let rows = body.get("rows")?.as_array()?;
        let ids: Vec<String> = rows
            .iter()
            .filter_map(|row| row.get("id").and_then(|v| v.as_str()).map(String::from))
            .collect();

        Some(ids)
    }

    /// Cancel a single Restate invocation by ID (best-effort, logs but does not fail).
    async fn cancel_restate_invocation(&self, source_name: &str, invocation_id: &str) {
        let url = format!(
            "{}/invocations/{}?mode=cancel",
            self.restate_admin_url, invocation_id,
        );

        match self.http_client.delete(&url).send().await {
            Ok(resp) if !resp.status().is_success() => {
                let status_code = resp.status();
                let body = resp.text().await.unwrap_or_default();
                info!(source = %source_name, %invocation_id, %status_code, %body, "Restate cancel response");
            }
            Err(e) => {
                warn!(source = %source_name, %invocation_id, error = %e, "failed to cancel Restate invocation");
            }
            _ => {
                info!(source = %source_name, %invocation_id, "cancelled Restate invocation");
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

        // Collect invocation IDs to cancel: start with the stored one, then
        // also query Restate for any active invocations on this virtual object
        // (covers cases where the stored ID is stale or missing).
        let mut ids_to_cancel = Vec::new();

        if let Some(id) = self
            .repos
            .activity
            .get_current_invocation_id(&req.source_name)
            .await
            .map_err(db_err)?
        {
            ids_to_cancel.push(id);
        }

        // Query Restate admin for active invocations on this virtual object
        if let Some(active_ids) = self.query_active_invocations(&req.source_name).await {
            for id in active_ids {
                if !ids_to_cancel.contains(&id) {
                    ids_to_cancel.push(id);
                }
            }
        }

        // Cancel all discovered invocations
        for id in &ids_to_cancel {
            self.cancel_restate_invocation(&req.source_name, id).await;
        }

        // Mark active runs as cancelled in the database
        self.repos
            .activity
            .cancel_active_runs(&req.source_name)
            .await
            .map_err(db_err)?;

        info!(source = %req.source_name, cancelled = ?ids_to_cancel, "cancelled ingestion run");

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
