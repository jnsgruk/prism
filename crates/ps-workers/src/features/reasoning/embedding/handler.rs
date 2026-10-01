use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::EmbeddingHandlerImpl;
use crate::features::pipeline::ownership::OwnedCycleArgs;

/// Arguments carried through a `run_cycle` chain.
///
/// Continuations `.send()` (fire-and-forget) themselves with these args rather
/// than `.call().await` — that keeps the chain flat instead of a deep
/// call-stack of awaiting parents, which was found to pathologically stall
/// under replay when the chain got deep. The caller (e.g. pipeline workflow)
/// waits on `completion_awakeable` instead of on the initial invocation's
/// return, so it still knows when the full chain has drained.
#[derive(Serialize, Deserialize, Default)]
pub struct RunCycleArgs {
    /// Run ID to reuse across the chain. `None` on the initial call.
    pub parent_run_id: Option<Uuid>,
    /// Awakeable ID to resolve once the chain's final invocation drains the
    /// queue. `None` for manual invocations (e.g. UI trigger) that don't need
    /// a completion signal.
    pub completion_awakeable: Option<String>,
}

#[restate_sdk::service]
pub trait EmbeddingHandler {
    /// Run a single embedding cycle: process queued contributions, embed, store.
    ///
    /// When the per-invocation iteration cap is hit, the handler dispatches a
    /// fire-and-forget continuation carrying the same args; the caller awaits
    /// [`RunCycleArgs::completion_awakeable`] to know when the chain drains.
    async fn run_cycle(args: Json<RunCycleArgs>) -> Result<(), TerminalError>;

    async fn run_scoped(args: Json<OwnedCycleArgs>) -> Result<(), TerminalError>;
}

impl EmbeddingHandler for EmbeddingHandlerImpl {
    async fn run_scoped(
        &self,
        ctx: Context<'_>,
        Json(args): Json<OwnedCycleArgs>,
    ) -> Result<(), TerminalError> {
        let awakeable = args.completion_awakeable.clone();
        let result = async {
            args.owner.validate_supported()?;
            self.run_embedding_cycle(
                &ctx,
                RunCycleArgs {
                    parent_run_id: args.parent_run_id,
                    completion_awakeable: args.completion_awakeable,
                },
                Some(args.owner),
            )
            .await
        }
        .await;
        if let Err(ref error) = result
            && let Some(awakeable) = awakeable
        {
            ctx.reject_awakeable(&awakeable, TerminalError::new(error.to_string()));
        }
        result
    }

    async fn run_cycle(
        &self,
        ctx: Context<'_>,
        args: Json<RunCycleArgs>,
    ) -> Result<(), TerminalError> {
        self.run_embedding_cycle(&ctx, args.into_inner(), None)
            .await
    }
}
