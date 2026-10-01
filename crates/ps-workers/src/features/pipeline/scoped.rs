//! Versioned workflow: durable admission snapshots and exact invocation ownership.
//! Legacy workflow journals are intentionally left untouched.
mod lifecycle;
use lifecycle::{PipelineCompletion, owned_call_result};

use std::pin::Pin;

use futures::future::join_all;
use ps_core::ingestion::{PipelineRequest, PipelineScope, SourceRunContext};
use ps_core::models::Platform;
use restate_sdk::prelude::*;
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
    create_owned_run, ensure_owned_active, journaled, journaled_value, register_owned_invocation,
    terminal_err,
};

use super::ownership::{OwnedCycleArgs, OwnedProcessingRequest};
use super::stages::{
    PipelineResult, PipelineStatus, SourceInfo, StageStatus, build_handler_list,
    build_initial_stages, call_result, derive_pipeline_status, mark_remaining_cancelled,
    mark_stage_complete, mark_stage_running,
};

pub struct ScopedIngestionPipelineWorkflowImpl {
    pub state: SharedState,
}

#[restate_sdk::workflow]
pub trait ScopedIngestionPipelineWorkflow {
    async fn run(request: Json<PipelineRequest>) -> Result<Json<PipelineResult>, TerminalError>;

    #[shared]
    async fn get_status() -> Result<Json<PipelineStatus>, TerminalError>;

    #[shared]
    async fn cancel() -> Result<(), TerminalError>;
}

impl ScopedIngestionPipelineWorkflow for ScopedIngestionPipelineWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        Json(request): Json<PipelineRequest>,
    ) -> Result<Json<PipelineResult>, TerminalError> {
        let pipeline_id: Uuid = ctx
            .key()
            .parse()
            .map_err(terminal_err("invalid pipeline ID"))?;
        ctx.set("root_invocation_id", ctx.invocation_id().to_string());
        request
            .validate()
            .map_err(terminal_err("invalid pipeline snapshot"))?;
        if !matches!(request.scope, PipelineScope::All) {
            return Err(TerminalError::new(
                "person pipeline adapters and processing are not available",
            ));
        }
        let repos = self.state.repos.clone();
        let invocation_id = ctx.invocation_id().to_string();
        journaled!(ctx, "start_reserved_pipeline", [repos, invocation_id], {
            repos
                .activity
                .create_pipeline(pipeline_id, Some(&invocation_id))
                .await
                .map_err(terminal_err("failed to start reserved pipeline"))?;
        });
        let owner = OwnedProcessingRequest {
            pipeline_id,
            request,
        };
        let has_discourse = owner
            .request
            .sources
            .iter()
            .any(|s| s.platform.is_discourse());
        let sources: Vec<SourceInfo> = owner
            .request
            .sources
            .iter()
            .map(|s| SourceInfo {
                name: s.source_name.clone(),
                source_type: s.platform.to_string(),
            })
            .collect();
        let handlers = build_handler_list(&sources, has_discourse);
        let mut stages = build_initial_stages(has_discourse, &handlers);
        if ensure_owned_active!(ctx, self.state.repos, owner.pipeline_id).is_err() {
            mark_remaining_cancelled(&mut stages);
            return self
                .finalize(
                    &ctx,
                    PipelineCompletion {
                        pipeline_id,
                        run_id: None,
                        stages: &stages,
                        status: "cancelled",
                        error: None,
                    },
                )
                .await;
        }
        let created = async {
            Ok::<_, TerminalError>(create_owned_run!(
                ctx,
                self.state.repos,
                pipeline_id,
                "_pipeline",
                "ScopedIngestionPipelineWorkflow",
                "run"
            ))
        }
        .await;
        let run_id = match created {
            Ok(run_id) => run_id,
            Err(error) => {
                tracing::error!(%pipeline_id,error=%error,"owned pipeline initialization failed");
                let status = if ensure_owned_active!(ctx, self.state.repos, pipeline_id).is_err() {
                    "cancelled"
                } else {
                    "failed"
                };
                mark_remaining_cancelled(&mut stages);
                return self
                    .finalize(
                        &ctx,
                        PipelineCompletion {
                            pipeline_id,
                            run_id: None,
                            stages: &stages,
                            status,
                            error: Some("Pipeline initialization failed"),
                        },
                    )
                    .await;
            }
        };
        let result = self.execute(&ctx, &owner, &mut stages).await;
        if let Err(ref error) = result {
            tracing::warn!(%pipeline_id,error=%error,"owned pipeline processing failed");
        }
        let repos = self.state.repos.clone();
        let cancelled = journaled_value!(ctx, "pipeline_cancelled", [repos], {
            repos
                .activity
                .pipeline_cancel_requested(pipeline_id)
                .await
                .map_err(terminal_err("failed to read cancellation"))?
        });
        if cancelled || result.is_err() {
            mark_remaining_cancelled(&mut stages);
        }
        let status = if cancelled {
            "cancelled"
        } else if result.is_err() {
            "failed"
        } else {
            derive_pipeline_status(&stages)
        };
        self.finalize(
            &ctx,
            PipelineCompletion {
                pipeline_id,
                run_id: Some(run_id),
                stages: &stages,
                status,
                error: result.err().map(|_| "Pipeline processing failed"),
            },
        )
        .await
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
    ) -> Result<Json<PipelineStatus>, TerminalError> {
        Ok(Json(PipelineStatus {
            current_stage: ctx.get::<String>("current_stage").await?,
            stages: ctx
                .get::<Json<serde_json::Value>>("stages")
                .await?
                .map(Json::into_inner)
                .unwrap_or_default(),
        }))
    }

    async fn cancel(&self, ctx: SharedWorkflowContext<'_>) -> Result<(), TerminalError> {
        let pipeline_id: Uuid = ctx
            .key()
            .parse()
            .map_err(terminal_err("invalid pipeline ID"))?;
        let repos = self.state.repos.clone();
        journaled!(ctx, "request_owned_cancellation", [repos], {
            repos
                .activity
                .request_pipeline_cancel(pipeline_id)
                .await
                .map_err(terminal_err("failed to request cancellation"))?;
        });
        let root = ctx.get::<String>("root_invocation_id").await?;
        let repos = self.state.repos.clone();
        let ids: Vec<String> = journaled_value!(ctx, "load_owned_invocations", [repos], {
            repos
                .activity
                .list_pipeline_invocation_ids(pipeline_id)
                .await
                .map_err(terminal_err("failed to load owned invocations"))?
        });
        for id in ids {
            if root.as_deref() != Some(id.as_str()) {
                ctx.invocation_handle(id).cancel();
            }
        }
        ctx.resolve_promise::<()>("cancel", ());
        Ok(())
    }
}

