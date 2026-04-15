use std::pin::Pin;

use futures::future::join_all;
use restate_sdk::prelude::*;
use tracing::{info, warn};
use uuid::Uuid;

use crate::features::identity_resolution::handler::IdentityResolutionHandlerClient;
use crate::features::ingestion::discourse::handler::DiscourseIngestionHandlerClient;
use crate::features::ingestion::github::handler::GithubIngestionHandlerClient;

use crate::features::ingestion::jira::handler::JiraIngestionHandlerClient;
use crate::features::metrics::handler::MetricsComputeHandlerClient;
use crate::features::reasoning::embedding::EmbeddingHandlerClient;
use crate::features::reasoning::enrichment::EnrichmentHandlerClient;
use crate::features::reasoning::insights::InsightsHandlerClient;
use crate::infra::SharedState;
use crate::infra::run_lifecycle::{
    complete_run, create_run, fail_run, journaled, journaled_value, terminal_err,
};

use super::stages::{
    HandlerResult, PipelineResult, PipelineStatus, SourceInfo, StageStatus, build_handler_list,
    build_initial_stages, call_result, derive_pipeline_status, mark_remaining_cancelled,
    mark_stage_complete, mark_stage_running,
};

pub struct IngestionPipelineWorkflowImpl {
    pub state: SharedState,
}

#[restate_sdk::workflow]
pub trait IngestionPipelineWorkflow {
    /// Run the full pipeline: ingestion → [metrics → enrichment →
    /// embedding → insights] + [identity resolution].
    ///
    /// When `since_date` is provided (YYYY-MM-DD), the ingestion stage
    /// calls `backfill(since_date)` on each handler instead of
    /// `run_ingestion()`, re-fetching data from the specified date.
    async fn run(since_date: Option<String>) -> Result<Json<PipelineResult>, TerminalError>;

    /// Query current pipeline progress (callable while `run()` is executing).
    #[shared]
    async fn get_status() -> Result<Json<PipelineStatus>, TerminalError>;

    /// Signal the pipeline to cancel after the current stage completes.
    #[shared]
    async fn cancel() -> Result<(), TerminalError>;
}

