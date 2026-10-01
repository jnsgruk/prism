//! Release admission only after the exact owner and its descendants stop.
use std::collections::{HashMap, HashSet};

use ps_core::models::Pipeline;
use tracing::warn;

use super::{HandlersServiceImpl, validate_restate_identifier};

impl HandlersServiceImpl {
    /// Missing records and transport failures remain uncertain and hold admission.
    /// Legacy cancellation first stops the root; ancestry is safe to inspect only
    /// after that root is confirmed terminal and cannot dispatch more children.
    pub(crate) async fn reconcile_terminal_pipeline(
        &self,
        pipeline: &Pipeline,
    ) -> Result<bool, ps_core::Error> {
        let Some(root_id) = pipeline.current_invocation_id.as_deref() else {
            return Ok(false);
        };
        let root_completed = self.exact_invocation_completed(root_id).await;
        if root_completed != Some(true) {
            if pipeline.request_snapshot == serde_json::json!({}) && pipeline.cancellation_requested
            {
                if root_completed == Some(false) {
                    self.cancel_restate_invocation("_pipeline", root_id).await;
                }
                // Do not send the legacy cooperative cancellation promise. Its
                // workflow can finalize before detached descendants have stopped.
                return Ok(true);
            }
            return Ok(false);
        }

        self.repos
            .activity
            .request_pipeline_stop(pipeline.id)
            .await?;
        let mut ids: HashSet<String> = self
            .repos
            .activity
            .list_pipeline_invocation_ids(pipeline.id)
            .await?
            .into_iter()
            .collect();
        ids.insert(root_id.to_owned());
        // Restate's explicit parent edges cover legacy workflows and the gap
        // between durable child submission and recording it in our registry.
        let Some(discovered) = self.discover_invocation_descendants(&ids).await else {
            return Ok(true);
        };
        ids.extend(discovered.keys().cloned());
        let mut drained = true;
        for id in ids.iter().filter(|id| id.as_str() != root_id) {
            let terminal = match discovered.get(id) {
                Some(terminal) => Some(*terminal),
                None => self.exact_invocation_completed(id).await,
            };
            match terminal {
                Some(true) => {}
                Some(false) => {
                    self.cancel_restate_invocation("_pipeline", id).await;
                    drained = false;
                }
                None => drained = false,
            }
        }
        if drained {
            self.repos
                .activity
                .finish_owned_pipeline(ps_core::repo::activity::PipelineFinishParams {
                    pipeline_id: pipeline.id,
                    status: "failed",
                    stages: &pipeline.stages,
                    error: Some("Workflow stopped before finalizing"),
                    parent_run_id: None,
                })
                .await?;
            warn!(pipeline_id = %pipeline.id, "reconciled terminal pipeline after draining its descendants");
        }
        Ok(true)
    }

    async fn discover_invocation_descendants(
        &self,
        roots: &HashSet<String>,
    ) -> Option<HashMap<String, bool>> {
        let mut visited = roots.clone();
        let mut frontier: Vec<String> = roots.iter().cloned().collect();
        let mut discovered = HashMap::new();
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for batch in frontier.chunks(50) {
                for id in batch {
                    validate_restate_identifier(id).ok()?;
                }
                let identifiers = batch
                    .iter()
                    .map(|id| format!("'{id}'"))
                    .collect::<Vec<_>>()
                    .join(",");
                let query = format!(
                    "SELECT id, invoked_by_id, status FROM sys_invocation WHERE invoked_by_id IN ({identifiers}) LIMIT 10000"
                );
                let body = self.query_exact_invocations(&query).await?;
                let rows = body.get("rows")?.as_array()?;
                // A saturated result may have been truncated; never infer a drain.
                if rows.len() >= 10000 {
                    return None;
                }
                for row in rows {
                    let id = row.get("id")?.as_str()?;
                    validate_restate_identifier(id).ok()?;
                    let parent = row.get("invoked_by_id")?.as_str()?;
                    if id == parent || !batch.iter().any(|id| id == parent) {
                        return None;
                    }
                    let terminal = completed_status(row.get("status")?.as_str()?)?;
                    discovered.insert(id.to_owned(), terminal);
                    if visited.insert(id.to_owned()) {
                        next.push(id.to_owned());
                    }
                }
            }
            if visited.len() > 100_000 {
                return None;
            }
            frontier = next;
        }
        Some(discovered)
    }

    async fn exact_invocation_completed(&self, invocation_id: &str) -> Option<bool> {
        let invocation_id = validate_restate_identifier(invocation_id).ok()?;
        let query = format!("SELECT id, status FROM sys_invocation WHERE id = '{invocation_id}'");
        let body = self.query_exact_invocations(&query).await?;
        let rows = body.get("rows")?.as_array()?;
        let row = rows
            .iter()
            .find(|row| row.get("id").and_then(serde_json::Value::as_str) == Some(invocation_id))?;
        completed_status(row.get("status")?.as_str()?)
    }

    pub(super) async fn query_exact_invocations(&self, query: &str) -> Option<serde_json::Value> {
        let response = match self
            .http_client
            .post(format!("{}/query", self.restate_admin_url))
            .header("Accept", "application/json")
            .json(&serde_json::json!({"query": query}))
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                warn!(%error, "failed to inspect exact pipeline invocations");
                return None;
            }
        };
        if !response.status().is_success() {
            warn!(status = %response.status(), "Restate rejected exact pipeline inspection");
            return None;
        }
        match response.json().await {
            Ok(body) => Some(body),
            Err(error) => {
                warn!(%error, "invalid exact pipeline inspection response");
                None
            }
        }
    }
}

fn completed_status(status: &str) -> Option<bool> {
    match status {
        "completed" => Some(true),
        "pending" | "ready" | "scheduled" | "running" | "suspended" | "backing-off" | "paused" => {
            Some(false)
        }
        _ => None,
    }
}