impl ScopedIngestionPipelineWorkflowImpl {
    async fn execute(
        &self,
        ctx: &WorkflowContext<'_>,
        owner: &OwnedProcessingRequest,
        stages: &mut serde_json::Value,
    ) -> Result<(), TerminalError> {
        self.ingest(ctx, owner, stages).await?;
        for (stage, name) in [
            ("identity_resolution", "Identity Resolution"),
            ("metrics", "Metrics"),
            ("enrichment", "Enrichment"),
            ("embedding", "Embedding"),
            ("insights", "Insights"),
        ] {
            if stages.get(stage).is_none() {
                continue;
            }
            owner.validate_supported()?;
            ensure_owned_active!(ctx, self.state.repos, owner.pipeline_id)?;
            mark_stage_running(stages, stage);
            self.persist(ctx, owner.pipeline_id, stage, stages).await?;
            let result = self.process_stage(ctx, owner, stage).await;
            mark_stage_complete(stages, stage, &[owned_call_result(name.into(), &result)]);
            self.persist(ctx, owner.pipeline_id, stage, stages).await?;
            result?;
        }
        Ok(())
    }

    async fn ingest(
        &self,
        ctx: &WorkflowContext<'_>,
        owner: &OwnedProcessingRequest,
        stages: &mut serde_json::Value,
    ) -> Result<(), TerminalError> {
        type Call<'a> = Pin<Box<dyn Future<Output = Result<(), TerminalError>> + Send + 'a>>;
        mark_stage_running(stages, "ingestion");
        self.persist(ctx, owner.pipeline_id, "ingestion", stages)
            .await?;
        let mut calls: Vec<Call<'_>> = Vec::new();
        let mut names = Vec::new();
        for source in &owner.request.sources {
            owner.validate_supported()?;
            ensure_owned_active!(ctx, self.state.repos, owner.pipeline_id)?;
            let request = SourceRunContext {
                pipeline_id: owner.pipeline_id,
                scope: owner.request.scope.clone(),
                source: source.clone(),
                since_date: owner.request.since_date.clone(),
                run_started_at: owner.request.run_started_at,
                processing: owner.request.processing.clone(),
            };
            // Dispatch and register in snapshot order; future polling cannot change journal order.
            let key = source.platform.to_string();
            let handle = match &source.platform {
                Platform::Github => {
                    ctx.object_client::<GithubIngestionHandlerClient>(&key)
                        .run_scoped(Json(request))
                        .send()
                        .await?
                }
                Platform::Jira => {
                    ctx.object_client::<JiraIngestionHandlerClient>(&key)
                        .run_scoped(Json(request))
                        .send()
                        .await?
                }
                Platform::Discourse(_) => {
                    ctx.object_client::<DiscourseIngestionHandlerClient>(&key)
                        .run_scoped(Json(request))
                        .send()
                        .await?
                }
                _ => {
                    return Err(TerminalError::new(
                        "selected source has no ingestion adapter",
                    ));
                }
            };
            register_owned_invocation!(
                ctx,
                self.state.repos,
                owner.pipeline_id,
                handle,
                "coordinator",
                None::<Uuid>
            );
            calls.push(Box::pin(handle.attach::<()>()));
            names.push(source.source_name.clone());
        }
        let outcomes = join_all(calls).await;
        let results: Vec<_> = names
            .into_iter()
            .zip(outcomes.iter())
            .map(|(name, result)| owned_call_result(name, result))
            .collect();
        mark_stage_complete(stages, "ingestion", &results);
        self.persist(ctx, owner.pipeline_id, "ingestion", stages)
            .await?;
        if results.iter().any(|r| r.status == StageStatus::Failed) {
            return Err(TerminalError::new("source ingestion failed"));
        }
        Ok(())
    }

