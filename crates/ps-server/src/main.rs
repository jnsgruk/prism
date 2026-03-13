use ps_core::crypto::load_secret_key;
use ps_proto::prism::v1::admin_service_server::AdminServiceServer;
use ps_proto::prism::v1::auth_service_server::AuthServiceServer;
use ps_proto::prism::v1::config_service_server::ConfigServiceServer;
use ps_proto::prism::v1::ingestion_service_server::IngestionServiceServer;
use ps_proto::prism::v1::org_service_server::OrgServiceServer;
use ps_server::interceptor::AuthLayer;
use ps_server::services::admin::AdminServiceImpl;
use ps_server::services::auth::AuthServiceImpl;
use ps_server::services::config::ConfigServiceImpl;
use ps_server::services::ingestion::IngestionServiceImpl;
use ps_server::services::org::OrgServiceImpl;
use tonic::transport::Server;
use tonic_health::ServingStatus;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let config_service = ConfigServiceImpl::new(repos.clone(), secret_key);
    let restate_url = std::env::var("RESTATE_URL").unwrap_or_else(|_| "http://restate:8080".into());
    let restate_admin_url =
        std::env::var("RESTATE_ADMIN_URL").unwrap_or_else(|_| "http://restate:9070".into());
    let ingestion_service =
        IngestionServiceImpl::new(repos.clone(), restate_url, restate_admin_url);

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
        .add_service(IngestionServiceServer::new(ingestion_service))
        .serve(addr)
        .await?;

    Ok(())
}
