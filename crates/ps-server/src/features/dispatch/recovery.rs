use ps_core::models::Pipeline;
use tracing::warn;

use super::HandlersServiceImpl;

impl HandlersServiceImpl {
    /// Recover durable intents on startup and throughout the server lifetime.
    /// Workflows deduplicate by the reserved UUID, including response-loss retries.
    pub fn start_dispatch_recovery(&self) {
        let service = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                if let Err(error) = service.recover_pipeline_dispatch().await {
                    warn!(%error, "pipeline dispatch recovery failed");
                }
            }
        });
    }

    pub async fn recover_pipeline_dispatch(&self) -> Result<(), ps_core::Error> {
        let Some(pipeline) = self.repos.activity.claim_pipeline_dispatch().await? else {
            return Ok(());
        };
        self.deliver_pipeline(&pipeline).await
    }

    async fn deliver_pipeline(&self, pipeline: &Pipeline) -> Result<(), ps_core::Error> {
        let url = format!(
            "{}/ScopedIngestionPipelineWorkflow/{}/run/send",
            self.restate_url, pipeline.id
        );
        let legacy = pipeline.request_snapshot == serde_json::json!({});
        if pipeline.current_invocation_id.is_some()
            && self.reconcile_terminal_pipeline(pipeline).await?
        {
            return Ok(());
        }
        if !legacy && !pipeline.dispatch_acknowledged {
            match self
                .http_client
                .post(&url)
                .json(&pipeline.request_snapshot)
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    if let Ok(body) = response.json::<serde_json::Value>().await
                        && let Some(invocation_id) =
                            body.get("invocationId").and_then(serde_json::Value::as_str)
                        && invocation_id.starts_with("inv_")
                        && super::validate_restate_identifier(invocation_id).is_ok()
                    {
                        self.repos
                            .activity
                            .acknowledge_pipeline_dispatch(pipeline.id, invocation_id)
                            .await?;
                    }
                    // Missing/malformed response means uncertain delivery. Retrying
                    // the same workflow UUID never creates another workflow.
                }
                Ok(response)
                    if pipeline.dispatch_attempts == 1
                        && response.status().is_client_error()
                        && response.status() != reqwest::StatusCode::REQUEST_TIMEOUT
                        && response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS =>
                {
                    warn!(pipeline_id = %pipeline.id, status = %response.status(), "pipeline dispatch definitively rejected");
                    self.repos
                        .activity
                        .complete_pipeline(
                            pipeline.id,
                            "failed",
                            &pipeline.stages,
                            Some("Pipeline submission was rejected"),
                        )
                        .await?;
                    return Ok(());
                }
                Ok(response) => {
                    warn!(pipeline_id = %pipeline.id, status = %response.status(), "pipeline dispatch will retry original intent");
                }
                Err(error) => {
                    warn!(pipeline_id = %pipeline.id, %error, "pipeline delivery uncertain; retrying original intent");
                }
            }
        }
        if self
            .repos
            .activity
            .get_pipeline(pipeline.id)
            .await?
            .is_some_and(|pipeline| pipeline.cancellation_requested)
        {
            let handler = if legacy {
                "IngestionPipelineWorkflow"
            } else {
                "ScopedIngestionPipelineWorkflow"
            };
            let cancel_url = format!("{}/{handler}/{}/cancel/send", self.restate_url, pipeline.id);
            if let Err(error) = self.send_to_restate(&cancel_url, None).await {
                warn!(pipeline_id = %pipeline.id, %error, "pipeline cancellation will retry");
            }
        }
        Ok(())
    }
}
