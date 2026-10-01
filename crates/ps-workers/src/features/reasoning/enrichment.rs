mod continuation;
use continuation::dispatch_continuation;
mod persistence;
mod progress;

use progress::{CycleState, EnrichmentProgress};
use std::sync::Arc;

use ps_core::models::EnrichmentType;
use ps_core::repo::reasoning::QueuedContribution;
use ps_reasoning::features::enrichment;
use ps_reasoning::routing::TaskRouter;
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::features::pipeline::ownership::{
    CycleRun, OwnedCycleArgs, OwnedProcessingRequest, create_cycle_run,
};
use crate::infra::SharedState;
use crate::infra::run_lifecycle::{
    complete_handler_run, complete_owned_run, complete_run, ensure_owned_active, fail_handler_run,
    fail_owned_run, fail_run, journaled_value, terminal_err,
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

    async fn run_scoped(args: Json<OwnedCycleArgs>) -> Result<(), TerminalError>;
}

impl EnrichmentHandler for EnrichmentHandlerImpl {
    async fn run_scoped(
        &self,
        ctx: Context<'_>,
        Json(args): Json<OwnedCycleArgs>,
    ) -> Result<(), TerminalError> {
        let awakeable = args.completion_awakeable.clone();
        let result = async {
            args.owner.validate_supported()?;
            self.run_enrichment_cycle(
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
        self.run_enrichment_cycle(&ctx, args.into_inner(), None)
            .await
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
        owner: Option<OwnedProcessingRequest>,
    ) -> Result<(), TerminalError> {
        let start = std::time::Instant::now();

        let is_continuation = args.parent_run_id.is_some();
        let run_id = create_cycle_run(
            ctx,
            &self.state,
            CycleRun {
                owner: owner.as_ref(),
                parent_run_id: args.parent_run_id,
                source_name: "_enrichment",
                handler_name: "EnrichmentHandler",
                continuation_kind: "enrichment_continuation",
            },
        )
        .await?;

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
        let mut systemic_failure = false;

        loop {
            if let Some(ref owner) = owner {
                owner.validate_supported()?;
                ensure_owned_active!(ctx, self.state.repos, owner.pipeline_id)?;
            }
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

            let prev_processed = s.total_processed;

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

            // Detect systemic failure: if this iteration made zero progress
            // (no successful enrichments) but encountered errors, continuing
            // will re-fetch the same failing items forever. Break out and
            // report the failure instead of chaining infinite continuations.
            let iteration_errors: usize = results.iter().map(|r| r.errors).sum();
            if s.total_processed == prev_processed && iteration_errors > 0 {
                warn!(
                    errors = iteration_errors,
                    first_error = ?s.first_error,
                    "systemic failure: all items in batch failed, aborting enrichment cycle"
                );
                systemic_failure = true;
                break;
            }

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
            dispatch_continuation(ctx, &self.state, owner, run_id, args.completion_awakeable)
                .await?;
        } else {
            self.delete_fully_enriched(ctx, s.iteration).await;
            self.finalize_run(ctx, run_id, &mut s, start.elapsed(), owner.is_some())
                .await;
            if let Some(awakeable_id) = args.completion_awakeable.as_deref() {
                if systemic_failure {
                    let reason = s
                        .first_error
                        .as_deref()
                        .unwrap_or("all enrichment items failed");
                    ctx.reject_awakeable(
                        awakeable_id,
                        TerminalError::new(format!("enrichment failed: {reason}")),
                    );
                } else {
                    ctx.resolve_awakeable(awakeable_id, ());
                }
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
        owned: bool,
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
            fail_handler_run!(owned, ctx, self.state.repos, run_id, "_enrichment", &msg);
            warn!(errors = s.total_errors, "enrichment cycle failed");
        } else {
            complete_handler_run!(
                owned,
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
