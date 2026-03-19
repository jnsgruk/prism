use std::sync::Arc;

use ps_core::models::TaskType;
use ps_core::repo::reasoning::QueuedContribution;
use ps_reasoning::cost::CostTracker;
use ps_reasoning::features::enrichment;
use ps_reasoning::features::enrichment::types::EnrichmentType;
use ps_reasoning::routing::TaskRouter;
use restate_sdk::prelude::*;
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::SharedState;

/// Max contributions to process per enrichment type per cycle.
/// Kept small so sequential AI API calls complete within Restate's inactivity timeout.
const MAX_BATCH_SIZE: i64 = 10;

pub struct EnrichmentHandlerImpl {
    pub state: SharedState,
    pub router: Arc<RwLock<TaskRouter>>,
}

#[restate_sdk::service]
pub trait EnrichmentHandler {
    /// Run a single enrichment cycle: process all un-enriched contributions for all types.
    async fn run_cycle() -> Result<(), TerminalError>;
}

impl EnrichmentHandler for EnrichmentHandlerImpl {
    async fn run_cycle(&self, ctx: Context<'_>) -> Result<(), TerminalError> {
        self.run_enrichment_cycle(&ctx).await
    }
}

/// Progress report for the enrichment pipeline (stored as run progress JSON).
#[derive(Serialize)]
struct EnrichmentProgress {
    phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    review_depth: Option<TypeProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sentiment: Option<TypeProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    significance: Option<TypeProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: Option<TypeProgress>,
    status_message: String,
}

#[derive(Serialize, Clone)]
struct TypeProgress {
    processed: usize,
    errors: usize,
}

impl EnrichmentHandlerImpl {
    /// Run a full enrichment cycle with Restate-native journaling.
    ///
    /// Each step is wrapped in `ctx.run()` for idempotency on replay:
    /// - Run creation (so retries don't create duplicate run records)
    /// - Finding unenriched contributions (DB read, journaled so replay skips)
    /// - Storing enrichment results (DB write, idempotent via upsert)
    ///
    /// AI API calls are intentionally OUTSIDE `ctx.run()`:
    /// - Responses are large (wasteful to journal)
    /// - Re-enriching is safe (upsert with ON CONFLICT)
    /// - No secrets in the journal (API key is in the `TaskRouter`)
    async fn run_enrichment_cycle(&self, ctx: &Context<'_>) -> Result<(), TerminalError> {
        // Step 1: Create run record (journaled — retries reuse the same run_id)
        let run_id = self.create_run(ctx).await?;

        let mut total_processed = 0i32;
        let mut total_errors = 0usize;
        let mut first_error: Option<String> = None;
        let mut progress = EnrichmentProgress {
            phase: "starting".into(),
            review_depth: None,
            sentiment: None,
            significance: None,
            topic: None,
            status_message: "Starting enrichment cycle".into(),
        };

        'outer: for enrichment_type in EnrichmentType::all() {
            let type_name = enrichment_type.as_str();
            let mut type_processed = 0usize;
            let mut type_errors = 0usize;

            loop {
                // Budget check (outside ctx.run — read-only, re-checking on replay is correct)
                let router = self.router.read().await;
                if let Some(cap) = router.budget_cap_usd() {
                    let cost_tracker = CostTracker::new(self.state.repos.reasoning.clone());
                    match cost_tracker.check_budget(cap).await {
                        Ok(true) => {}
                        Ok(false) => {
                            info!(cap, "daily budget exceeded, pausing enrichment");
                            break 'outer;
                        }
                        Err(e) => {
                            warn!(error = %e, "failed to check budget, continuing cautiously");
                        }
                    }
                }
                drop(router);

                // Step 2: Find queued contributions missing this enrichment type (journaled DB read)
                let contributions = self.find_queued(ctx, type_name).await?;

                if contributions.is_empty() {
                    debug!(enrichment = type_name, "no more contributions to enrich");
                    break;
                }

                info!(
                    enrichment = type_name,
                    count = contributions.len(),
                    batch_num = type_processed / MAX_BATCH_SIZE as usize + 1,
                    "processing enrichment batch"
                );

                // Update progress: starting this type
                progress.phase = type_name.into();
                progress.status_message = format!("Processing {type_name}");
                self.update_progress(run_id, total_processed, &progress)
                    .await;

                // Step 3: AI API calls (NOT journaled — idempotent, large payloads)
                let router = self.router.read().await;
                let batch = enrichment::process_queued_enrichment_batch(
                    &router,
                    &self.state.repos.reasoning,
                    *enrichment_type,
                    &contributions,
                )
                .await;
                drop(router);

                // Capture first error for the run record
                if first_error.is_none() {
                    first_error.clone_from(&batch.first_error);
                }

                let batch_processed = batch.processed;
                let batch_errors = batch.errors;

                // Step 4: Log cost (journaled DB write)
                self.log_cost(ctx, type_name, &batch).await;

                #[allow(clippy::cast_possible_wrap)]
                {
                    total_processed += batch_processed as i32;
                }
                type_processed += batch_processed;
                type_errors += batch_errors;
                total_errors += batch_errors;

                // Update per-type progress (accumulates across batches)
                let tp = TypeProgress {
                    processed: type_processed,
                    errors: type_errors,
                };
                match type_name {
                    "review_depth" => progress.review_depth = Some(tp),
                    "sentiment" => progress.sentiment = Some(tp),
                    "significance" => progress.significance = Some(tp),
                    "topic" => progress.topic = Some(tp),
                    _ => {}
                }
                progress.status_message = format!(
                    "Completed {type_name} batch: {batch_processed} processed, {batch_errors} errors ({type_processed} total)"
                );
                self.update_progress(run_id, total_processed, &progress)
                    .await;

                // Clean up fully enriched entries between batches to free queue slots
                self.delete_fully_enriched(ctx).await;
            }
        }

