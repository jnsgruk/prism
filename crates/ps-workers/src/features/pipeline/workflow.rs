use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::features::identity_resolution::handler::IdentityResolutionHandlerClient;
use crate::features::ingestion::discourse::handler::DiscourseIngestionHandlerClient;
use crate::features::ingestion::github::handler::GithubIngestionHandlerClient;
use crate::features::ingestion::github::team_sync::GithubTeamSyncHandlerClient;
use crate::features::ingestion::jira::handler::JiraIngestionHandlerClient;
use crate::features::metrics::handler::MetricsComputeHandlerClient;
use crate::features::reasoning::embedding::EmbeddingHandlerClient;
use crate::features::reasoning::enrichment::EnrichmentHandlerClient;
use crate::features::reasoning::insights::InsightsHandlerClient;
use crate::infra::SharedState;
use crate::infra::run_lifecycle::terminal_err;

use super::stages::{
    HandlerResult, StageStatus, build_initial_stages, derive_pipeline_status,
    mark_remaining_cancelled, mark_stage_complete, mark_stage_running,
};

pub struct IngestionPipelineWorkflowImpl {
    pub state: SharedState,
}

/// Result returned from the pipeline workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineResult {
    pub pipeline_id: String,
    pub status: String,
}

/// Status response for `get_status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStatus {
    pub current_stage: Option<String>,
    pub stages: serde_json::Value,
}

#[restate_sdk::workflow]
pub trait IngestionPipelineWorkflow {
    /// Run the full pipeline: team sync → ingestion → [metrics → enrichment →
    /// embedding → insights] + [identity resolution].
    async fn run() -> Result<Json<PipelineResult>, TerminalError>;

    /// Query current pipeline progress (callable while `run()` is executing).
    #[shared]
    async fn get_status() -> Result<Json<PipelineStatus>, TerminalError>;

    /// Signal the pipeline to cancel after the current stage completes.
    #[shared]
    async fn cancel() -> Result<(), TerminalError>;
}