    async fn process_stage(
        &self,
        ctx: &WorkflowContext<'_>,
        owner: &OwnedProcessingRequest,
        stage: &str,
    ) -> Result<(), TerminalError> {
        let request = Json(owner.clone());
        let handle = match stage {
            "metrics" => {
                ctx.service_client::<MetricsComputeHandlerClient>()
                    .compute_scoped(request)
                    .send()
                    .await?
            }
            "insights" => {
                ctx.service_client::<InsightsHandlerClient>()
                    .compute_scoped(request)
                    .send()
                    .await?
            }
            "identity_resolution" => {
                ctx.service_client::<IdentityResolutionHandlerClient>()
                    .resolve_scoped(request)
                    .send()
                    .await?
            }
            "enrichment" | "embedding" => {
                let (awakeable_id, completion) = ctx.awakeable::<()>();
                let args = Json(OwnedCycleArgs {
                    owner: owner.clone(),
                    parent_run_id: None,
                    completion_awakeable: Some(awakeable_id.clone()),
                });
                let handle = if stage == "enrichment" {
                    ctx.service_client::<EnrichmentHandlerClient>()
                        .run_scoped(args)
                        .send()
                        .await?
                } else {
                    ctx.service_client::<EmbeddingHandlerClient>()
                        .run_scoped(args)
                        .send()
                        .await?
                };
                register_owned_invocation!(
                    ctx,
                    self.state.repos,
                    owner.pipeline_id,
                    handle,
                    stage,
                    None::<Uuid>
                );
                handle.attach::<()>().await?;
                restate_sdk::select! {
                    result = completion => result,
                    _ = ctx.promise::<()>("cancel") => Err(TerminalError::new("pipeline cancelled")),
                }?;
                return Ok(());
            }
            _ => return Err(TerminalError::new("unknown pipeline stage")),
        };
        register_owned_invocation!(
            ctx,
            self.state.repos,
            owner.pipeline_id,
            handle,
            stage,
            None::<Uuid>
        );
        handle.attach::<()>().await
    }
}
