use crate::infra::{
    SharedState,
    run_lifecycle::{
        complete_handler_run, complete_owned_run, complete_run, fail_handler_run, fail_owned_run,
        fail_run,
    },
};
use restate_sdk::prelude::*;
use tracing::{info, warn};
use uuid::Uuid;

pub(super) struct EmbeddingCompletion {
    pub run_id: Uuid,
    pub embedded: usize,
    pub skipped: usize,
    pub errors: usize,
    pub owned: bool,
    pub awakeable: Option<String>,
    pub elapsed: std::time::Duration,
}

pub(super) async fn finish_embedding_cycle(
    ctx: &Context<'_>,
    state: &SharedState,
    completion: EmbeddingCompletion,
) {
    let processed = completion.embedded + completion.skipped + completion.errors;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let total = processed as i32;
    if completion.errors > 0 && completion.embedded == 0 {
        fail_handler_run!(
            completion.owned,
            ctx,
            state.repos,
            completion.run_id,
            "_embedding",
            &format!("all {} items failed", completion.errors)
        );
        warn!(errors = completion.errors, "embedding cycle failed");
    } else {
        complete_handler_run!(
            completion.owned,
            ctx,
            state.repos,
            completion.run_id,
            "_embedding",
            total
        );
        info!(
            embedded = completion.embedded,
            skipped = completion.skipped,
            errors = completion.errors,
            duration_secs = completion.elapsed.as_secs(),
            "embedding cycle complete"
        );
    }
    if let Some(awakeable_id) = completion.awakeable.as_deref() {
        if completion.owned && completion.errors > 0 && completion.embedded == 0 {
            ctx.reject_awakeable(
                awakeable_id,
                TerminalError::new("Embedding processing failed"),
            );
        } else {
            ctx.resolve_awakeable(awakeable_id, ());
        }
    }
}
