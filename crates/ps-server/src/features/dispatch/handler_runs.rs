use ps_proto::canonical::prism::v1::{CancelHandlerRunRequest, CancelHandlerRunResponse};
use tonic::{Request, Response, Status};
use tracing::info;

use super::HandlersServiceImpl;
use crate::common::{db_err, require_auth};

impl HandlersServiceImpl {
    pub(crate) async fn require_unowned_source(&self, source_name: &str) -> Result<(), Status> {
        if self
            .repos
            .activity
            .has_active_pipeline()
            .await
            .map_err(db_err)?
        {
            return Err(Status::failed_precondition(
                "cancel the active pipeline before cancelling individual runs",
            ));
        }

        let active_runs = self
            .repos
            .activity
            .get_active_handler_runs()
            .await
            .map_err(db_err)?;
        if active_runs
            .iter()
            .any(|run| run.source_name == source_name && run.pipeline_id.is_some())
        {
            return Err(Status::failed_precondition(
                "cancel the owning pipeline for this source",
            ));
        }
        Ok(())
    }

    pub(crate) async fn handle_cancel_handler_run(
        &self,
        request: Request<CancelHandlerRunRequest>,
    ) -> Result<Response<CancelHandlerRunResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        if req.run_id.is_empty() {
            return Err(Status::invalid_argument("run_id is required"));
        }

        let run_id: uuid::Uuid = req
            .run_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid run_id"))?;

        // Look up the run to find its source and invocation info
        let run = self
            .repos
            .activity
            .get_run(run_id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("run not found"))?;

        if run.pipeline_id.is_some() {
            return Err(Status::failed_precondition(
                "cancel the owning pipeline for this run",
            ));
        }

        if run.status != ps_core::models::IngestionStatus::Running {
            return Err(Status::failed_precondition("run is not active"));
        }

        self.require_unowned_source(&run.source_name).await?;

        // Service handlers (prefixed with "_") have no Restate object key —
        // cancel the DB record and attempt Restate cancellation via stored invocation ID.
        let is_service_handler = run.source_name.starts_with('_');

        if run.source_name == "_system" {
            // System handlers: just cancel the specific run in the DB
            self.repos
                .activity
                .cancel_run_by_id(run_id)
                .await
                .map_err(db_err)?;
        } else if is_service_handler {
            // Service handlers (_enrichment, _embedding, etc.)
            if let Some(inv_id) = self
                .repos
                .activity
                .get_current_invocation_id(&run.source_name)
                .await
                .map_err(db_err)?
            {
                self.cancel_legacy_restate_invocation(&run.source_name, &inv_id)
                    .await?;
            }
            self.repos
                .activity
                .cancel_active_runs(&run.source_name)
                .await
                .map_err(db_err)?;
        } else {
            // Try stored invocation ID first (DB keyed on display name)
            if let Some(inv_id) = self
                .repos
                .activity
                .get_current_invocation_id(&run.source_name)
                .await
                .map_err(db_err)?
            {
                self.cancel_legacy_restate_invocation(&run.source_name, &inv_id)
                    .await?;
            }

            // Also query Restate for any active invocations (uses source_type as Restate key)
            let restate_key = self
                .repos
                .config
                .get_enabled_source_by_name(&run.source_name)
                .await
                .ok()
                .flatten()
                .map(|s| s.source_type.to_string())
                .unwrap_or_default();

            if !restate_key.is_empty()
                && let Some(active_ids) = self.query_active_invocations(&restate_key).await
            {
                for id in &active_ids {
                    self.cancel_legacy_restate_invocation(&run.source_name, id)
                        .await?;
                }
            }

            self.repos
                .activity
                .cancel_active_runs(&run.source_name)
                .await
                .map_err(db_err)?;
        }

        info!(
            run_id = %req.run_id,
            handler = %run.handler_name,
            source = %run.source_name,
            "cancelled handler run",
        );

        Ok(Response::new(CancelHandlerRunResponse {}))
    }
}