impl IngestionPipelineWorkflow for IngestionPipelineWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        since_date: Option<String>,
    ) -> Result<Json<PipelineResult>, TerminalError> {
        let pipeline_id_str = ctx.key().to_string();
        let pipeline_id: Uuid = pipeline_id_str
            .parse()
            .map_err(terminal_err("invalid pipeline ID"))?;

        // Create a run record so the pipeline appears in handler runs UI
        let run_id = create_run!(
            ctx,
            self.state.repos,
            "_pipeline",
            "IngestionPipelineWorkflow",
            "run"
        )?;

        let span = tracing::info_span!(
            "handler",
            handler = "IngestionPipelineWorkflow",
            run_id = %run_id,
            pipeline_id = %pipeline_id,
        );
        let _guard = span.enter();

        info!("starting pipeline");

        // 1. Create pipeline record in DB (journaled)
        let repos = self.state.repos.clone();
        let invocation_id = ctx.invocation_id().to_string();
        journaled!(ctx, "create_pipeline", [repos, invocation_id], {
            repos
                .activity
                .create_pipeline(pipeline_id, Some(&invocation_id))
                .await
                .map_err(terminal_err("failed to create pipeline record"))?;
        });

        // 2. Load all enabled sources (journaled)
        let repos = self.state.repos.clone();
        let sources: Vec<SourceInfo> = journaled_value!(ctx, "load_sources", [repos], {
            repos
                .config
                .list_sources()
                .await
                .map_err(terminal_err("failed to load sources"))?
                .into_iter()
                .filter(|c| c.enabled)
                .map(|c| {
                    let source_type_str = c.source_type.to_string();
                    SourceInfo {
                        name: c.name,
                        source_type: source_type_str,
                    }
                })
                .collect::<Vec<_>>()
        });

        let has_discourse = sources
            .iter()
            .any(|s| s.source_type.starts_with("discourse"));

        // 3. Build initial stages structure
        let handler_names = build_handler_list(&sources, has_discourse);
        let mut stages = build_initial_stages(has_discourse, &handler_names);

        // Set initial K/V state
        ctx.set("stages", Json(stages.clone()));
        ctx.set("current_stage", "initializing".to_string());

        self.persist_stage(&ctx, pipeline_id, "initializing", &stages)
            .await?;

        // 4. STAGE: Ingestion (fan-out)
        let (any_failed, failed_names) = self
            .run_ingestion(
                pipeline_id,
                &mut stages,
                &sources,
                &ctx,
                since_date.as_deref(),
            )
            .await?;
        if any_failed {
            mark_remaining_cancelled(&mut stages);
            let error_msg = format!("ingestion failed for: {}", failed_names.join(", "));
            return self
                .finalize(
                    pipeline_id,
                    run_id,
                    &mut stages,
                    &ctx,
                    "failed",
                    Some(&error_msg),
                )
                .await;
        }
        if self.is_cancelled(&ctx).await {
            return self
                .finalize_cancelled(pipeline_id, run_id, &mut stages, &ctx)
                .await;
        }

        // 5. Dispatch identity resolution as fire-and-forget (if applicable).
        //
        // IMPORTANT: This must use `.send()` not `.call()` to avoid a
        // `futures::join` with the main branch.  `join` polls both futures
        // concurrently and the order in which their `.call()` journal entries
        // are recorded depends on poll order — which is non-deterministic
        // across executions, causing Restate error 570 on replay.
        //
        // Identity resolution has its own run record for observability;
        // we mark the stage completed optimistically here.
        if has_discourse {
            mark_stage_running(&mut stages, "identity_resolution");
            self.update_state(&ctx, pipeline_id, "identity_resolution", &stages)
                .await?;

            ctx.service_client::<IdentityResolutionHandlerClient>()
                .resolve_identities()
                .send();

            let handler_result = HandlerResult {
                name: "Identity Resolution".to_string(),
                status: StageStatus::Completed,
                items: None,
                error: None,
            };
            mark_stage_complete(&mut stages, "identity_resolution", &[handler_result]);
            self.update_state(&ctx, pipeline_id, "identity_resolution", &stages)
                .await?;
        }

        // 6. Main branch: metrics → enrichment → embedding → insights
        let main_result = self.run_main_branch(pipeline_id, &mut stages, &ctx).await;

        // 7. Finalize
        let status = derive_pipeline_status(&stages);
        let error_msg = if let Err(ref e) = main_result {
            Some(e.to_string())
        } else {
            None
        };

        self.finalize(
            pipeline_id,
            run_id,
            &mut stages,
            &ctx,
            status,
            error_msg.as_deref(),
        )
        .await
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
    ) -> Result<Json<PipelineStatus>, TerminalError> {
        let current_stage = ctx.get::<String>("current_stage").await?;
        let stages = ctx
            .get::<Json<serde_json::Value>>("stages")
            .await?
            .map(Json::into_inner)
            .unwrap_or_default();

        Ok(Json(PipelineStatus {
            current_stage,
            stages,
        }))
    }

    async fn cancel(&self, ctx: SharedWorkflowContext<'_>) -> Result<(), TerminalError> {
        info!(pipeline_id = %ctx.key(), "cancel requested");
        ctx.resolve_promise::<()>("cancel", ());
        Ok(())
    }
}

