//! Atomically finalize the owner and its run records under the admission lock.
use uuid::Uuid;

use crate::Error;

use super::ActivityRepo;

pub struct PipelineFinishParams<'a> {
    pub pipeline_id: Uuid,
    pub status: &'a str,
    pub stages: &'a serde_json::Value,
    pub error: Option<&'a str>,
    pub parent_run_id: Option<Uuid>,
}

impl ActivityRepo {
    pub async fn finish_owned_pipeline(
        &self,
        params: PipelineFinishParams<'_>,
    ) -> Result<String, Error> {
        let mut transaction = self.pool.begin().await?;
        let pipeline = sqlx::query!(
            "SELECT status, cancellation_requested, request_snapshot FROM activity.pipelines WHERE id = $1 FOR UPDATE",
            params.pipeline_id,
        )
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| Error::NotFound("pipeline".into()))?;

        if !matches!(
            pipeline.status.as_str(),
            "pending" | "running" | "cancelling"
        ) {
            return Ok(pipeline.status);
        }

        let effective_status = if pipeline.cancellation_requested {
            "cancelled"
        } else {
            params.status
        };
        let reliable_owner = pipeline.request_snapshot != serde_json::json!({});
        let failure_message = match effective_status {
            "cancelled" => Some("Parent pipeline cancelled"),
            "failed" => Some("Parent pipeline failed"),
            _ => None,
        };

        if let Some(message) = failure_message {
            sqlx::query!(
                r#"
                UPDATE activity.ingestion_runs
                SET completed_at = now(), status = $2, error_message = $3
                WHERE pipeline_id = $1 AND status = 'running' AND completed_at IS NULL
                  AND ($4 OR id IN (SELECT run_id FROM activity.pipeline_invocations WHERE pipeline_id = $1 AND run_id IS NOT NULL))
                "#,
                params.pipeline_id,
                effective_status,
                message,
                reliable_owner,
            )
            .execute(&mut *transaction)
            .await?;
        } else if let Some(run_id) = params.parent_run_id {
            sqlx::query!(
                r#"
                UPDATE activity.ingestion_runs
                SET completed_at = now(), status = 'completed', items_collected = 0
                WHERE id = $1 AND pipeline_id = $2 AND completed_at IS NULL
                  AND ($3 OR id IN (SELECT run_id FROM activity.pipeline_invocations WHERE pipeline_id = $2 AND run_id IS NOT NULL))
                "#,
                run_id,
                params.pipeline_id,
                reliable_owner,
            )
            .execute(&mut *transaction)
            .await?;
        }

        let mut stages = params.stages.clone();
        terminalize_stages(&mut stages, effective_status);
        sqlx::query!(
            r#"
            UPDATE activity.pipelines
            SET completed_at = now(), status = $2, stages = $3,
                current_invocation_id = NULL,
                error = CASE WHEN cancellation_requested THEN NULL ELSE $4 END
            WHERE id = $1
            "#,
            params.pipeline_id,
            effective_status,
            stages,
            params.error,
        )
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;

        Ok(effective_status.to_owned())
    }
}

fn terminalize_stages(stages: &mut serde_json::Value, status: &str) {
    if !matches!(status, "failed" | "cancelled") {
        return;
    }
    let Some(stages) = stages.as_object_mut() else {
        return;
    };
    for stage in stages.values_mut() {
        terminalize_active_status(stage, status);
        if let Some(handlers) = stage
            .get_mut("handlers")
            .and_then(serde_json::Value::as_array_mut)
        {
            for handler in handlers {
                terminalize_active_status(handler, status);
            }
        }
    }
}

fn terminalize_active_status(value: &mut serde_json::Value, status: &str) {
    if value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|status| matches!(status, "pending" | "running"))
    {
        value["status"] = serde_json::json!(status);
    }
}
