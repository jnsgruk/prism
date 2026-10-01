use ps_workers::features::ingestion::github::handler::{
    GithubIngestionHandler, GithubIngestionHandlerImpl,
};
use ps_workers::features::ingestion::jira::handler::{
    JiraIngestionHandler, JiraIngestionHandlerImpl,
};
use ps_workers::features::ingestion::lib::chunk::IngestionChunkServiceImpl;
use ps_workers::features::pipeline::scoped::{
    ScopedIngestionPipelineWorkflow, ScopedIngestionPipelineWorkflowImpl,
};
use ps_workers::features::reasoning::embedding::{EmbeddingHandler, EmbeddingHandlerImpl};
use ps_workers::features::reasoning::enrichment::{EnrichmentHandler, EnrichmentHandlerImpl};
use ps_workers::features::reasoning::insights::{InsightsHandler, InsightsHandlerImpl};
use ps_workers::infra::SharedState;
use restate_sdk::prelude::{Endpoint, HttpServer};
use std::process::Command;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

impl super::RestateTestContext {
    /// Force short durable sleeps to suspend in tests of replay and cancellation.
    pub async fn shorten_chunk_inactivity_timeout(&self) {
        let response = self
            .client
            .patch(format!("{}/services/IngestionChunkService", self.admin))
            .json(&serde_json::json!({"inactivity_timeout": "1s"}))
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "chunk timeout update failed: {}",
            response.text().await.unwrap()
        );
    }
}

pub(super) fn start_worker(
    listener: TcpListener,
    state: SharedState,
    provider_url: String,
) -> (JoinHandle<()>, oneshot::Sender<()>) {
    let (stop, stopped) = oneshot::channel();
    let worker = tokio::spawn(async move {
        let mut router =
            ps_reasoning::routing::TaskRouter::new(ps_reasoning::types::AiConfig::default());
        let key = "fixture-key-no-upstream-calls";
        let client = ps_reasoning::rig::providers::gemini::Client::builder()
            .api_key(key)
            .base_url(provider_url)
            .build()
            .unwrap();
        router.set_google_client(client, key);
        let router = std::sync::Arc::new(tokio::sync::RwLock::new(router));
        let endpoint = Endpoint::builder()
            .bind(
                ScopedIngestionPipelineWorkflowImpl {
                    state: state.clone(),
                }
                .serve(),
            )
            .bind(
                InsightsHandlerImpl {
                    state: state.clone(),
                }
                .serve(),
            )
            .bind(
                EnrichmentHandlerImpl {
                    state: state.clone(),
                    router: router.clone(),
                }
                .serve(),
            )
            .bind(
                EmbeddingHandlerImpl {
                    state: state.clone(),
                    router,
                }
                .serve(),
            )
            .bind(IngestionChunkServiceImpl {
                state: state.clone(),
            })
            .bind(
                JiraIngestionHandlerImpl {
                    state: state.clone(),
                }
                .serve(),
            )
            .bind(
                GithubIngestionHandlerImpl {
                    state: state.clone(),
                }
                .serve(),
            );
        let endpoint = ps_workers::features::metrics::bind(endpoint, &state).build();
        HttpServer::new(endpoint)
            .serve_with_cancel(listener, stopped)
            .await;
    });
    (worker, stop)
}

pub(super) fn docker_url(container: &str, port: &str) -> String {
    let output = Command::new("docker")
        .args(["port", container, port])
        .output()
        .unwrap();
    assert!(output.status.success(), "Docker did not expose {port}");
    format!(
        "http://{}",
        String::from_utf8(output.stdout).unwrap().trim()
    )
}

pub(super) fn embedding_response(request: &wiremock::Request) -> wiremock::ResponseTemplate {
    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    let embeddings: Vec<serde_json::Value> = body["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|_| serde_json::json!({"values":vec![0.25; 768]}))
        .collect();
    wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"embeddings":embeddings}))
}
