//! Ownership and processing boundary for versioned downstream handlers.

use ps_core::ingestion::PipelineRequest;
use restate_sdk::prelude::{ContextSideEffects, RunFuture, TerminalError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infra::run_lifecycle::terminal_err;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedProcessingRequest {
    pub pipeline_id: Uuid,
    pub request: PipelineRequest,
}

impl OwnedProcessingRequest {
    pub fn validate_supported(&self) -> Result<(), TerminalError> {
        self.request
            .validate()
            .map_err(terminal_err("invalid processing snapshot"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedCycleArgs {
    pub owner: OwnedProcessingRequest,
    pub parent_run_id: Option<Uuid>,
    pub completion_awakeable: Option<String>,
}

pub(crate) struct CycleRun<'a> {
    pub owner: Option<&'a OwnedProcessingRequest>,
    pub parent_run_id: Option<Uuid>,
    pub source_name: &'a str,
    pub handler_name: &'a str,
    pub continuation_kind: &'a str,
}

pub(crate) async fn create_cycle_run(
    ctx: &restate_sdk::prelude::Context<'_>,
    state: &crate::infra::SharedState,
    run: CycleRun<'_>,
) -> Result<Uuid, TerminalError> {
    use crate::infra::run_lifecycle::{
        create_owned_run, create_run, ensure_owned_active, journaled_value, register_owned_self,
        terminal_err,
    };
    if let Some(owner) = run.owner {
        if run.parent_run_id.is_some() {
            register_owned_self!(
                ctx,
                state.repos,
                owner.pipeline_id,
                run.continuation_kind,
                run.parent_run_id
            );
        }
        ensure_owned_active!(ctx, state.repos, owner.pipeline_id)?;
    }
    match (run.parent_run_id, run.owner) {
        (Some(id), _) => Ok(id),
        (None, Some(owner)) => Ok(create_owned_run!(
            ctx,
            state.repos,
            owner.pipeline_id,
            run.source_name,
            run.handler_name,
            "run_cycle"
        )),
        (None, None) => create_run!(
            ctx,
            state.repos,
            run.source_name,
            run.handler_name,
            "run_cycle"
        ),
    }
}
