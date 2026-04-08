use std::sync::Arc;

use ps_workers::infra::SharedState;
use restate_sdk::prelude::*;
use tonic::transport::Server;
use tonic_health::ServingStatus;
use tracing::{error, info, warn};

#[tokio::main]
#[allow(clippy::expect_used)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let pool = connect_database().await?;
    let secret_key = ps_core::crypto::load_secret_key().expect("PS_SECRET_KEY must be set");
    let http_client = build_http_client();
    let container_manager = setup_container_manager().await;

    let state = SharedState {
        repos: ps_core::repo::Repos::new(pool.clone()),
        secret_key,
        http_client,
        container_manager,
    };

    let ai_router = setup_ai_router(&state).await;

    let health_server = start_health_server().await?;
    let restate_server = start_restate_server(state, ai_router);

    spawn_bootstrap_tasks();

    tokio::select! {
        r = health_server => {
            if let Err(e) = r {
                error!("health server panicked: {e}");
            }
        }
        r = restate_server => {
            if let Err(e) = r {
                error!("restate server panicked: {e}");
            }
        }
    }

    Ok(())
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();
}

#[allow(clippy::expect_used)]
async fn connect_database() -> Result<sqlx::PgPool, Box<dyn std::error::Error>> {
    // Install the rustls crypto provider before any TLS usage (kube, reqwest).
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = sqlx::PgPool::connect(&database_url).await?;
    info!("connected to database");
    Ok(pool)
}

#[allow(clippy::expect_used)]
fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("failed to build HTTP client")
}

async fn setup_container_manager() -> Option<Arc<ps_agent::ContainerManager>> {
    match kube::Client::try_default().await {
        Ok(kube_client) => {
            let namespace = std::env::var("POD_NAMESPACE").unwrap_or_else(|_| "prism".into());
            let agent_image =
                std::env::var("AGENT_IMAGE").unwrap_or_else(|_| "prism/prism-agent:latest".into());
            info!(agent_image = %agent_image, "using agent container image");
            let config = ps_agent::AgentPodConfig {
                image: agent_image,
                namespace: namespace.clone(),
                model: String::new(),
                small_model: String::new(),
                prism_api_url: "http://ps-server:8080".to_string(),
                service_token: String::new(),
                provider_keys: vec![],
            };
            Some(Arc::new(ps_agent::ContainerManager::new(
                kube_client,
                namespace,
                config,
            )))
        }
        Err(e) => {
            warn!(error = %e, "K8s not available — agent containers disabled");
            None
        }
    }
}

async fn setup_ai_router(
    state: &SharedState,
) -> Arc<tokio::sync::RwLock<ps_reasoning::routing::TaskRouter>> {
    let ai_config = ps_reasoning::types::AiConfig::default();
    let mut router = ps_reasoning::routing::TaskRouter::new(ai_config);

    if let Ok(settings) = state.repos.config.list_global_settings("ai.").await {
        let mut config = ps_reasoning::types::AiConfig::default();
        for s in &settings {
            match s.key.as_str() {
                "ai.tasks.enrichment" => {
                    if let Ok(tc) = serde_json::from_value(s.value.clone()) {
                        config.tasks.enrichment = tc;
                    }
                }
                "ai.tasks.agentic" => {
                    if let Ok(tc) = serde_json::from_value(s.value.clone()) {
                        config.tasks.agentic = tc;
                    }
                }
                "ai.tasks.embeddings" => {
                    if let Ok(tc) = serde_json::from_value(s.value.clone()) {
                        config.tasks.embeddings = tc;
                    }
                }
                _ => {}
            }
        }
        router.update_config(config);
    }

    if let Ok(Some(encrypted)) = state.repos.config.get_global_secret("google_api_key").await
        && let Ok(decrypted) = ps_core::crypto::decrypt(&state.secret_key, &encrypted)
        && let Ok(api_key) = String::from_utf8(decrypted)
    {
        router.set_google(&api_key);
        info!("loaded Google AI provider key for enrichment handler");
    }

    Arc::new(tokio::sync::RwLock::new(router))
}

