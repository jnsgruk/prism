use ps_core::models::{EnrichmentType, TaskType};
use ps_core::repo::reasoning::{EmbeddingQueueEntry, QueuedContribution};
use ps_reasoning::features::enrichment;
use restate_sdk::prelude::*;
use tracing::{debug, warn};
use uuid::Uuid;

use super::{EnrichmentHandlerImpl, EnrichmentProgress, MAX_BATCH_SIZE};
use crate::infra::run_lifecycle::{journaled_value, terminal_err};

impl EnrichmentHandlerImpl {
    pub(super) async fn find_queued(
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
    pub(super) async fn commit_iteration(
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

    pub(super) async fn delete_fully_enriched(&self, ctx: &Context<'_>, cleanup_counter: u32) {
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
    pub(super) async fn update_progress(
        &self,
        run_id: Uuid,
        items: i32,
        progress: &EnrichmentProgress,
    ) {
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
