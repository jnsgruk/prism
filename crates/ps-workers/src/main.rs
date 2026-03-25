use std::sync::Arc;

use ps_workers::handlers::SharedState;
use ps_workers::handlers::agent_reaper::{AgentPodReaperHandler, AgentPodReaperHandlerImpl};
use ps_workers::handlers::agentic_query::{AgenticQueryHandler, AgenticQueryHandlerImpl};
use ps_workers::handlers::discourse_ingestion::{
    DiscourseIngestionHandler, DiscourseIngestionHandlerImpl,
};
use ps_workers::handlers::embedding::{EmbeddingHandler, EmbeddingHandlerImpl};
use ps_workers::handlers::enrichment::{EnrichmentHandler, EnrichmentHandlerImpl};
use ps_workers::handlers::github_ingestion::{GithubIngestionHandler, GithubIngestionHandlerImpl};
use ps_workers::handlers::github_team_sync::{GithubTeamSyncHandler, GithubTeamSyncHandlerImpl};
use ps_workers::handlers::identity_resolution::{
    IdentityResolutionHandler, IdentityResolutionHandlerImpl,
};
use ps_workers::handlers::insights::{InsightsHandler, InsightsHandlerImpl};
use ps_workers::handlers::jira_ingestion::{JiraIngestionHandler, JiraIngestionHandlerImpl};
use ps_workers::handlers::metrics_compute::{MetricsComputeHandler, MetricsComputeHandlerImpl};
use ps_workers::handlers::model_catalogue::{ModelCatalogueHandler, ModelCatalogueHandlerImpl};
use restate_sdk::prelude::*;
use tonic::transport::Server;
use tonic_health::ServingStatus;
use tracing::{error, info, warn};