async fn start_health_server() -> Result<tokio::task::JoinHandle<()>, Box<dyn std::error::Error>> {
    let health_port = std::env::var("PORT").unwrap_or_else(|_| "9080".into());
    let health_addr: std::net::SocketAddr = format!("0.0.0.0:{health_port}").parse()?;

    let (health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_service_status("", ServingStatus::Serving)
        .await;

    info!(%health_addr, "starting health server");
    Ok(tokio::spawn(async move {
        if let Err(e) = Server::builder()
            .add_service(health_service)
            .serve(health_addr)
            .await
        {
            error!("health server error: {e}");
        }
    }))
}

#[allow(clippy::expect_used)]
fn start_restate_server(
    state: SharedState,
    ai_router: Arc<tokio::sync::RwLock<ps_reasoning::routing::TaskRouter>>,
) -> tokio::task::JoinHandle<()> {
    let restate_port = std::env::var("PS_RESTATE_LISTEN_PORT").unwrap_or_else(|_| "9081".into());
    let restate_addr: std::net::SocketAddr = format!("0.0.0.0:{restate_port}")
        .parse()
        .expect("invalid Restate listen address");

    info!(%restate_addr, "starting Restate endpoint");
    tokio::spawn(async move {
        let endpoint = Endpoint::builder();
        let endpoint = ps_workers::features::ingestion::bind(endpoint, &state);
        let endpoint = ps_workers::features::identity_resolution::bind(endpoint, &state);
        let endpoint = ps_workers::features::metrics::bind(endpoint, &state);
        let endpoint = ps_workers::features::reasoning::bind(endpoint, &state, ai_router);
        let endpoint = ps_workers::features::pipeline::bind(endpoint, &state);

        HttpServer::new(endpoint.build())
            .listen_and_serve(restate_addr)
            .await;
    })
}

fn spawn_bootstrap_tasks() {
    let restate_admin_url =
        std::env::var("RESTATE_ADMIN_URL").unwrap_or_else(|_| "http://restate:9070".into());
    let restate_port = std::env::var("PS_RESTATE_LISTEN_PORT").unwrap_or_else(|_| "9081".into());
    let self_url = std::env::var("RESTATE_SELF_URL")
        .unwrap_or_else(|_| format!("http://ps-workers:{restate_port}"));
    let restate_ingress_url =
        std::env::var("RESTATE_URL").unwrap_or_else(|_| "http://restate:8080".into());

    tokio::spawn(async move {
        register_with_restate(&restate_admin_url, &self_url).await;
        bootstrap_reaper(&restate_ingress_url, &restate_admin_url).await;
        bootstrap_watchdog(&restate_ingress_url, &restate_admin_url).await;
    });
}

/// Register this service deployment with the Restate admin API.
///
/// Retries up to 10 times with exponential backoff to handle startup
/// ordering (Restate may not be ready yet when we start).
async fn register_with_restate(admin_url: &str, self_url: &str) {
    let client = reqwest::Client::new();
    let url = format!("{admin_url}/deployments");

    for attempt in 1u64..=10 {
        let body = serde_json::json!({
            "uri": self_url,
            "force": true,
        });

        match client.post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!(self_url, "registered with Restate");
                return;
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                warn!(
                    attempt,
                    %status,
                    body,
                    "failed to register with Restate, retrying"
                );
            }
            Err(e) => {
                warn!(attempt, "cannot reach Restate admin: {e}");
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(attempt * 2)).await;
    }

    error!("gave up registering with Restate after 10 attempts");
}

/// Ensure the agent pod reaper loop is running.
///
/// Queries the Restate admin API for existing invocations before sending a
/// new one. Without this check, every pod restart would queue a new bootstrap
/// invocation — each of which self-schedules its own chain, causing geometric
/// growth of reaper invocations.
async fn bootstrap_reaper(ingress_url: &str, admin_url: &str) {
    use ps_workers::features::reasoning::agent_reaper::REAPER_KEY;

    let client = reqwest::Client::new();

    // Check if there are already active invocations for the reaper via Restate SQL API.
    let query_url = format!("{admin_url}/query");
    let sql = serde_json::json!({
        "query": "SELECT COUNT(*) AS cnt FROM sys_invocation \
                  WHERE target_service_name = 'AgentPodReaperHandler' \
                  AND status IN ('scheduled', 'running', 'suspended', 'ready', 'backing-off')"
    });
    match client
        .post(&query_url)
        .header("Accept", "application/json")
        .json(&sql)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                let active_count = body
                    .get("rows")
                    .and_then(|r| r.as_array())
                    .and_then(|rows| rows.first())
                    .and_then(|row| row.get("cnt"))
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);

                if active_count > 0 {
                    info!(
                        active_count,
                        "reaper already has active invocations, skipping bootstrap"
                    );
                    return;
                }
            }
        }
        Ok(resp) => {
            warn!(
                status = %resp.status(),
                "could not query reaper invocations, will bootstrap anyway"
            );
        }
        Err(e) => {
            warn!(error = %e, "could not reach Restate admin to check reaper, will bootstrap anyway");
        }
    }

    let send_url = format!("{ingress_url}/AgentPodReaperHandler/{REAPER_KEY}/reap/send");
    match client.post(&send_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            info!("bootstrapped agent pod reaper loop");
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(%status, body, "failed to bootstrap agent pod reaper");
        }
        Err(e) => {
            error!(error = %e, "failed to send reaper bootstrap invocation");
        }
    }
}

/// Ensure the query watchdog loop is running.
///
/// Same duplicate-prevention pattern as `bootstrap_reaper`: query Restate
/// admin for existing invocations before sending a new one.
async fn bootstrap_watchdog(ingress_url: &str, admin_url: &str) {
    use ps_workers::features::reasoning::query_watchdog::WATCHDOG_KEY;

    let client = reqwest::Client::new();

    let query_url = format!("{admin_url}/query");
    let sql = serde_json::json!({
        "query": "SELECT COUNT(*) AS cnt FROM sys_invocation \
                  WHERE target_service_name = 'QueryWatchdogHandler' \
                  AND status IN ('scheduled', 'running', 'suspended', 'ready', 'backing-off')"
    });
    match client
        .post(&query_url)
        .header("Accept", "application/json")
        .json(&sql)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                let active_count = body
                    .get("rows")
                    .and_then(|r| r.as_array())
                    .and_then(|rows| rows.first())
                    .and_then(|row| row.get("cnt"))
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);

                if active_count > 0 {
                    info!(
                        active_count,
                        "query watchdog already has active invocations, skipping bootstrap"
                    );
                    return;
                }
            }
        }
        Ok(resp) => {
            warn!(
                status = %resp.status(),
                "could not query watchdog invocations, will bootstrap anyway"
            );
        }
        Err(e) => {
            warn!(error = %e, "could not reach Restate admin to check watchdog, will bootstrap anyway");
        }
    }

    let send_url = format!("{ingress_url}/QueryWatchdogHandler/{WATCHDOG_KEY}/check/send");
    match client.post(&send_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            info!("bootstrapped query watchdog loop");
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(%status, body, "failed to bootstrap query watchdog");
        }
        Err(e) => {
            error!(error = %e, "failed to send watchdog bootstrap invocation");
        }
    }
}
