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
use super::insights::InsightsHandlerClient;
use super::run_lifecycle::{complete_run, create_run, fail_run};

/// Max contributions to process per enrichment type per batch.
/// Items within a batch are processed concurrently, so this can be larger
/// than when processing was sequential.
const MAX_BATCH_SIZE: i64 = 50;

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
    /// Each iteration fetches one batch per enrichment type, then processes
    /// all types concurrently for maximum throughput. Within each type,
    /// items are also processed concurrently (see `process_queued_enrichment_batch`).
    ///
    /// Journaling strategy:
    /// - `ctx.run()`: run creation, queue lookups (DB reads), cost logging, cleanup
    /// - Outside `ctx.run()`: AI API calls (large, idempotent), budget checks (read-only)
    async fn run_enrichment_cycle(&self, ctx: &Context<'_>) -> Result<(), TerminalError> {
        let start = std::time::Instant::now();

        // Step 1: Create run record (journaled — retries reuse the same run_id)
        let run_id = create_run!(
            ctx,
            self.state.repos,
            "_enrichment",
            "EnrichmentHandler",
            "run_cycle"
        )?;

        let span = tracing::info_span!("handler", handler = "EnrichmentHandler", run_id = %run_id);
        let _guard = span.enter();

        info!("starting enrichment cycle");

        let mut total_processed = 0i32;
        let mut total_errors = 0usize;
        let mut first_error: Option<String> = None;
        let mut rd_stats = TypeProgress {
            processed: 0,
            errors: 0,
        };
        let mut se_stats = TypeProgress {
            processed: 0,
            errors: 0,
        };
        let mut si_stats = TypeProgress {
            processed: 0,
            errors: 0,
        };
        let mut to_stats = TypeProgress {
            processed: 0,
            errors: 0,
        };
        let mut progress = EnrichmentProgress {
            phase: "starting".into(),
            review_depth: None,
            sentiment: None,
            significance: None,
            topic: None,
            status_message: "Starting enrichment cycle".into(),
        };

        let mut iteration = 0u32;
        let mut cleanup_counter = 0u32;

        loop {
            // Budget check (outside ctx.run — read-only, re-checking on replay is correct)
            let router = self.router.read().await;
            if let Some(cap) = router.budget_cap_usd() {
                let cost_tracker = CostTracker::new(self.state.repos.reasoning.clone());
                match cost_tracker.check_budget(cap).await {
                    Ok(true) => {}
                    Ok(false) => {
                        warn!(cap, "daily budget exceeded");
                        break;
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to check budget, continuing cautiously");
                    }
                }
            }
            drop(router);

            // Step 2: Fetch one batch per enrichment type (journaled DB reads, fast)
            let all_types = EnrichmentType::all();
            let mut batches = Vec::with_capacity(all_types.len());
            let mut any_non_empty = false;
            for enrichment_type in all_types {
                let contributions = self
                    .find_queued(ctx, enrichment_type.as_str(), iteration)
                    .await?;
                if !contributions.is_empty() {
                    any_non_empty = true;
                }
                batches.push((*enrichment_type, contributions));
            }

            if !any_non_empty {
                debug!("no more contributions to enrich across any type");
                break;
            }

            // Update progress before AI calls
            progress.phase = "processing".into();
            progress.status_message = format!(
                "Processing batches: {}",
                batches
                    .iter()
                    .filter(|(_, c)| !c.is_empty())
                    .map(|(t, c)| format!("{}={}", t.as_str(), c.len()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            self.update_progress(run_id, total_processed, &progress)
                .await;

            // Step 3: Process ALL types concurrently (AI calls NOT journaled)
            let router = self.router.read().await;
            let repo = &self.state.repos.reasoning;
            let futures: Vec<_> = batches
                .iter()
                .filter(|(_, contributions)| !contributions.is_empty())
                .map(|(etype, contributions)| {
                    enrichment::process_queued_enrichment_batch(
                        &router,
                        repo,
                        *etype,
                        contributions,
                    )
                })
                .collect();
            let results: Vec<enrichment::BatchResult> = futures::future::join_all(futures).await;
            drop(router);

            // Step 4: Aggregate results and log costs (journaled DB writes)
            for batch in &results {
                let type_name = batch.enrichment_type.as_str();

                if first_error.is_none() {
                    first_error.clone_from(&batch.first_error);
                }

                #[allow(clippy::cast_possible_wrap)]
                {
                    total_processed += batch.processed as i32;
                }
                total_errors += batch.errors;

                // Update per-type cumulative stats
                let stats = match batch.enrichment_type {
                    EnrichmentType::ReviewDepth => &mut rd_stats,
                    EnrichmentType::Sentiment => &mut se_stats,
                    EnrichmentType::Significance => &mut si_stats,
                    EnrichmentType::Topic => &mut to_stats,
                };
                stats.processed += batch.processed;
                stats.errors += batch.errors;

                self.log_cost(ctx, type_name, iteration, batch).await;
            }

            // Update progress with cumulative per-type stats
            progress.review_depth = Some(rd_stats.clone());
            progress.sentiment = Some(se_stats.clone());
            progress.significance = Some(si_stats.clone());
            progress.topic = Some(to_stats.clone());
            progress.status_message = format!(
                "Batch complete: {} processed, {} errors ({total_processed} total)",
                results.iter().map(|r| r.processed).sum::<usize>(),
                results.iter().map(|r| r.errors).sum::<usize>(),
            );
            self.update_progress(run_id, total_processed, &progress)
                .await;

            // Clean up fully enriched entries between batches to free queue slots
            self.delete_fully_enriched(ctx, cleanup_counter).await;
            cleanup_counter += 1;
            iteration += 1;
        }

        // Step 5: Final cleanup of any remaining fully enriched entries
        self.delete_fully_enriched(ctx, cleanup_counter).await;

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
            fail_run!(ctx, self.state.repos, run_id, "_enrichment", &msg);
            warn!(errors = total_errors, "enrichment cycle failed");
        } else {
            complete_run!(
                ctx,
                self.state.repos,
                run_id,
                "_enrichment",
                total_processed
            );
            info!(
                processed = total_processed,
                errors = total_errors,
                duration_secs = start.elapsed().as_secs(),
                "complete"
            );

            // Trigger insight snapshot recomputation after successful enrichment.
            if total_processed > 0 {
                ctx.service_client::<InsightsHandlerClient>()
                    .compute_current_periods()
                    .send();
                debug!("triggered InsightsHandler after enrichment cycle");
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // ctx.run() wrappers — journaled, idempotent on replay
    // -----------------------------------------------------------------------

    async fn find_queued(
        &self,
        ctx: &Context<'_>,
        enrichment_type: &str,
        iteration: u32,
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
            .name(format!("find_{enrichment_type}_{iteration}"))
            .await?
            .into_inner())
    }

    async fn log_cost(
        &self,
        ctx: &Context<'_>,
        enrichment_type: &str,
        iteration: u32,
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
            .name(format!("log_cost_{enrichment_type}_{iteration}"))
            .await;

        if let Err(e) = result {
            debug!(error = %e, "failed to log enrichment cost");
        }
    }

    async fn delete_fully_enriched(&self, ctx: &Context<'_>, cleanup_counter: u32) {
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
            .name(format!("cleanup_{cleanup_counter}"))
            .await;

        match result {
            Ok(count) => {
                let deleted = count.into_inner();
                if deleted > 0 {
                    debug!(deleted, "cleaned up fully enriched queue entries");
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
            debug!(error = %e, "failed to update enrichment progress");
        }
    }
}