        // Step 5: Final cleanup of any remaining fully enriched entries
        self.delete_fully_enriched(ctx).await;

        // Step 6: Complete or fail the run (journaled)
        progress.phase = "complete".into();
        progress.status_message =
            format!("Enrichment complete: {total_processed} processed, {total_errors} errors");
        self.update_progress(run_id, total_processed, &progress)
            .await;

        if total_errors > 0 && total_processed == 0 {
            let msg = if let Some(ref err) = first_error {
                format!("processed 0, errors {total_errors}: {err}")
            } else {
                format!("processed 0, errors {total_errors}")
            };
            self.fail_run(ctx, run_id, &msg).await;
            warn!(errors = total_errors, "enrichment cycle failed");
        } else {
            self.complete_run(ctx, run_id, total_processed).await;
            info!(
                processed = total_processed,
                errors = total_errors,
                "enrichment cycle complete"
            );
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // ctx.run() wrappers — journaled, idempotent on replay
    // -----------------------------------------------------------------------

    async fn create_run(&self, ctx: &Context<'_>) -> Result<Uuid, TerminalError> {
        let repos = self.state.repos.clone();
        ctx.run(|| {
            let repos = repos.clone();
            async move {
                let id = Uuid::now_v7();
                repos
                    .activity
                    .create_run(id, "_enrichment", "EnrichmentHandler", "run_cycle")
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                Ok(Json::from(id.to_string()))
            }
        })
        .name("create_run")
        .await?
        .into_inner()
        .parse()
        .map_err(|e| TerminalError::new(format!("invalid run_id: {e}")))
    }

    async fn find_queued(
        &self,
        ctx: &Context<'_>,
        enrichment_type: &str,
    ) -> Result<Vec<QueuedContribution>, TerminalError> {
        let repos = self.state.repos.clone();
        let etype = enrichment_type.to_string();
        Ok(ctx
            .run(|| {
                let repos = repos.clone();
                let etype = etype.clone();
                async move {
                    let contributions = repos
                        .reasoning
                        .find_queued_for_enrichment(&etype, MAX_BATCH_SIZE)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(contributions))
                }
            })
            .name(format!("find_{enrichment_type}"))
            .await?
            .into_inner())
    }

    async fn log_cost(
        &self,
        ctx: &Context<'_>,
        enrichment_type: &str,
        batch: &enrichment::BatchResult,
    ) {
        if batch.total_usage.input_tokens == 0 && batch.total_usage.output_tokens == 0 {
            return;
        }
        let repos = self.state.repos.clone();
        let router = self.router.read().await;
        let task_config = router.task_config(TaskType::Enrichment);
        let provider = task_config.provider.as_str().to_string();
        let model = task_config.model.clone();
        drop(router);

        let cost = ps_reasoning::cost::estimate_cost(&model, &batch.total_usage);
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let input_tokens = batch.total_usage.input_tokens as i32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let output_tokens = batch.total_usage.output_tokens as i32;
        #[allow(clippy::cast_possible_truncation)]
        let cost_f32 = cost as f32;

        let result = ctx
            .run(|| {
                let repos = repos.clone();
                let provider = provider.clone();
                let model = model.clone();
                async move {
                    repos
                        .reasoning
                        .log_api_usage(
                            &provider,
                            &model,
                            "enrichment",
                            input_tokens,
                            output_tokens,
                            cost_f32,
                        )
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name(format!("log_cost_{enrichment_type}"))
            .await;

        if let Err(e) = result {
            warn!(error = %e, "failed to log enrichment cost");
        }
    }

    async fn complete_run(&self, ctx: &Context<'_>, run_id: Uuid, items: i32) {
        let repos = self.state.repos.clone();
        let result = ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    repos
                        .activity
                        .complete_run(run_id, items)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("complete_run")
            .await;

        if let Err(e) = result {
            warn!(error = %e, "failed to complete enrichment run");
        }
    }

    async fn fail_run(&self, ctx: &Context<'_>, run_id: Uuid, error_msg: &str) {
        let repos = self.state.repos.clone();
        let err = error_msg.to_string();
        let result = ctx
            .run(|| {
                let repos = repos.clone();
                let err = err.clone();
                async move {
                    repos
                        .activity
                        .fail_run(run_id, &err)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("fail_run")
            .await;

        if let Err(e) = result {
            warn!(error = %e, "failed to mark enrichment run as failed");
        }
    }

    async fn delete_fully_enriched(&self, ctx: &Context<'_>) {
        let repos = self.state.repos.clone();
        let result = ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    let deleted = repos
                        .reasoning
                        .delete_fully_enriched_entries()
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(deleted))
                }
            })
            .name("delete_fully_enriched")
            .await;

        match result {
            Ok(count) => {
                let deleted = count.into_inner();
                if deleted > 0 {
                    info!(deleted, "cleaned up fully enriched queue entries");
                }
            }
            Err(e) => {
                warn!(error = %e, "failed to delete fully enriched entries");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Non-journaled helpers
    // -----------------------------------------------------------------------

    /// Update run progress (NOT journaled — best-effort, doesn't affect replay).
    async fn update_progress(&self, run_id: Uuid, items: i32, progress: &EnrichmentProgress) {
        let json = serde_json::to_value(progress).unwrap_or_default();
        if let Err(e) = self
            .state
            .repos
            .activity
            .update_run_progress_detail(run_id, items, &json)
            .await
        {
            warn!(error = %e, "failed to update enrichment progress");
        }
    }
}
