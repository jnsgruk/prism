use std::sync::Arc;

use ps_core::models::TaskType;
use ps_core::repo::reasoning::QueuedEmbedding;
use ps_reasoning::cost::CostTracker;
use ps_reasoning::features::embeddings;
use ps_reasoning::routing::TaskRouter;
use restate_sdk::prelude::*;
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::SharedState;
use super::run_lifecycle::{complete_run, create_run, fail_run};

/// Max contributions to fetch from the embedding queue per cycle.
const MAX_BATCH_SIZE: i64 = 500;

pub struct EmbeddingHandlerImpl {
    pub state: SharedState,
    pub router: Arc<RwLock<TaskRouter>>,
}

#[restate_sdk::service]
pub trait EmbeddingHandler {
    /// Run a single embedding cycle: process queued contributions, embed, store.
    async fn run_cycle() -> Result<(), TerminalError>;
}

impl EmbeddingHandler for EmbeddingHandlerImpl {
    async fn run_cycle(&self, ctx: Context<'_>) -> Result<(), TerminalError> {
        self.run_embedding_cycle(&ctx).await
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
    async fn run_embedding_cycle(&self, ctx: &Context<'_>) -> Result<(), TerminalError> {
        let start = std::time::Instant::now();

        // Step 1: Create run record (journaled)
        let run_id = create_run!(
            ctx,
            self.state.repos,
            "_embedding",
            "EmbeddingHandler",
            "run_cycle"
        )?;

        let span = tracing::info_span!("handler", handler = "EmbeddingHandler", run_id = %run_id);
        let _guard = span.enter();

        info!("starting embedding cycle");

        // Step 2: Check daily budget (NOT journaled — read-only)
        {
            let router = self.router.read().await;
            if let Some(cap) = router.budget_cap_usd() {
                let cost_tracker = CostTracker::new(self.state.repos.reasoning.clone());
                match cost_tracker.check_budget(cap).await {
                    Ok(true) => {}
                    Ok(false) => {
                        warn!(cap, "daily budget exceeded, pausing embedding");
                        complete_run!(ctx, self.state.repos, run_id, "_embedding", 0i32);
                        return Ok(());
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to check budget, continuing cautiously");
                    }
                }
            }
        }

        // Step 3: Resolve embedding model from TaskRouter (NOT journaled)
        let (model_name, embedding_model) = {
            let router = self.router.read().await;
            let task_config = router.task_config(TaskType::Embeddings);
            let model_name = task_config.model.clone();
            let model = router.embedding_model().map_err(|e| {
                TerminalError::new(format!("failed to resolve embedding model: {e}"))
            })?;
            (model_name, model)
        };

        // Step 4: Fetch queued batch (journaled — DB read)
        let items = self.find_queued(ctx).await?;

        if items.is_empty() {
            debug!("no items in embedding queue");
            complete_run!(ctx, self.state.repos, run_id, "_embedding", 0i32);
            return Ok(());
        }

        let batch_size = items.len();
        info!(batch_size, "processing embedding batch");

        // Step 5: Process batch (NOT journaled — API calls are idempotent on replay)
        let result = embeddings::process_embedding_batch(
            &items,
            embedding_model.as_ref(),
            &self.state.repos,
            &model_name,
        )
        .await
        .map_err(|e| TerminalError::new(format!("embedding error: {e}")))?;

        // Step 6: Log cost (journaled)
        self.log_cost(ctx, &model_name, &result).await;

        // Step 7: Clean up queue (journaled)
        self.cleanup_queue(ctx).await;

        // Step 8: Update progress (NOT journaled)
        let progress = EmbeddingProgress {
            phase: "complete".into(),
            embedded: result.embedded,
            skipped: result.skipped,
            errors: result.errors,
            status_message: format!(
                "Embedded {}, skipped {}, errors {}",
                result.embedded, result.skipped, result.errors
            ),
        };
        self.update_progress(run_id, &result, &progress).await;

        // Step 9: Complete/fail run (journaled)
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let total = result.embedded as i32;
        if result.errors > 0 && result.embedded == 0 {
            fail_run!(
                ctx,
                self.state.repos,
                run_id,
                "_embedding",
                &format!("all {} items failed", result.errors)
            );
            warn!(errors = result.errors, "embedding cycle failed");
        } else {
            complete_run!(ctx, self.state.repos, run_id, "_embedding", total);
            info!(
                embedded = result.embedded,
                skipped = result.skipped,
                errors = result.errors,
                duration_secs = start.elapsed().as_secs(),
                "embedding cycle complete"
            );
        }

        // Step 10: If items remain in queue → self-invoke with short delay
        if batch_size >= MAX_BATCH_SIZE as usize {
            ctx.service_client::<EmbeddingHandlerClient>()
                .run_cycle()
                .send_after(std::time::Duration::from_secs(5));
            debug!("scheduled next embedding cycle (queue may have more items)");
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // ctx.run() wrappers
    // -----------------------------------------------------------------------

    async fn find_queued(&self, ctx: &Context<'_>) -> Result<Vec<QueuedEmbedding>, TerminalError> {
        let repos = self.state.repos.clone();
        Ok(ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    let items = repos
                        .reasoning
                        .find_queued_for_embedding(MAX_BATCH_SIZE)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(items))
                }
            })
            .name("fetch_queue")
            .await?
            .into_inner())
    }

    async fn log_cost(
        &self,
        ctx: &Context<'_>,
        model_name: &str,
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

        let usage = ps_reasoning::rig::completion::Usage {
            input_tokens: result.total_tokens,
            output_tokens: 0,
            total_tokens: result.total_tokens,
            cached_input_tokens: 0,
        };
        let cost = ps_reasoning::cost::estimate_cost(provider, model_name, &usage);
        #[allow(clippy::cast_possible_truncation)]
        let cost_f32 = cost as f32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let tokens = result.total_tokens as i32;
        let model_name = model_name.to_string();

        let log_result = ctx
            .run(|| {
                let repos = repos.clone();
                let model = model_name.clone();
                async move {
                    repos
                        .reasoning
                        .log_api_usage(provider.as_str(), &model, "embedding", tokens, 0, cost_f32)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("log_cost")
            .await;

        if let Err(e) = log_result {
            debug!(error = %e, "failed to log embedding cost");
        }
    }

    async fn cleanup_queue(&self, ctx: &Context<'_>) {
        let repos = self.state.repos.clone();
        let result = ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    let deleted = repos
                        .reasoning
                        .delete_embedded_queue_entries()
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(deleted))
                }
            })
            .name("cleanup_queue")
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

    async fn update_progress(
        &self,
        run_id: Uuid,
        result: &embeddings::BatchResult,
        progress: &EmbeddingProgress,
    ) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let items = result.embedded as i32;
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
