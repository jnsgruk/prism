use std::sync::Arc;

use ps_core::crypto::load_secret_key;
use ps_proto::canonical::prism::v1::admin_service_server::AdminServiceServer;
use ps_proto::canonical::prism::v1::auth_service_server::AuthServiceServer;
use ps_proto::canonical::prism::v1::backup_service_server::BackupServiceServer;
use ps_proto::canonical::prism::v1::config_service_server::ConfigServiceServer;
use ps_proto::canonical::prism::v1::handlers_service_server::HandlersServiceServer;
use ps_proto::canonical::prism::v1::insights_service_server::InsightsServiceServer;
use ps_proto::canonical::prism::v1::metrics_service_server::MetricsServiceServer;
use ps_proto::canonical::prism::v1::org_service_server::OrgServiceServer;
use ps_proto::canonical::prism::v1::reasoning_service_server::ReasoningServiceServer;
use ps_server::features::admin::AdminServiceImpl;
use ps_server::features::auth::AuthServiceImpl;
use ps_server::features::backup::BackupServiceImpl;
use ps_server::features::config::ConfigServiceImpl;
use ps_server::features::dispatch::HandlersServiceImpl;
use ps_server::features::insights::InsightsServiceImpl;
use ps_server::features::metrics::MetricsServiceImpl;
use ps_server::features::org::OrgServiceImpl;
use ps_server::features::reasoning::ReasoningServiceImpl;
use ps_server::interceptor::AuthLayer;
use tonic::transport::Server;
use tonic_health::ServingStatus;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Install the rustls crypto provider before any TLS usage. Multiple
    // providers may be in the dep tree, so we explicitly pick aws-lc-rs.
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

    // Workspace filesystem path — set when the shared workspaces PVC is mounted.
    let workspaces_path = std::env::var("WORKSPACES_PATH").ok().map(|p| {
        info!(path = %p, "workspace filesystem access configured");
        std::path::PathBuf::from(p)
    });

    let workspaces_capacity_bytes = std::env::var("WORKSPACES_CAPACITY_BYTES")
        .ok()
        .and_then(|v| v.parse::<i64>().ok());

    let backups_path = std::env::var("BACKUPS_PATH")
        .ok()
        .map(std::path::PathBuf::from);

    let restate_url = std::env::var("RESTATE_URL").unwrap_or_else(|_| "http://restate:8080".into());

    // Backup generator — K8s Job management
    let backup_generator: std::sync::Arc<dyn ps_server::features::backup::BackupGenerator> = {
        let kube_client = kube::Client::try_default()
            .await
            .map_err(|e| format!("failed to create kube client: {e}"))?;
        let backup_image =
            std::env::var("BACKUP_IMAGE").unwrap_or_else(|_| "prism/ps-backup:latest".into());
        let pod_namespace = std::env::var("POD_NAMESPACE").unwrap_or_else(|_| "prism".into());
        std::sync::Arc::new(
            ps_server::features::backup::generator::KubeBackupGenerator::new(
                kube_client,
                pod_namespace,
                backup_image,
            ),
        )
    };

    let auth_service = AuthServiceImpl::new(repos.clone());
    let admin_service = AdminServiceImpl::new(
        repos.clone(),
        workspaces_path.clone(),
        workspaces_capacity_bytes,
    );

    // AI reasoning — task router with default config, providers set later via admin UI
    let ai_config = ps_reasoning::types::AiConfig::default();
    let router = Arc::new(tokio::sync::RwLock::new(
        ps_reasoning::routing::TaskRouter::new(ai_config),
    ));

    // Post-restore hook to reload AI provider keys from the freshly-restored database.
    let post_restore_hook: ps_server::features::backup::PostRestoreHook = {
        let repos = repos.clone();
        let secret_key = secret_key.clone();
        let router = router.clone();
        Arc::new(move || {
            let repos = repos.clone();
            let secret_key = secret_key.clone();
            let router = router.clone();
            Box::pin(async move {
                ps_server::features::reasoning::reload_ai_providers(&repos, &secret_key, &router)
                    .await;
            })
        })
    };

    let backup_service = BackupServiceImpl::new(
        repos.clone(),
        secret_key.clone(),
        backups_path,
        backup_generator,
        Some(post_restore_hook),
    );
    let org_service = OrgServiceImpl::new(repos.clone());
    let config_service = ConfigServiceImpl::new(repos.clone(), secret_key.clone());
    let restate_admin_url =
        std::env::var("RESTATE_ADMIN_URL").unwrap_or_else(|_| "http://restate:9070".into());
    let metrics_service = MetricsServiceImpl::new(repos.clone());
    let insights_service = InsightsServiceImpl::new(repos.clone());
    let handlers_service =
        HandlersServiceImpl::new(repos.clone(), restate_url.clone(), restate_admin_url);

    let reasoning_service = ReasoningServiceImpl::new(
        repos.clone(),
        secret_key,
        router,
        workspaces_path,
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
        .add_service(BackupServiceServer::new(backup_service))
        .add_service(OrgServiceServer::new(org_service))
        .add_service(ConfigServiceServer::new(config_service))
        .add_service(MetricsServiceServer::new(metrics_service))
        .add_service(HandlersServiceServer::new(handlers_service))
        .add_service(InsightsServiceServer::new(insights_service))
        .add_service(
            ReasoningServiceServer::new(reasoning_service)
                .max_decoding_message_size(100 * 1024 * 1024),
        )
        .serve(addr)
        .await?;

    Ok(())
}
