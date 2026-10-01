//! Ephemeral real Restate runtime for worker-path tests. No cluster or provider
//! credentials are needed; Docker is already required by the PostgreSQL harness.

use std::process::Command;
use std::time::Duration;

use ps_core::repo::Repos;
use ps_workers::features::ingestion::jira::handler::{
    JiraIngestionHandler, JiraIngestionHandlerImpl,
};
use ps_workers::features::ingestion::lib::chunk::{
    ChunkRequest, ChunkResult, IngestionChunkService, IngestionChunkServiceImpl,
};
use ps_workers::infra::SharedState;
use restate_sdk::prelude::{Endpoint, HttpServer};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;
use zeroize::Zeroizing;

pub const TEST_SECRET_KEY: [u8; 32] = [71; 32];
const IMAGE: &str = "docker.io/restatedev/restate:1.6";

pub struct RestateTestContext {
    pub admin: String,
    pub ingress: String,
    pub client: reqwest::Client,
    container: String,
    worker: Option<JoinHandle<()>>,
    stop_worker: Option<oneshot::Sender<()>>,
    worker_port: u16,
    state: SharedState,
}

impl RestateTestContext {
    pub async fn new(repos: Repos) -> Self {
        let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
        let worker_port = listener.local_addr().unwrap().port();
        let state = SharedState {
            repos,
            secret_key: Zeroizing::new(TEST_SECRET_KEY),
            http_client: reqwest::Client::new(),
            container_manager: None,
            workspaces_path: None,
        };
        let (worker, stop_worker) = start_worker(listener, state.clone());
        let container = format!("prism-scoped-test-{}", Uuid::now_v7().simple());
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "--name",
                &container,
                "--add-host=host.docker.internal:host-gateway",
                "-p",
                "127.0.0.1:0:8080",
                "-p",
                "127.0.0.1:0:9070",
                "-e",
                "RESTATE_DEFAULT_NUM_PARTITIONS=1",
                "-e",
                "RESTATE_WORKER__INVOKER__INACTIVITY_TIMEOUT=1s",
                "-e",
                "DO_NOT_TRACK=1",
                IMAGE,
            ])
            .output()
            .expect("start ephemeral Restate container");
        assert!(
            output.status.success(),
            "Restate Docker startup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let context = Self {
            admin: docker_url(&container, "9070/tcp"),
            ingress: docker_url(&container, "8080/tcp"),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
            container,
            worker: Some(worker),
            stop_worker: Some(stop_worker),
            worker_port,
            state,
        };
        context.wait_ready().await;
        let response = context.client.post(format!("{}/deployments", context.admin))
            .json(&serde_json::json!({"uri":format!("http://host.docker.internal:{worker_port}"),"force":true}))
            .send().await.unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        assert!(
            status.is_success(),
            "worker discovery failed: {status} {body}"
        );
        context
    }

    async fn wait_ready(&self) {
        for _ in 0..150 {
            if let Ok(response) = self
                .client
                .get(format!("{}/health", self.admin))
                .send()
                .await
                && response.status().is_success()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let logs = Command::new("docker")
            .args(["logs", &self.container])
            .output()
            .unwrap();
        panic!(
            "Restate never became healthy: {} {}",
            String::from_utf8_lossy(&logs.stdout),
            String::from_utf8_lossy(&logs.stderr)
        );
    }

    pub async fn send_chunk(&self, request: &ChunkRequest) -> String {
        self.send("process_scoped_chunk", request, None).await
    }

    pub async fn send_jira_coordinator(
        &self,
        request: &ps_core::ingestion::SourceRunContext,
    ) -> String {
        let response = self
            .client
            .post(format!(
                "{}/JiraIngestionHandler/{}/run_scoped/send",
                self.ingress, request.source.source_id
            ))
            .json(request)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert!(
            status.is_success(),
            "coordinator send failed: {status} {body}"
        );
        body.get("invocationId")
            .and_then(Value::as_str)
            .unwrap()
            .into()
    }

    pub async fn send(&self, handler: &str, request: &ChunkRequest, delay: Option<&str>) -> String {
        let mut request_builder = self
            .client
            .post(format!(
                "{}/IngestionChunkService/{handler}/send",
                self.ingress
            ))
            .header("idempotency-key", Uuid::now_v7().to_string())
            .json(request);
        if let Some(delay) = delay {
            request_builder = request_builder.query(&[("delay", delay)]);
        }
        let response = request_builder.send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert!(status.is_success(), "chunk send failed: {status} {body}");
        body.get("invocationId")
            .and_then(Value::as_str)
            .unwrap()
            .into()
    }

    pub async fn attach(&self, invocation: &str) -> reqwest::Response {
        match self
            .client
            .get(format!(
                "{}/restate/invocation/{invocation}/attach",
                self.ingress
            ))
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                let state = self.query(&format!("SELECT id,status,retry_count,last_failure,suspended_waiting_for_completions FROM sys_invocation WHERE id = '{invocation}'")).await;
                let journal = self.query(&format!("SELECT index,entry_type,name,entry_lite_json FROM sys_journal WHERE id = '{invocation}' ORDER BY index")).await;
                panic!("attach failed: {error}; runtime state={state:?}; journal={journal:?}");
            }
        }
    }

    pub async fn result(&self, invocation: &str) -> ChunkResult {
        let response = self.attach(invocation).await;
        let status = response.status();
        let body = response.text().await.unwrap();
        assert!(status.is_success(), "chunk failed: {status} {body}");
        serde_json::from_str(&body).unwrap()
    }

    pub async fn query(&self, query: &str) -> Vec<Value> {
        let response = self
            .client
            .post(format!("{}/query", self.admin))
            .header("accept", "application/json")
            .json(&serde_json::json!({"query":query}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert!(
            status.is_success(),
            "Restate introspection failed: {status} {body}"
        );
        body.get("rows").and_then(Value::as_array).unwrap().clone()
    }

    /// Decode persisted payloads, including byte arrays in journal v2 JSON.
    /// Restate 1.6 exposes `raw` only for v1; lite JSON omits run payloads.
    pub async fn journal_bytes(&self, invocation: &str) -> Vec<u8> {
        let rows = self
            .query(&format!(
                "SELECT index,version,entry_type,raw,entry_json FROM sys_journal WHERE id = '{invocation}' ORDER BY index"
            ))
            .await;
        assert!(!rows.is_empty(), "journal must contain evidence");
        let mut bytes = Vec::new();
        for row in rows {
            if row.get("version").and_then(Value::as_u64) == Some(2) {
                let entry = row
                    .get("entry_json")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("v2 journal payload must be inspectable: {row:?}"));
                let entry: Value = serde_json::from_str(entry).unwrap();
                append_persisted_bytes(&entry, &mut bytes);
                continue;
            }
            let raw = row
                .get("raw")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("raw journal bytes must be hex encoded: {row:?}"));
            let raw = raw.strip_prefix("0x").unwrap_or(raw);
            assert_eq!(raw.len() % 2, 0);
            for pair in raw.as_bytes().chunks_exact(2) {
                let pair = std::str::from_utf8(pair).unwrap();
                bytes.push(u8::from_str_radix(pair, 16).expect("valid journal hex"));
            }
        }
        assert!(!bytes.is_empty());
        bytes
    }

    pub async fn wait_status(&self, invocation: &str, expected: &str) {
        let query = format!("SELECT status FROM sys_invocation WHERE id = '{invocation}'");
        for _ in 0..100 {
            let rows = self.query(&query).await;
            if rows
                .first()
                .and_then(|row| row.get("status"))
                .and_then(Value::as_str)
                == Some(expected)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!(
            "invocation {invocation} did not reach {expected}: {:?}",
            self.query(&query).await
        );
    }

    pub async fn restart_worker(&mut self) {
        if let Some(stop) = self.stop_worker.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            worker.await.unwrap();
        }
        let listener = TcpListener::bind(("0.0.0.0", self.worker_port))
            .await
            .unwrap();
        let (worker, stop_worker) = start_worker(listener, self.state.clone());
        self.worker = Some(worker);
        self.stop_worker = Some(stop_worker);
    }

    pub async fn kill(&self, invocation: &str) {
        let response = self
            .client
            .delete(format!("{}/invocations/{invocation}?mode=kill", self.admin))
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "kill rejected: {}",
            response.status()
        );
    }
}

