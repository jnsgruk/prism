use ps_proto::prism::v1::admin_service_server::AdminServiceServer;
use ps_proto::prism::v1::auth_service_server::AuthServiceServer;
use ps_server::services::admin::AdminServiceImpl;
use ps_server::services::auth::AuthServiceImpl;
use tonic::transport::Server;
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

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let addr = format!("0.0.0.0:{port}").parse()?;

    info!("connecting to database");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    let auth_service = AuthServiceImpl::new(pool.clone());
    let admin_service = AdminServiceImpl::new(pool.clone());

    info!(%addr, "starting gRPC server");

    Server::builder()
        .accept_http1(true)
        .layer(tonic_web::GrpcWebLayer::new())
        .add_service(AuthServiceServer::new(auth_service))
        .add_service(AdminServiceServer::new(admin_service))
        .serve(addr)
        .await?;

    Ok(())
}
