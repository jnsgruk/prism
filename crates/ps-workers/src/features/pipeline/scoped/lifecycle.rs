use super::{PipelineResult, ScopedIngestionPipelineWorkflowImpl, call_result};
use crate::infra::run_lifecycle::{journaled, journaled_value, terminal_err};
use restate_sdk::prelude::*;
use uuid::Uuid;

pub(super) struct PipelineCompletion<'a> {
    pub(super) pipeline_id: Uuid,
    pub(super) run_id: Option<Uuid>,
    pub(super) stages: &'a serde_json::Value,
    pub(super) status: &'a str,
    pub(super) error: Option<&'a str>,
}

impl ScopedIngestionPipelineWorkflowImpl {
    pub(super) async fn persist(
        &self,
        ctx: &WorkflowContext<'_>,
        pipeline_id: Uuid,
        stage: &str,
        stages: &serde_json::Value,
    ) -> Result<(), TerminalError> {
        ctx.set("current_stage", stage.to_string());
        ctx.set("stages", Json(stages.clone()));
        let repos = self.state.repos.clone();
        let stage = stage.to_string();
        let stages = stages.clone();
        journaled!(ctx, "update_owned_stage", [repos, stage, stages], {
            repos
                .activity
                .update_pipeline_stage(pipeline_id, &stage, &stages)
                .await
                .map_err(terminal_err("failed to update pipeline stage"))?;
        });
        Ok(())
    }

    pub(super) async fn finalize(
        &self,
        ctx: &WorkflowContext<'_>,
        completion: PipelineCompletion<'_>,
    ) -> Result<Json<PipelineResult>, TerminalError> {
        let PipelineCompletion {
            pipeline_id,
            run_id,
            stages,
            status,
            error,
        } = completion;
        if status == "failed" {
            let repos = self.state.repos.clone();
            journaled!(ctx, "stop_failed_pipeline", [repos], {
                repos
                    .activity
                    .request_pipeline_stop(pipeline_id)
                    .await
                    .map_err(terminal_err("failed to stop pipeline descendants"))?;
            });
        }
        if matches!(status, "cancelled" | "failed") {
            let repos = self.state.repos.clone();
            let ids: Vec<String> = journaled_value!(ctx, "drain_owned_invocations", [repos], {
                repos
                    .activity
                    .list_pipeline_invocation_ids(pipeline_id)
                    .await
                    .map_err(terminal_err("failed to load owned invocations"))?
            });
            for id in ids {
                if id != ctx.invocation_id() {
                    let handle = ctx.invocation_handle(id);
                    handle.cancel();
                    let _ = handle.attach::<Json<serde_json::Value>>().await;
                }
            }
        }
        self.persist(ctx, pipeline_id, "done", stages).await?;
        let repos = self.state.repos.clone();
        let stages = stages.clone();
        let status = status.to_string();
        let error = error.map(str::to_string);
        let status = journaled_value!(
            ctx,
            "finish_owned_pipeline",
            [repos, stages, status, error],
            {
                repos
                    .activity
                    .finish_owned_pipeline(ps_core::repo::activity::PipelineFinishParams {
                        pipeline_id,
                        status: &status,
                        stages: &stages,
                        error: error.as_deref(),
                        parent_run_id: run_id,
                    })
                    .await
                    .map_err(terminal_err("failed to finish owned pipeline"))?
            }
        );
        Ok(Json(PipelineResult {
            pipeline_id: pipeline_id.to_string(),
            status,
        }))
    }
}

pub(super) fn owned_call_result(
    name: String,
    result: &Result<(), TerminalError>,
) -> crate::features::pipeline::stages::HandlerResult {
    if let Err(error) = result {
        tracing::warn!(handler=%name,error=%error,"owned pipeline child failed");
    }
    let safe_result = result
        .as_ref()
        .copied()
        .map_err(|_| TerminalError::new("Handler processing failed"));
    call_result(name, &safe_result)
}
