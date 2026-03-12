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

    let port = std::env::var("PORT").unwrap_or_else(|_| "9080".into());
    let addr = format!("0.0.0.0:{port}").parse()?;

    let (health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_service_status("", ServingStatus::Serving)
        .await;

    info!(%addr, "starting ingestion server");

    Server::builder()
        .add_service(health_service)
        .serve(addr)
        .await?;

    Ok(())
}
