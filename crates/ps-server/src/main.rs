use std::sync::Arc;

use ps_core::crypto::load_secret_key;
use ps_proto::canonical::prism::v1::admin_service_server::AdminServiceServer;
use ps_proto::canonical::prism::v1::auth_service_server::AuthServiceServer;
use ps_proto::canonical::prism::v1::config_service_server::ConfigServiceServer;
use ps_proto::canonical::prism::v1::handlers_service_server::HandlersServiceServer;
use ps_proto::canonical::prism::v1::insights_service_server::InsightsServiceServer;
use ps_proto::canonical::prism::v1::metrics_service_server::MetricsServiceServer;
use ps_proto::canonical::prism::v1::org_service_server::OrgServiceServer;
use ps_proto::canonical::prism::v1::reasoning_service_server::ReasoningServiceServer;
use ps_server::interceptor::AuthLayer;
use ps_server::services::admin::AdminServiceImpl;
use ps_server::services::auth::AuthServiceImpl;
use ps_server::services::config::ConfigServiceImpl;
use ps_server::services::handlers::HandlersServiceImpl;
use ps_server::services::insights::InsightsServiceImpl;
use ps_server::services::metrics::MetricsServiceImpl;
use ps_server::services::org::OrgServiceImpl;
use ps_server::services::reasoning::ReasoningServiceImpl;
use tonic::transport::Server;
use tonic_health::ServingStatus;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Install the rustls crypto provider before any TLS usage. Both `ring`
    // (from object_store) and `aws-lc-rs` (from kube) are in the dep tree,
    // so rustls can't auto-detect — we explicitly pick aws-lc-rs.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL environment variable not set")?;

    let secret_key = load_secret_key()?;

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let addr = format!("0.0.0.0:{port}").parse()?;

    info!("connecting to database");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    let repos = ps_core::repo::Repos::new(pool.clone());

    let auth_service = AuthServiceImpl::new(repos.clone());
    let admin_service = AdminServiceImpl::new(repos.clone());
    let org_service = OrgServiceImpl::new(repos.clone());
    let config_service = ConfigServiceImpl::new(repos.clone(), secret_key.clone());
    let restate_url = std::env::var("RESTATE_URL").unwrap_or_else(|_| "http://restate:8080".into());
    let restate_admin_url =
        std::env::var("RESTATE_ADMIN_URL").unwrap_or_else(|_| "http://restate:9070".into());
    let metrics_service = MetricsServiceImpl::new(repos.clone());
    let insights_service = InsightsServiceImpl::new(repos.clone());
    let handlers_service =
        HandlersServiceImpl::new(repos.clone(), restate_url.clone(), restate_admin_url);

    // AI reasoning — task router with default config, providers set later via admin UI
    let ai_config = ps_reasoning::types::AiConfig::default();
    let router = Arc::new(tokio::sync::RwLock::new(
        ps_reasoning::routing::TaskRouter::new(ai_config),
    ));

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
                    tracing::warn!(error = %e, "failed to configure S3 artifact store");
                    None
                }
            }
        } else {
            info!("S3 artifact store not configured (S3_ENDPOINT/S3_BUCKET not set)");
            None
        };

    // Container manager — optional, requires K8s access
    let container_manager = match kube::Client::try_default().await {
        Ok(kube_client) => {
            let agent_image =
                std::env::var("AGENT_IMAGE").unwrap_or_else(|_| "prism-agent:latest".into());
            let namespace = std::env::var("POD_NAMESPACE").unwrap_or_else(|_| "prism".into());
            let config = ps_server::container_manager::pod_spec::AgentPodConfig {
                image: agent_image,
                namespace: namespace.clone(),
                model: String::new(), // Set dynamically from AI settings per request.
                small_model: String::new(),
                prism_api_url: "http://ps-server:8080".to_string(),
                service_token: String::new(), // TODO: generate/read service token
                s3_endpoint: std::env::var("S3_ENDPOINT").unwrap_or_default(),
                s3_bucket: std::env::var("S3_BUCKET").unwrap_or_else(|_| "ps-artifacts".into()),
                provider_keys: vec![],
            };
            let cm =
                ps_server::container_manager::ContainerManager::new(kube_client, namespace, config);

            // Start background reaper task.
            let reaper = cm.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    reaper.reap_idle_pods().await;
                }
            });

            info!("Container manager configured");
            Some(Arc::new(cm))
        }
        Err(e) => {
            tracing::warn!(error = %e, "K8s not available — agent containers disabled");
            None
        }
    };

    let reasoning_service = ReasoningServiceImpl::new(
        repos.clone(),
        secret_key,
        router,
        artifact_store,
        container_manager,
        restate_url,
    );

    // Load AI provider keys from the database so they survive server restarts.
    reasoning_service.load_providers_from_db().await;

    let (health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_service_status("", ServingStatus::Serving)
        .await;

    info!(%addr, "starting gRPC server");

    Server::builder()
        .accept_http1(true)
        .layer(tonic_web::GrpcWebLayer::new())
        .layer(AuthLayer::new(repos.auth.clone()))
        .add_service(health_service)
        .add_service(AuthServiceServer::new(auth_service))
        .add_service(AdminServiceServer::new(admin_service))
        .add_service(OrgServiceServer::new(org_service))
        .add_service(ConfigServiceServer::new(config_service))
        .add_service(MetricsServiceServer::new(metrics_service))
        .add_service(HandlersServiceServer::new(handlers_service))
        .add_service(InsightsServiceServer::new(insights_service))
        .add_service(ReasoningServiceServer::new(reasoning_service))
        .serve(addr)
        .await?;

    Ok(())
}
