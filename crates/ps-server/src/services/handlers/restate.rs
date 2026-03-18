use ps_core::repo::activity::SourceStatusRow;
use tonic::Status;
use tracing::{info, warn};

use super::{HandlersServiceImpl, validate_restate_identifier};

impl HandlersServiceImpl {
    /// Check whether a Restate invocation is still alive via the SQL introspection API.
    /// Returns `true` if the invocation is actively running/suspended, `false`
    /// if it has completed, been cancelled, or doesn't exist.
    pub(crate) async fn is_invocation_alive(&self, invocation_id: &str) -> bool {
        let Ok(invocation_id) = validate_restate_identifier(invocation_id) else {
            warn!(%invocation_id, "invalid invocation ID format, treating as not alive");
            return false;
        };
        let url = format!("{}/query", self.restate_admin_url);
        let query = format!("SELECT status FROM sys_invocation WHERE id = '{invocation_id}'");

        let resp = match self
            .http_client
            .post(&url)
            .header("Accept", "application/json")
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
            Err(e) => {
                warn!(error = %e, "failed to parse Restate admin response as JSON");
                return true;
            }
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
    pub(crate) async fn reconcile_stale_runs(&self, sources: &mut [SourceStatusRow]) {
        // Collect sources that need checking, split into two groups:
        // 1. Sources with an invocation ID — verify against Restate
        // 2. Sources with no invocation ID — inherently stale, cancel immediately
        let mut checks: Vec<(usize, String)> = Vec::new();
        let mut orphaned: Vec<usize> = Vec::new();

        for (i, s) in sources.iter().enumerate() {
            if !s.has_active_run {
                continue;
            }
            match &s.current_invocation_id {
                Some(id) if !id.is_empty() => checks.push((i, id.clone())),
                _ => orphaned.push(i),
            }
        }

        // Check all invocations with IDs in parallel (read-only HTTP calls)
        let alive_results: Vec<(usize, bool)> = futures::future::join_all(
            checks
                .iter()
                .map(|(i, id)| async move { (*i, self.is_invocation_alive(id).await) }),
        )
        .await;

        // Combine: dead invocations + orphaned (no invocation ID at all)
        let stale_indices: Vec<usize> = alive_results
            .into_iter()
            .filter(|(_, alive)| !alive)
            .map(|(idx, _)| idx)
            .chain(orphaned)
            .collect();

        // Process stale results sequentially (writes to DB + in-memory mutation)
        for idx in stale_indices {
            let Some(source) = sources.get_mut(idx) else {
                continue;
            };
            warn!(
                source = %source.name,
                invocation_id = source.current_invocation_id.as_deref().unwrap_or("(none)"),
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

            source.has_active_run = false;
            source.active_run_items = None;
            source.active_run_started_at = None;
            source.current_invocation_id = None;
        }
    }

    /// Query Restate admin SQL API for active invocations on the given virtual object.
    ///
    /// `restate_key` is the Restate virtual object key (i.e. the source type string).
    /// Returns `None` if the query fails (best-effort).
    pub(crate) async fn query_active_invocations(&self, restate_key: &str) -> Option<Vec<String>> {
        let Ok(restate_key) = validate_restate_identifier(restate_key) else {
            warn!(%restate_key, "invalid Restate key format for invocation query");
            return None;
        };
        let url = format!("{}/query", self.restate_admin_url);
        let query = format!(
            "SELECT id FROM sys_invocation \
             WHERE target_service_name IN ('GithubIngestionHandler', 'JiraIngestionHandler', 'DiscourseIngestionHandler') \
             AND target_service_key = '{restate_key}' \
             AND status != 'completed'",
        );

        let resp = match self
            .http_client
            .post(&url)
            .header("Accept", "application/json")
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(restate_key = %restate_key, error = %e, "failed to query Restate invocations");
                return None;
            }
        };

        if !resp.status().is_success() {
            warn!(restate_key = %restate_key, status = %resp.status(), "Restate invocation query failed");
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
    pub(crate) async fn cancel_restate_invocation(&self, source_name: &str, invocation_id: &str) {
        let url = format!(
            "{}/invocations/{}?mode=kill",
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
    pub(crate) async fn send_to_restate(
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
