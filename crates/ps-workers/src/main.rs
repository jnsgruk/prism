use std::sync::Arc;

use ps_workers::handlers::SharedState;
use ps_workers::handlers::discourse_ingestion::{
    DiscourseIngestionHandler, DiscourseIngestionHandlerImpl,
};
use ps_workers::handlers::enrichment::{EnrichmentHandler, EnrichmentHandlerImpl};
use ps_workers::handlers::github_ingestion::{GithubIngestionHandler, GithubIngestionHandlerImpl};
use ps_workers::handlers::github_team_sync::{GithubTeamSyncHandler, GithubTeamSyncHandlerImpl};
use ps_workers::handlers::identity_resolution::{
    IdentityResolutionHandler, IdentityResolutionHandlerImpl,
};
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

    // Shared HTTP client
    let http_client = reqwest::Client::new();

    // Build shared state for all handlers
    let state = SharedState {
        repos: ps_core::repo::Repos::new(pool.clone()),
        secret_key,
        http_client,
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
        router: ai_router,
    };

    let model_catalogue = ModelCatalogueHandlerImpl {
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
                .bind(enrichment.serve())
                .bind(model_catalogue.serve())
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