impl IngestionPipelineWorkflowImpl {
    /// Run ingestion for all sources concurrently. Returns `(any_failed, failed_names)`.
    ///
    /// All handlers are dispatched at once so each creates its own run record
    /// immediately — source rows show "collecting" in the UI, consistent with
    /// how individual triggers work.
    ///
    /// When `since_date` is provided, calls `backfill(since_date)` on each
    /// handler instead of `run_ingestion()`.
    async fn run_ingestion(
        &self,
        pipeline_id: Uuid,
        stages: &mut serde_json::Value,
        sources: &[SourceInfo],
        ctx: &WorkflowContext<'_>,
        since_date: Option<&str>,
    ) -> Result<(bool, Vec<String>), TerminalError> {
        type CallFut<'a> = Pin<Box<dyn Future<Output = Result<(), TerminalError>> + Send + 'a>>;

        mark_stage_running(stages, "ingestion");
        self.update_state(ctx, pipeline_id, "ingestion", stages)
            .await?;

        let mut calls: Vec<CallFut<'_>> = Vec::new();
        let mut names: Vec<String> = Vec::new();

        for source in sources {
            let call: CallFut<'_> = match (source.source_type.as_str(), since_date) {
                ("github", Some(date)) => Box::pin(
                    ctx.object_client::<GithubIngestionHandlerClient>(&source.source_type)
                        .backfill(date.to_string())
                        .call(),
                ),
                ("github", None) => Box::pin(
                    ctx.object_client::<GithubIngestionHandlerClient>(&source.source_type)
                        .run_ingestion()
                        .call(),
                ),
                ("jira", Some(date)) => Box::pin(
                    ctx.object_client::<JiraIngestionHandlerClient>(&source.source_type)
                        .backfill(date.to_string())
                        .call(),
                ),
                ("jira", None) => Box::pin(
                    ctx.object_client::<JiraIngestionHandlerClient>(&source.source_type)
                        .run_ingestion()
                        .call(),
                ),
                (p, Some(date)) if p.starts_with("discourse") => Box::pin(
                    ctx.object_client::<DiscourseIngestionHandlerClient>(&source.source_type)
                        .backfill(date.to_string())
                        .call(),
                ),
                (p, None) if p.starts_with("discourse") => Box::pin(
                    ctx.object_client::<DiscourseIngestionHandlerClient>(&source.source_type)
                        .run_ingestion()
                        .call(),
                ),
                _ => {
                    warn!(source_type = %source.source_type, "unknown platform, skipping");
                    continue;
                }
            };
            calls.push(call);
            names.push(source.name.clone());
        }

        let outcomes = join_all(calls).await;
        let results: Vec<_> = names
            .into_iter()
            .zip(outcomes.iter())
            .map(|(name, result)| call_result(name, result))
            .collect();

        mark_stage_complete(stages, "ingestion", &results);
        self.update_state(ctx, pipeline_id, "ingestion", stages)
            .await?;