impl IngestionPipelineWorkflow for IngestionPipelineWorkflowImpl {
    async fn run(&self, ctx: WorkflowContext<'_>) -> Result<Json<PipelineResult>, TerminalError> {
        let pipeline_id_str = ctx.key().to_string();
        let pipeline_id: Uuid = pipeline_id_str
            .parse()
            .map_err(terminal_err("invalid pipeline ID"))?;

        info!(pipeline_id = %pipeline_id, "starting pipeline");

        // 1. Create pipeline record in DB (journaled)
        let repos = self.state.repos.clone();
        let invocation_id = ctx.invocation_id().to_string();
        ctx.run(move || {
            let repos = repos.clone();
            let invocation_id = invocation_id.clone();
            async move {
                repos
                    .activity
                    .create_pipeline(pipeline_id, Some(&invocation_id))
                    .await
                    .map_err(terminal_err("failed to create pipeline record"))?;
                Ok(Json::from(()))
            }
        })
        .name("create_pipeline")
        .await?;

        // 2. Load all enabled sources (journaled)
        let repos = self.state.repos.clone();
        let sources: Vec<SourceInfo> = ctx
            .run(move || {
                let repos = repos.clone();
                async move {
                    let configs = repos
                        .config
                        .list_sources()
                        .await
                        .map_err(terminal_err("failed to load sources"))?;
                    Ok(Json::from(
                        configs
                            .into_iter()
                            .filter(|c| c.enabled)
                            .map(|c| {
                                let source_type_str = c.source_type.to_string();
                                SourceInfo {
                                    name: c.name,
                                    source_type: source_type_str,
                                }
                            })
                            .collect::<Vec<_>>(),
                    ))
                }
            })
            .name("load_sources")
            .await?
            .into_inner();

        let has_github = sources.iter().any(|s| s.source_type == "github");
        let has_discourse = sources
            .iter()
            .any(|s| s.source_type.starts_with("discourse"));

        // 3. Build initial stages structure
        let handler_names = build_handler_list(&sources, has_github, has_discourse);
        let mut stages = build_initial_stages(has_github, has_discourse, &handler_names);

        // Set initial K/V state
        ctx.set("stages", Json(stages.clone()));
        ctx.set("current_stage", "initializing".to_string());

        self.persist_stage(&ctx, pipeline_id, "initializing", &stages)
            .await?;

        // 4. STAGE: Team Sync (conditional)
        if has_github {
            self.run_team_sync(pipeline_id, &mut stages, &sources, &ctx)
                .await?;
            if self.is_cancelled(&ctx).await {
                return self
                    .finalize_cancelled(pipeline_id, &mut stages, &ctx)
                    .await;
            }
        }

        // 5. STAGE: Ingestion (fan-out)
        let all_failed = self
            .run_ingestion(pipeline_id, &mut stages, &sources, &ctx)
            .await?;
        if all_failed {
            mark_remaining_cancelled(&mut stages);
            return self
                .finalize(
                    pipeline_id,
                    &mut stages,
                    &ctx,
                    "failed",
                    Some("all ingestion handlers failed"),
                )
                .await;
        }
        if self.is_cancelled(&ctx).await {
            return self
                .finalize_cancelled(pipeline_id, &mut stages, &ctx)
                .await;
        }

        // 6. FORK: two concurrent branches after ingestion
        // Note: we run these sequentially because both branches mutate `stages`.
        // The identity resolution branch is independent and fast enough that
        // sequential execution is acceptable.
        let main_result = self.run_main_branch(pipeline_id, &mut stages, &ctx).await;
        let identity_result = if has_discourse {
            self.run_identity_branch(pipeline_id, &mut stages, &ctx)
                .await
        } else {
            Ok(())
        };

        // 7. Finalize
        let status = derive_pipeline_status(&stages);
        let error_msg = match (&main_result, &identity_result) {
            (Err(e), _) | (_, Err(e)) => Some(e.to_string()),
            _ => None,
        };

        self.finalize(pipeline_id, &mut stages, &ctx, status, error_msg.as_deref())
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
    /// Run the team sync stage for all GitHub sources.
    async fn run_team_sync(
        &self,
        pipeline_id: Uuid,
        stages: &mut serde_json::Value,
        sources: &[SourceInfo],
        ctx: &WorkflowContext<'_>,
    ) -> Result<(), TerminalError> {
        mark_stage_running(stages, "team_sync");
        self.update_state(ctx, pipeline_id, "team_sync", stages)
            .await?;

        let mut results = Vec::new();
        for source in sources.iter().filter(|s| s.source_type == "github") {
            let result = ctx
                .object_client::<GithubTeamSyncHandlerClient>(&source.source_type)
                .sync_teams()
                .call()
                .await;
            results.push(call_result(format!("{} Team Sync", source.name), &result));
        }

        mark_stage_complete(stages, "team_sync", &results);
        self.update_state(ctx, pipeline_id, "team_sync", stages)
            .await?;

        if results.iter().any(|r| r.status == StageStatus::Failed) {
            warn!("team sync had failures, continuing with existing team data");
        }
        Ok(())
    }

    /// Run ingestion for all sources. Returns `true` if all handlers failed.
    async fn run_ingestion(
        &self,
        pipeline_id: Uuid,
        stages: &mut serde_json::Value,
        sources: &[SourceInfo],
        ctx: &WorkflowContext<'_>,
    ) -> Result<bool, TerminalError> {
        mark_stage_running(stages, "ingestion");
        self.update_state(ctx, pipeline_id, "ingestion", stages)
            .await?;

        let mut results = Vec::new();
        for source in sources {
            let result = match source.source_type.as_str() {
                "github" => {
                    ctx.object_client::<GithubIngestionHandlerClient>(&source.source_type)
                        .run_ingestion()
                        .call()
                        .await
                }
                "jira" => {
                    ctx.object_client::<JiraIngestionHandlerClient>(&source.source_type)
                        .run_ingestion()
                        .call()
                        .await
                }
                p if p.starts_with("discourse") => {
                    ctx.object_client::<DiscourseIngestionHandlerClient>(&source.source_type)
                        .run_ingestion()
                        .call()
                        .await
                }
                _ => {
                    warn!(source_type = %source.source_type, "unknown platform, skipping");
                    continue;
                }
            };
            results.push(call_result(source.name.clone(), &result));
        }

        mark_stage_complete(stages, "ingestion", &results);
        self.update_state(ctx, pipeline_id, "ingestion", stages)
            .await?;

        let all_failed =
            !results.is_empty() && results.iter().all(|r| r.status == StageStatus::Failed);
        Ok(all_failed)
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
        ctx.run(move || {
            let repos = repos.clone();
            let stage = stage.clone();
            let stages_clone = stages_clone.clone();
            async move {
                repos
                    .activity
                    .update_pipeline_stage(pipeline_id, &stage, &stages_clone)
                    .await
                    .map_err(terminal_err("failed to persist stage update"))?;
                Ok(Json::from(()))
            }
        })
        .name("update_stage")
        .await?;
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

    /// Run the identity resolution branch.
    async fn run_identity_branch(
        &self,
        pipeline_id: Uuid,
        stages: &mut serde_json::Value,
        ctx: &WorkflowContext<'_>,
    ) -> Result<(), TerminalError> {
        mark_stage_running(stages, "identity_resolution");
        self.update_state(ctx, pipeline_id, "identity_resolution", stages)
            .await?;

        let result = ctx
            .service_client::<IdentityResolutionHandlerClient>()
            .resolve_identities()
            .call()
            .await;

        let handler_result = call_result("Identity Resolution".to_string(), &result);
        mark_stage_complete(stages, "identity_resolution", &[handler_result]);
        self.update_state(ctx, pipeline_id, "identity_resolution", stages)
            .await?;

        result
    }

    /// Finalize the pipeline with a given status.
    async fn finalize(
        &self,
        pipeline_id: Uuid,
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
        ctx.run(move || {
            let repos = repos.clone();
            let status_owned = status_owned.clone();
            let stages_clone = stages_clone.clone();
            let error_owned = error_owned.clone();
            async move {
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
                Ok(Json::from(()))
            }
        })
        .name("finalize_pipeline")
        .await?;

        info!(pipeline_id = %pipeline_id, status = status, "pipeline completed");

        Ok(Json(PipelineResult {
            pipeline_id: pipeline_id.to_string(),
            status: status.to_string(),
        }))
    }

    /// Finalize as cancelled.
    async fn finalize_cancelled(
        &self,
        pipeline_id: Uuid,
        stages: &mut serde_json::Value,
        ctx: &WorkflowContext<'_>,
    ) -> Result<Json<PipelineResult>, TerminalError> {
        mark_remaining_cancelled(stages);
        self.finalize(pipeline_id, stages, ctx, "cancelled", None)
            .await
    }
}

/// Convert a handler call result into a `HandlerResult`.
fn call_result(name: String, result: &Result<(), TerminalError>) -> HandlerResult {
    HandlerResult {
        name,
        status: if result.is_ok() {
            StageStatus::Completed
        } else {
            StageStatus::Failed
        },
        items: None,
        error: result.as_ref().err().map(ToString::to_string),
    }
}

/// Lightweight source info that can be journaled.
/// `source_type` is the `Platform::to_string()` value (e.g. `"github"`, `"discourse_ubuntu"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SourceInfo {
    name: String,
    source_type: String,
}

/// Build the handler name list for initial stages JSONB.
fn build_handler_list(
    sources: &[SourceInfo],
    has_github: bool,
    has_discourse: bool,
) -> Vec<(&'static str, Vec<String>)> {
    let mut list = Vec::new();

    if has_github {
        let github_names: Vec<String> = sources
            .iter()
            .filter(|s| s.source_type == "github")
            .map(|s| format!("{} Team Sync", s.name))
            .collect();
        list.push(("team_sync", github_names));
    }

    let ingestion_names: Vec<String> = sources.iter().map(|s| s.name.clone()).collect();
    list.push(("ingestion", ingestion_names));

    list.push(("metrics", vec!["Metrics".into()]));
    list.push(("enrichment", vec!["Enrichment".into()]));
    list.push(("embedding", vec!["Embedding".into()]));
    list.push(("insights", vec!["Insights".into()]));

    if has_discourse {
        list.push(("identity_resolution", vec!["Identity Resolution".into()]));
    }

    list
}
