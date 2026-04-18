use std::sync::Arc;

use ps_core::models::EnrichmentType;
use ps_core::models::TaskType;
use ps_core::repo::reasoning::{EmbeddingQueueEntry, QueuedContribution};
use ps_reasoning::features::enrichment;
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

/// Max contributions to process per enrichment type per batch.
/// Items within a batch are processed concurrently, so this can be larger
/// than when processing was sequential.
const MAX_BATCH_SIZE: i64 = 50;

/// Max iterations per Restate invocation. Each iteration journals ~6 entries
/// (one `find_*` per type + `process_*` + `commit_*`) with significant payload
/// (serialized batches and results), so a cap of 10 keeps the journal ~60
/// entries and bounds replay cost on retry. When more work remains the handler
/// chains a fresh `run_cycle` call — each continuation gets its own invocation
/// with a fresh journal, while the outer caller still awaits full drain
/// through the chain.
const MAX_ITERATIONS_PER_INVOCATION: u32 = 10;

pub struct EnrichmentHandlerImpl {
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
pub trait EnrichmentHandler {
    /// Run a single enrichment cycle: process all un-enriched contributions for all types.
    ///
    /// When the per-invocation iteration cap is hit, the handler dispatches a
    /// fire-and-forget continuation carrying the same args; the caller awaits
    /// [`RunCycleArgs::completion_awakeable`] to know when the chain drains.
    async fn run_cycle(args: Json<RunCycleArgs>) -> Result<(), TerminalError>;
}

impl EnrichmentHandler for EnrichmentHandlerImpl {
    async fn run_cycle(
        &self,
        ctx: Context<'_>,
        args: Json<RunCycleArgs>,
    ) -> Result<(), TerminalError> {
        self.run_enrichment_cycle(&ctx, args.into_inner()).await
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

/// Mutable state accumulated across enrichment batches.
struct CycleState {
    total_processed: i32,
    total_errors: usize,
    first_error: Option<String>,
    rd_stats: TypeProgress,
    se_stats: TypeProgress,
    si_stats: TypeProgress,
    to_stats: TypeProgress,
    progress: EnrichmentProgress,
    iteration: u32,
}

impl CycleState {
    fn new() -> Self {
        Self {
            total_processed: 0,
            total_errors: 0,
            first_error: None,
            rd_stats: TypeProgress {
                processed: 0,
                errors: 0,
            },
            se_stats: TypeProgress {
                processed: 0,
                errors: 0,
            },
            si_stats: TypeProgress {
                processed: 0,
                errors: 0,
            },
            to_stats: TypeProgress {
                processed: 0,
                errors: 0,
            },
            progress: EnrichmentProgress {
                phase: "starting".into(),
                review_depth: None,
                sentiment: None,
                significance: None,
                topic: None,
                status_message: "Starting enrichment cycle".into(),
            },
            iteration: 0,
        }
    }

    /// Per-type telemetry (enrichment-row counts). Does NOT update
    /// `total_processed` — that is handled once per iteration by
    /// [`aggregate_iteration`] with distinct contribution counting.
    fn aggregate_batch(&mut self, batch: &enrichment::BatchResult) {
        if self.first_error.is_none() {
            self.first_error.clone_from(&batch.first_error);
        }
        self.total_errors += batch.errors;

        let stats = match batch.enrichment_type {
            EnrichmentType::ReviewDepth => &mut self.rd_stats,
            EnrichmentType::Sentiment => &mut self.se_stats,
            EnrichmentType::Significance => &mut self.si_stats,
            EnrichmentType::Topic => &mut self.to_stats,
        };
        stats.processed += batch.processed;
        stats.errors += batch.errors;
    }

    /// Count distinct contributions successfully processed across all types
    /// in an iteration. A `pr_review` can produce 2 enrichment rows and a
    /// large `pull_request` up to 3, but each is one unit of work for the
    /// progress UI.
    fn aggregate_iteration(&mut self, batches: &[enrichment::BatchResult]) {
        let mut distinct = std::collections::HashSet::new();
        for b in batches {
            distinct.extend(b.successful_contribution_ids.iter().copied());
        }
        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        {
            self.total_processed += distinct.len() as i32;
        }
    }

    fn update_progress_after_batch(&mut self, results: &[enrichment::BatchResult]) {
        self.progress.review_depth = Some(self.rd_stats.clone());
        self.progress.sentiment = Some(self.se_stats.clone());
        self.progress.significance = Some(self.si_stats.clone());
        self.progress.topic = Some(self.to_stats.clone());
        self.progress.status_message = format!(
            "Batch complete: {} processed, {} errors ({} total)",
            results.iter().map(|r| r.processed).sum::<usize>(),
            results.iter().map(|r| r.errors).sum::<usize>(),
            self.total_processed,
        );
    }
}

impl EnrichmentHandlerImpl {
    /// Run a full enrichment cycle with Restate-native journaling.
    ///
    /// Each iteration fetches one batch per enrichment type, then processes
    /// all types concurrently for maximum throughput. Within each type,
    /// items are also processed concurrently (see `process_queued_enrichment_batch`).
    ///
    /// Journaling strategy:
    /// - `ctx.run()`: run creation, queue lookups (DB reads), AI processing
    ///   results, cost logging, cleanup
    /// - Outside `ctx.run()`: budget checks (read-only), progress updates
    async fn run_enrichment_cycle(
        &self,
        ctx: &Context<'_>,
        args: RunCycleArgs,
    ) -> Result<(), TerminalError> {
        let start = std::time::Instant::now();

        let is_continuation = args.parent_run_id.is_some();
        let run_id = match args.parent_run_id {
            Some(id) => id,
            None => create_run!(
                ctx,
                self.state.repos,
                "_enrichment",
                "EnrichmentHandler",
                "run_cycle"
            )?,
        };

        let span = tracing::info_span!("handler", handler = "EnrichmentHandler", run_id = %run_id);
        let _guard = span.enter();
        if is_continuation {
            info!("resuming enrichment cycle (continuation)");
        } else {
            info!("starting enrichment cycle");
        }

        let mut s = CycleState::new();
        // On continuation, seed the cumulative items count from the run row so
        // progress updates append to the chain total rather than restarting.
        if is_continuation {
            match self.state.repos.activity.get_run(run_id).await {
                Ok(Some(row)) => s.total_processed = row.items_collected.unwrap_or(0),
                Ok(None) => warn!(%run_id, "continuation: run row missing, starting at 0"),
                Err(e) => warn!(error = %e, "continuation: failed to read run row"),
            }
        }
        let mut more_work_remaining = false;

        loop {
            let batches = self.fetch_all_type_batches(ctx, s.iteration).await?;
            if batches.iter().all(|(_, c)| c.is_empty()) {
                debug!("no more contributions to enrich across any type");
                break;
            }

            s.progress.phase = "processing".into();
            s.progress.status_message = format!(
                "Processing batches: {}",
                batches
                    .iter()
                    .filter(|(_, c)| !c.is_empty())
                    .map(|(t, c)| format!("{}={}", t.as_str(), c.len()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            self.update_progress(run_id, s.total_processed, &s.progress)
                .await;

            // Journal the AI processing results so replays skip the
            // expensive AI calls entirely. Without this, every Restate
            // replay re-calls the AI APIs for all previous iterations.
            let results: Vec<enrichment::BatchResult> = {
                let router = self.router.clone();
                let repos = self.state.repos.clone();
                let batches = batches.clone();
                let step_name = format!("process_{}", s.iteration);
                journaled_value!(ctx, step_name, [router, repos, batches], {
                    process_batches_inner(&router, &repos.reasoning, &batches).await
                })
            };

            let batch_ids: Vec<Uuid> = batches
                .iter()
                .flat_map(|(_, contributions)| contributions.iter().map(|c| c.contribution_id))
                .collect();

            for batch in &results {
                s.aggregate_batch(batch);
            }
            s.aggregate_iteration(&results);

            // Commit ALL post-AI DB writes in a single ctx.run().
            self.commit_iteration(ctx, &batch_ids, &results, s.iteration)
                .await;

            s.update_progress_after_batch(&results);
            self.update_progress(run_id, s.total_processed, &s.progress)
                .await;

            s.iteration += 1;

            // Bound per-invocation journal growth. Chain a continuation
            // below so the outer caller still awaits full drain.
            if s.iteration >= MAX_ITERATIONS_PER_INVOCATION {
                more_work_remaining = true;
                break;
            }
        }

        // Only the final invocation in the chain runs cleanup, finalises the
        // run, and resolves the completion awakeable. Continuations .send()
        // (fire-and-forget) and return immediately — the chain is kept flat
        // instead of nested so parents don't get stuck in deep replay waits.
        if more_work_remaining {
            info!(
                iteration = s.iteration,
                "iteration cap reached; dispatching continuation for remaining queue"
            );
            ctx.service_client::<EnrichmentHandlerClient>()
                .run_cycle(Json(RunCycleArgs {
                    parent_run_id: Some(run_id),
                    completion_awakeable: args.completion_awakeable,
                }))
                .send();
        } else {
            self.delete_fully_enriched(ctx, s.iteration).await;
            self.finalize_run(ctx, run_id, &mut s, start.elapsed())
                .await;
            if let Some(awakeable_id) = args.completion_awakeable.as_deref() {
                ctx.resolve_awakeable(awakeable_id, ());
            }
        }

        Ok(())
    }

    /// Fetch one batch of queued contributions per enrichment type.
    async fn fetch_all_type_batches(
        &self,
        ctx: &Context<'_>,
        iteration: u32,
    ) -> Result<Vec<(EnrichmentType, Vec<QueuedContribution>)>, TerminalError> {
        let all_types = EnrichmentType::all();
        let mut batches = Vec::with_capacity(all_types.len());
        for enrichment_type in all_types {
            let contributions = self.find_queued(ctx, *enrichment_type, iteration).await?;
            batches.push((*enrichment_type, contributions));
        }
        Ok(batches)
    }

    /// Complete or fail the run based on accumulated stats.
    async fn finalize_run(
        &self,
        ctx: &Context<'_>,
        run_id: Uuid,
        s: &mut CycleState,
        elapsed: std::time::Duration,
    ) {
        s.progress.phase = "complete".into();
        s.progress.status_message = format!(
            "Enrichment complete: {} processed, {} errors",
            s.total_processed, s.total_errors,
        );
        self.update_progress(run_id, s.total_processed, &s.progress)
            .await;

        if s.total_errors > 0 && s.total_processed == 0 {
            let msg = if let Some(ref err) = s.first_error {
                format!("processed 0, errors {}: {err}", s.total_errors)
            } else {
                format!("processed 0, errors {}", s.total_errors)
            };
            fail_run!(ctx, self.state.repos, run_id, "_enrichment", &msg);
            warn!(errors = s.total_errors, "enrichment cycle failed");
        } else {
            complete_run!(
                ctx,
                self.state.repos,
                run_id,
                "_enrichment",
                s.total_processed
            );
            info!(
                processed = s.total_processed,
                errors = s.total_errors,
                duration_secs = elapsed.as_secs(),
                "complete"
            );
        }
    }

    // -----------------------------------------------------------------------
    // ctx.run() wrappers — journaled, idempotent on replay
    // -----------------------------------------------------------------------

    async fn find_queued(
        &self,
        ctx: &Context<'_>,
        enrichment_type: EnrichmentType,
        iteration: u32,
    ) -> Result<Vec<QueuedContribution>, TerminalError> {
        let repos = &self.state.repos;
        let step_name = format!("find_{}_{iteration}", enrichment_type.as_str());
        Ok(journaled_value!(ctx, step_name, [repos], {
            repos
                .reasoning
                .find_queued_for_enrichment(enrichment_type, MAX_BATCH_SIZE)
                .await
                .map_err(terminal_err("db error"))?
        }))
    }

    /// Commit all post-AI work for one iteration in a single `ctx.run()`.
    ///
    /// Batches embedding enqueue, usage logging, and queue cleanup into one
    /// journal entry to minimise suspension overhead.
    async fn commit_iteration(
        &self,
        ctx: &Context<'_>,
        contribution_ids: &[Uuid],
        results: &[enrichment::BatchResult],
        iteration: u32,
    ) {
        let repos = self.state.repos.clone();

        // Pre-compute everything we need inside the closure.
        let mut unique_ids: Vec<Uuid> = contribution_ids.to_vec();
        unique_ids.sort_unstable();
        unique_ids.dedup();

        let entries: Vec<EmbeddingQueueEntry> = unique_ids
            .into_iter()
            .map(|id| EmbeddingQueueEntry {
                contribution_id: id,
                content_hash: String::new(), // computed at embed time
            })
            .collect();

        let router = self.router.read().await;
        let task_config = router.task_config(TaskType::Enrichment);
        let provider_str = task_config.provider.as_str().to_string();
        let model = task_config.model.clone();
        drop(router);

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let usage_records: Vec<(i32, i32)> = results
            .iter()
            .filter(|b| b.total_usage.input_tokens > 0 || b.total_usage.output_tokens > 0)
            .map(|b| {
                (
                    b.total_usage.input_tokens as i32,
                    b.total_usage.output_tokens as i32,
                )
            })
            .collect();

        let result = ctx
            .run(|| {
                let repos = repos.clone();
                let entries = entries.clone();
                let provider_str = provider_str.clone();
                let model = model.clone();
                let usage_records = usage_records.clone();
                async move {
                    // 1. Enqueue for embeddings.
                    if !entries.is_empty() {
                        repos
                            .reasoning
                            .bulk_enqueue_embeddings(&entries)
                            .await
                            .map_err(terminal_err("enqueue embeddings"))?;
                    }

                    // 2. Log usage for each batch with non-zero tokens.
                    for (input_tokens, output_tokens) in &usage_records {
                        repos
                            .reasoning
                            .log_api_usage(
                                &provider_str,
                                &model,
                                "enrichment",
                                *input_tokens,
                                *output_tokens,
                            )
                            .await
                            .map_err(terminal_err("log usage"))?;
                    }

                    // 3. Clean up fully enriched queue entries.
                    repos
                        .reasoning
                        .delete_fully_enriched_entries()
                        .await
                        .map_err(terminal_err("cleanup"))?;

                    Ok(Json::from(()))
                }
            })
            .name(format!("commit_{iteration}"))
            .await;

        if let Err(e) = result {
            warn!(error = %e, "failed to commit enrichment iteration");
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
                        .map_err(terminal_err("db error"))?;
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

/// Process enrichment batches concurrently (free function for journaling).
///
/// Extracted from `EnrichmentHandlerImpl::process_batches` so it can be
/// called inside `journaled_value!` (which requires cloneable captures,
/// not `&self` references).
async fn process_batches_inner(
    router: &Arc<RwLock<TaskRouter>>,
    repo: &ps_core::repo::ReasoningRepo,
    batches: &[(EnrichmentType, Vec<QueuedContribution>)],
) -> Vec<enrichment::BatchResult> {
    let router = router.read().await;
    let futures: Vec<_> = batches
        .iter()
        .filter(|(_, contributions)| !contributions.is_empty())
        .map(|(etype, contributions)| {
            enrichment::process_queued_enrichment_batch(&router, repo, *etype, contributions)
        })
        .collect();
    futures::future::join_all(futures).await
}