fn append_persisted_bytes(value: &Value, bytes: &mut Vec<u8>) {
    match value {
        Value::String(text) => bytes.extend_from_slice(text.as_bytes()),
        Value::Array(values) => {
            if values.iter().all(|value| {
                value
                    .as_u64()
                    .is_some_and(|number| u8::try_from(number).is_ok())
            }) {
                bytes.extend(
                    values
                        .iter()
                        .map(|value| u8::try_from(value.as_u64().unwrap()).unwrap()),
                );
            } else {
                for value in values {
                    append_persisted_bytes(value, bytes);
                }
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                append_persisted_bytes(value, bytes);
            }
        }
        _ => {}
    }
}

impl Drop for RestateTestContext {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
        // RAII cleans up both successful tests and assertion failures.
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.container])
            .output();
    }
}

fn start_worker(
    listener: TcpListener,
    state: SharedState,
) -> (JoinHandle<()>, oneshot::Sender<()>) {
    let (stop, stopped) = oneshot::channel();
    let worker = tokio::spawn(async move {
        let endpoint = Endpoint::builder()
            .bind(
                IngestionChunkServiceImpl {
                    state: state.clone(),
                }
                .serve(),
            )
            .bind(JiraIngestionHandlerImpl { state }.serve())
            .build();
        HttpServer::new(endpoint)
            .serve_with_cancel(listener, stopped)
            .await;
    });
    (worker, stop)
}

fn docker_url(container: &str, port: &str) -> String {
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
