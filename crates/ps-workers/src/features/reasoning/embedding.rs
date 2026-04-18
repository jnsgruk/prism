use std::sync::Arc;

use ps_core::models::TaskType;
use ps_core::repo::reasoning::QueuedEmbedding;
use ps_reasoning::features::embeddings;
use ps_reasoning::routing::TaskRouter;
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::infra::SharedState;
use crate::infra::run_lifecycle::{
    complete_run, create_run, fail_run, journaled_value, terminal_err,
};

/// Max contributions to fetch from the embedding queue per cycle.
const MAX_BATCH_SIZE: i64 = 100;

/// Max iterations per Restate invocation. Each iteration journals
/// ~3 entries (fetch/log/cleanup), so a cap of 20 keeps the journal ~60
/// entries and bounds replay cost on retry. When more work remains the
/// handler chains a fresh `run_cycle` call — each continuation gets its
/// own invocation with a fresh journal, while the outer caller still
/// awaits full drain through the chain.
const MAX_ITERATIONS_PER_INVOCATION: u32 = 20;

pub struct EmbeddingHandlerImpl {
    pub state: SharedState,
    pub router: Arc<RwLock<TaskRouter>>,
}

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
}

impl EmbeddingHandler for EmbeddingHandlerImpl {
    async fn run_cycle(
        &self,
        ctx: Context<'_>,
        args: Json<RunCycleArgs>,
    ) -> Result<(), TerminalError> {
        self.run_embedding_cycle(&ctx, args.into_inner()).await
    }
}

/// Progress report for the embedding pipeline.
#[derive(Serialize)]
struct EmbeddingProgress {
    phase: String,
    embedded: usize,
    skipped: usize,
    errors: usize,
    status_message: String,
}

impl EmbeddingHandlerImpl {
    async fn run_embedding_cycle(
        &self,
        ctx: &Context<'_>,
        args: RunCycleArgs,
    ) -> Result<(), TerminalError> {
        let start = std::time::Instant::now();

        let is_continuation = args.parent_run_id.is_some();
        // Step 1: Create or reuse run record (journaled on first call only)
        let run_id = match args.parent_run_id {
            Some(id) => id,
            None => create_run!(
                ctx,
                self.state.repos,
                "_embedding",
                "EmbeddingHandler",
                "run_cycle"
            )?,
        };

        let span = tracing::info_span!("handler", handler = "EmbeddingHandler", run_id = %run_id);
        let _guard = span.enter();

        if is_continuation {
            info!("resuming embedding cycle (continuation)");
        } else {
            info!("starting embedding cycle");
        }

        // Step 2: Resolve embedding model from TaskRouter (NOT journaled)
        let (model_name, embedding_model) = {
            let router = self.router.read().await;
            let task_config = router.task_config(TaskType::Embeddings);
            let model_name = task_config.model.clone();
            match router.embedding_model() {
                Ok(model) => (model_name, model),
                Err(e) => {
                    let msg = format!("failed to resolve embedding model: {e}");
                    warn!(%msg);
                    fail_run!(ctx, self.state.repos, run_id, "_embedding", &msg);
                    return Err(TerminalError::new(msg));
                }
            }
        };

        let mut total_embedded = 0usize;
        let mut total_skipped = 0usize;
        let mut total_errors = 0usize;
        let mut iteration = 0u32;
        let mut more_work_remaining = false;

        // On continuation, seed cumulative totals from the run row so progress
        // updates append to the chain total rather than restarting.
        if is_continuation {
            match self.state.repos.activity.get_run(run_id).await {
                Ok(Some(row)) => {
                    #[allow(clippy::cast_sign_loss)]
                    {
                        total_embedded = row.items_collected.unwrap_or(0).max(0) as usize;
                    }
                }
                Ok(None) => warn!(%run_id, "continuation: run row missing, starting at 0"),
                Err(e) => warn!(error = %e, "continuation: failed to read run row"),
            }
        }

        loop {
            // Step 3: Fetch queued batch (journaled — DB read)
            let items = self.find_queued(ctx, iteration).await?;

            if items.is_empty() {
                debug!("no items in embedding queue");
                break;
            }

            let batch_size = items.len();
            info!(batch_size, iteration, "processing embedding batch");

            // Step 4: Process batch (NOT journaled — API calls are idempotent on replay)
            let result = match embeddings::process_embedding_batch(
                &items,
                embedding_model.as_ref(),
                &self.state.repos,
                &model_name,
            )
            .await
            {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!("embedding error: {e}");
                    warn!(%msg);
                    fail_run!(ctx, self.state.repos, run_id, "_embedding", &msg);
                    return Err(TerminalError::new(msg));
                }
            };

            // Step 5: Log cost (journaled)
            self.log_usage(ctx, &model_name, iteration, &result).await;

            // Step 6: Clean up queue (journaled)
            self.cleanup_queue(ctx, iteration).await;

            // Accumulate totals
            total_embedded += result.embedded;
            total_skipped += result.skipped;
            total_errors += result.errors;

            // Step 7: Update progress (NOT journaled)
            let progress = EmbeddingProgress {
                phase: "processing".into(),
                embedded: total_embedded,
                skipped: total_skipped,
                errors: total_errors,
                status_message: format!(
                    "Embedded {total_embedded}, skipped {total_skipped}, errors {total_errors}"
                ),
            };
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let items_so_far = total_embedded as i32;
            self.update_progress(run_id, items_so_far, &progress).await;

            iteration += 1;

            // Last batch was partial → queue is drained
            if batch_size < MAX_BATCH_SIZE as usize {
                break;
            }

            // Bound per-invocation journal growth. Chain a continuation
            // below so the outer caller still awaits full drain.
            if iteration >= MAX_ITERATIONS_PER_INVOCATION {
                more_work_remaining = true;
                break;
            }
        }