#[tokio::main]
#[allow(clippy::expect_used, clippy::too_many_lines)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Install the rustls crypto provider before any TLS usage (kube, object_store).
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    // Database connection
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = sqlx::PgPool::connect(&database_url).await?;
    info!("connected to database");

    // Load encryption key
    let secret_key = ps_core::crypto::load_secret_key().expect("PS_SECRET_KEY must be set");

    // Shared HTTP client (60s default timeout prevents indefinite hangs)
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("failed to build HTTP client");

    // Container manager — optional, requires K8s access
    let container_manager = match kube::Client::try_default().await {
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
                s3_endpoint: std::env::var("S3_ENDPOINT").unwrap_or_default(),
                s3_bucket: std::env::var("S3_BUCKET").unwrap_or_else(|_| "ps-artifacts".into()),
                s3_access_key_id: std::env::var("S3_ACCESS_KEY_ID").unwrap_or_default(),
                s3_secret_access_key: std::env::var("S3_SECRET_ACCESS_KEY").unwrap_or_default(),
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
    };

    // Object storage — optional, configured via env vars
    let artifact_store: Option<Arc<dyn ps_core::ArtifactStore>> =
        if let (Ok(endpoint), Ok(bucket)) =
            (std::env::var("S3_ENDPOINT"), std::env::var("S3_BUCKET"))
        {
            let access_key = std::env::var("S3_ACCESS_KEY_ID").unwrap_or_default();
            let secret_key_s3 = std::env::var("S3_SECRET_ACCESS_KEY").unwrap_or_default();
            match ps_core::artifact_store::S3ArtifactStore::new(
                &endpoint,
                &bucket,
                &access_key,
                &secret_key_s3,
            ) {
                Ok(store) => {
                    info!(%endpoint, %bucket, "S3 artifact store configured");
                    Some(Arc::new(store))
                }
                Err(e) => {
                    warn!(error = %e, "failed to configure S3 artifact store");
                    None
                }
            }
        } else {
            info!("S3 artifact store not configured (S3_ENDPOINT/S3_BUCKET not set)");
            None
        };

    // Build shared state for all handlers
    let state = SharedState {
        repos: ps_core::repo::Repos::new(pool.clone()),
        secret_key,
        http_client,
        container_manager,
        artifact_store,
    };

    let ingestion = GithubIngestionHandlerImpl {
        state: state.clone(),
    };
    let team_sync = GithubTeamSyncHandlerImpl {
        state: state.clone(),
    };
    let jira_ingestion = JiraIngestionHandlerImpl {
        state: state.clone(),
    };
    let discourse_ingestion = DiscourseIngestionHandlerImpl {
        state: state.clone(),
    };
    let identity_resolution = IdentityResolutionHandlerImpl {
        state: state.clone(),
    };
    let metrics_compute = MetricsComputeHandlerImpl {
        state: state.clone(),
    };
    let insights_compute = InsightsHandlerImpl {
        state: state.clone(),
    };

    // AI provider routing for enrichment handler
    let ai_router = {
        let ai_config = ps_reasoning::types::AiConfig::default();
        let mut router = ps_reasoning::routing::TaskRouter::new(ai_config);

        // Load AI config from global_settings
        if let Ok(settings) = state.repos.config.list_global_settings("ai.").await {
            let mut config = ps_reasoning::types::AiConfig::default();
            for s in &settings {
                match s.key.as_str() {
                    "ai.tasks.enrichment" => {
                        if let Ok(tc) = serde_json::from_value(s.value.clone()) {
                            config.tasks.enrichment = tc;
                        }
                    }
                    "ai.tasks.insights" => {
                        if let Ok(tc) = serde_json::from_value(s.value.clone()) {
                            config.tasks.insights = tc;
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
                    "ai.budget_cap_usd" => {
                        if let Some(cap) = s.value.as_f64() {
                            config.budget_cap_usd = Some(cap);
                        }
                    }
                    _ => {}
                }
            }
            router.update_config(config);
        }

        // Load provider API keys
        for (provider, secret_key_name) in &[
            ("google", "google_api_key"),
            ("openrouter", "openrouter_api_key"),
        ] {
            if let Ok(Some(encrypted)) = state.repos.config.get_global_secret(secret_key_name).await
                && let Ok(decrypted) = ps_core::crypto::decrypt(&state.secret_key, &encrypted)
                && let Ok(api_key) = String::from_utf8(decrypted)
            {
                match *provider {
                    "google" => router.set_google(&api_key),
                    "openrouter" => router.set_openrouter(&api_key),
                    _ => {}
                }
                info!(provider, "loaded AI provider key for enrichment handler");
            }
        }

        Arc::new(tokio::sync::RwLock::new(router))
    };

    let enrichment = EnrichmentHandlerImpl {
        state: state.clone(),
        router: ai_router.clone(),
    };

    let embedding = EmbeddingHandlerImpl {
        state: state.clone(),
        router: ai_router,
    };

    let model_catalogue = ModelCatalogueHandlerImpl {
        state: state.clone(),
    };

    let agentic_query = AgenticQueryHandlerImpl {
        state: state.clone(),
    };

    let agent_reaper = AgentPodReaperHandlerImpl {
        state: state.clone(),
    };

    // Health service for k8s probes
    let health_port = std::env::var("PORT").unwrap_or_else(|_| "9080".into());
    let health_addr = format!("0.0.0.0:{health_port}").parse()?;

    let (health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_service_status("", ServingStatus::Serving)
        .await;

    info!(%health_addr, "starting health server");
    let health_server = tokio::spawn(async move {
        if let Err(e) = Server::builder()
            .add_service(health_service)
            .serve(health_addr)
            .await
        {
            error!("health server error: {e}");
        }
    });

    // Restate endpoint — all handlers bound to a single endpoint
    let restate_port = std::env::var("PS_RESTATE_LISTEN_PORT").unwrap_or_else(|_| "9081".into());
    let restate_addr: std::net::SocketAddr = format!("0.0.0.0:{restate_port}").parse()?;

    info!(%restate_addr, "starting Restate endpoint");
    let restate_server = tokio::spawn(async move {
        HttpServer::new(
            Endpoint::builder()
                .bind(ingestion.serve())
                .bind(team_sync.serve())
                .bind(jira_ingestion.serve())
                .bind(discourse_ingestion.serve())
                .bind(identity_resolution.serve())
                .bind(metrics_compute.serve())
                .bind(insights_compute.serve())
                .bind(enrichment.serve())
                .bind(embedding.serve())
                .bind(model_catalogue.serve())
                .bind(agentic_query.serve())
                .bind(agent_reaper.serve())
                .build(),
        )
        .listen_and_serve(restate_addr)
        .await;
    });

    // Register with Restate admin (best-effort, retries on startup)
    let restate_admin_url =
        std::env::var("RESTATE_ADMIN_URL").unwrap_or_else(|_| "http://restate:9070".into());
    let self_url = std::env::var("RESTATE_SELF_URL")
        .unwrap_or_else(|_| format!("http://ps-workers:{restate_port}"));

    tokio::spawn(async move {
        register_with_restate(&restate_admin_url, &self_url).await;
    });

    // Wait for either server to exit
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
