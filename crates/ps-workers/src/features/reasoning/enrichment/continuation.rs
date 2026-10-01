use super::{EnrichmentHandlerClient, RunCycleArgs};
use crate::features::pipeline::ownership::{OwnedCycleArgs, OwnedProcessingRequest};
use crate::infra::{
    SharedState,
    run_lifecycle::{
        ensure_owned_active, journaled_value, register_owned_invocation, terminal_err,
    },
};
use restate_sdk::prelude::*;
use uuid::Uuid;

pub(super) async fn dispatch_continuation(
    ctx: &Context<'_>,
    state: &SharedState,
    owner: Option<OwnedProcessingRequest>,
    run_id: Uuid,
    completion_awakeable: Option<String>,
) -> Result<(), TerminalError> {
    if let Some(owner) = owner {
        owner.validate_supported()?;
        ensure_owned_active!(ctx, state.repos, owner.pipeline_id)?;
        let pipeline_id = owner.pipeline_id;
        let handle = ctx
            .service_client::<EnrichmentHandlerClient>()
            .run_scoped(Json(OwnedCycleArgs {
                owner,
                parent_run_id: Some(run_id),
                completion_awakeable,
            }))
            .header(
                "x-prism-parent-invocation".into(),
                ctx.invocation_id().to_string(),
            )
            .send()
            .await?;
        register_owned_invocation!(
            ctx,
            state.repos,
            pipeline_id,
            handle,
            "enrichment_continuation",
            Some(run_id)
        );
    } else {
        ctx.service_client::<EnrichmentHandlerClient>()
            .run_cycle(Json(RunCycleArgs {
                parent_run_id: Some(run_id),
                completion_awakeable,
            }))
            .send();
    }
    Ok(())
}