        // Only the final invocation in the chain finalises the run and
        // resolves the completion awakeable. Continuations .send()
        // (fire-and-forget) so the chain stays flat.
        if more_work_remaining {
            info!(
                iteration,
                "iteration cap reached; dispatching continuation for remaining queue"
            );
            ctx.service_client::<EmbeddingHandlerClient>()
                .run_cycle(Json(RunCycleArgs {
                    parent_run_id: Some(run_id),
                    completion_awakeable: args.completion_awakeable,
                }))
                .send();
        } else {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let total = total_embedded as i32;
            if total_errors > 0 && total_embedded == 0 {
                fail_run!(
                    ctx,
                    self.state.repos,
                    run_id,
                    "_embedding",
                    &format!("all {total_errors} items failed")
                );
                warn!(errors = total_errors, "embedding cycle failed");
            } else {
                complete_run!(ctx, self.state.repos, run_id, "_embedding", total);
                info!(
                    embedded = total_embedded,
                    skipped = total_skipped,
                    errors = total_errors,
                    duration_secs = start.elapsed().as_secs(),
                    "embedding cycle complete"
                );
            }
            if let Some(awakeable_id) = args.completion_awakeable.as_deref() {
                ctx.resolve_awakeable(awakeable_id, ());
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // ctx.run() wrappers
    // -----------------------------------------------------------------------

    async fn find_queued(
        &self,
        ctx: &Context<'_>,
        iteration: u32,
    ) -> Result<Vec<QueuedEmbedding>, TerminalError> {
        let repos = &self.state.repos;
        let step = format!("fetch_queue_{iteration}");
        Ok(journaled_value!(ctx, step, [repos], {
            repos
                .reasoning
                .find_queued_for_embedding(MAX_BATCH_SIZE)
                .await
                .map_err(terminal_err("db error"))?
        }))
    }

    async fn log_usage(
        &self,
        ctx: &Context<'_>,
        model_name: &str,
        iteration: u32,
        result: &embeddings::BatchResult,
    ) {
        if result.total_tokens == 0 {
            return;
        }

        let repos = self.state.repos.clone();
        let router = self.router.read().await;
        let task_config = router.task_config(TaskType::Embeddings);
        let provider = task_config.provider;
        drop(router);

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let tokens = result.total_tokens as i32;
        let model_name = model_name.to_string();

        let step = format!("log_usage_{iteration}");
        let log_result = ctx
            .run(|| {
                let repos = repos.clone();
                let model = model_name.clone();
                async move {
                    repos
                        .reasoning
                        .log_api_usage(provider.as_str(), &model, "embeddings", tokens, 0)
                        .await
                        .map_err(terminal_err("db error"))?;
                    Ok(Json::from(()))
                }
            })
            .name(&step)
            .await;

        if let Err(e) = log_result {
            debug!(error = %e, "failed to log embedding usage");
        }
    }

    async fn cleanup_queue(&self, ctx: &Context<'_>, iteration: u32) {
        let repos = self.state.repos.clone();
        let step = format!("cleanup_queue_{iteration}");
        let result = ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    let deleted = repos
                        .reasoning
                        .delete_embedded_queue_entries()
                        .await
                        .map_err(terminal_err("db error"))?;
                    Ok(Json::from(deleted))
                }
            })
            .name(&step)
            .await;

        match result {
            Ok(count) => {
                let deleted = count.into_inner();
                if deleted > 0 {
                    debug!(deleted, "cleaned up embedded queue entries");
                }
            }
            Err(e) => {
                warn!(error = %e, "failed to clean up embedding queue");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Non-journaled helpers
    // -----------------------------------------------------------------------

    async fn update_progress(&self, run_id: Uuid, items: i32, progress: &EmbeddingProgress) {
        let json = serde_json::to_value(progress).unwrap_or_default();
        if let Err(e) = self
            .state
            .repos
            .activity
            .update_run_progress_detail(run_id, items, &json)
            .await
        {
            debug!(error = %e, "failed to update embedding progress");
        }
    }
}