        let failed_names: Vec<String> = results
            .iter()
            .filter(|r| r.status == StageStatus::Failed)
            .map(|r| r.name.clone())
            .collect();
        let any_failed = !failed_names.is_empty();
        Ok((any_failed, failed_names))
    }

    /// Persist stage update to DB (journaled).
    async fn persist_stage(
        &self,
        ctx: &WorkflowContext<'_>,
        pipeline_id: Uuid,
        current_stage: &str,
        stages: &serde_json::Value,
    ) -> Result<(), TerminalError> {
        let repos = self.state.repos.clone();
        let stage = current_stage.to_string();
        let stages_clone = stages.clone();
        journaled!(ctx, "update_stage", [repos, stage, stages_clone], {
            repos
                .activity
                .update_pipeline_stage(pipeline_id, &stage, &stages_clone)
                .await
                .map_err(terminal_err("failed to persist stage update"))?;
        });
        Ok(())
    }

    /// Update both K/V state and DB with the current stage progress.
    async fn update_state(
        &self,
        ctx: &WorkflowContext<'_>,
        pipeline_id: Uuid,
        current_stage: &str,
        stages: &serde_json::Value,
    ) -> Result<(), TerminalError> {
        ctx.set("current_stage", current_stage.to_string());
        ctx.set("stages", Json(stages.clone()));
        self.persist_stage(ctx, pipeline_id, current_stage, stages)
            .await
    }

    /// Non-blocking check if cancellation has been requested.
    async fn is_cancelled(&self, ctx: &WorkflowContext<'_>) -> bool {
        matches!(ctx.peek_promise::<()>("cancel").await, Ok(Some(())))
    }

    /// Run a single sequential stage in the main branch.
    async fn run_stage<F, Fut>(
        &self,
        pipeline_id: Uuid,
        stages: &mut serde_json::Value,
        ctx: &WorkflowContext<'_>,
        stage_name: &str,
        handler_name: &str,
        call_handler: F,
    ) -> Result<bool, TerminalError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), TerminalError>>,
    {
        mark_stage_running(stages, stage_name);
        self.update_state(ctx, pipeline_id, stage_name, stages)
            .await?;

        let result = call_handler().await;
        let handler_result = call_result(handler_name.to_string(), &result);
        mark_stage_complete(stages, stage_name, &[handler_result]);
        self.update_state(ctx, pipeline_id, stage_name, stages)
            .await?;

        if result.is_err() {
            mark_remaining_cancelled(stages);
            return Ok(false);
        }
        if self.is_cancelled(ctx).await {
            return Ok(false);
        }
        Ok(true)
    }

    /// Run the main processing branch: metrics → enrichment → embedding → insights.
    async fn run_main_branch(
        &self,
        pipeline_id: Uuid,
        stages: &mut serde_json::Value,
        ctx: &WorkflowContext<'_>,
    ) -> Result<(), TerminalError> {
        // Metrics
        if !self
            .run_stage(pipeline_id, stages, ctx, "metrics", "Metrics", || {
                ctx.service_client::<MetricsComputeHandlerClient>()
                    .compute_current_periods()
                    .call()
            })
            .await?
        {
            return Ok(());
        }

        // Enrichment
        if !self
            .run_stage(pipeline_id, stages, ctx, "enrichment", "Enrichment", || {
                ctx.service_client::<EnrichmentHandlerClient>()
                    .run_cycle()
                    .call()
            })
            .await?
        {
            return Ok(());
        }

        // Embedding
        if !self
            .run_stage(pipeline_id, stages, ctx, "embedding", "Embedding", || {
                ctx.service_client::<EmbeddingHandlerClient>()
                    .run_cycle()
                    .call()
            })
            .await?
        {
            return Ok(());
        }

        // Insights
        self.run_stage(pipeline_id, stages, ctx, "insights", "Insights", || {
            ctx.service_client::<InsightsHandlerClient>()
                .compute_current_periods()
                .call()
        })
        .await?;

        Ok(())
    }

    /// Finalize the pipeline with a given status.
    async fn finalize(
        &self,
        pipeline_id: Uuid,
        run_id: Uuid,
        stages: &mut serde_json::Value,
        ctx: &WorkflowContext<'_>,
        status: &str,
        error: Option<&str>,
    ) -> Result<Json<PipelineResult>, TerminalError> {
        ctx.set("current_stage", "done".to_string());
        ctx.set("stages", Json(stages.clone()));

        let repos = self.state.repos.clone();
        let status_owned = status.to_string();
        let stages_clone = stages.clone();
        let error_owned = error.map(str::to_string);
        journaled!(
            ctx,
            "finalize_pipeline",
            [repos, status_owned, stages_clone, error_owned],
            {
                repos
                    .activity
                    .complete_pipeline(
                        pipeline_id,
                        &status_owned,
                        &stages_clone,
                        error_owned.as_deref(),
                    )
                    .await
                    .map_err(terminal_err("failed to finalize pipeline"))?;
            }
        );

        // Complete or fail the handler run record
        match status {
            "failed" => {
                let err_msg = error.unwrap_or("pipeline failed");
                fail_run!(ctx, self.state.repos, run_id, "_pipeline", err_msg);
            }
            _ => {
                complete_run!(ctx, self.state.repos, run_id, "_pipeline", 0i32);
            }
        }

        info!(status = status, "pipeline completed");

        Ok(Json(PipelineResult {
            pipeline_id: pipeline_id.to_string(),
            status: status.to_string(),
        }))
    }

    /// Finalize as cancelled.
    async fn finalize_cancelled(
        &self,
        pipeline_id: Uuid,
        run_id: Uuid,
        stages: &mut serde_json::Value,
        ctx: &WorkflowContext<'_>,
    ) -> Result<Json<PipelineResult>, TerminalError> {
        mark_remaining_cancelled(stages);
        self.finalize(pipeline_id, run_id, stages, ctx, "cancelled", None)
            .await
    }
}
