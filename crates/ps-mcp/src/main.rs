use rmcp::ServiceExt;
use rmcp::transport::io::stdio;
use tracing_subscriber::EnvFilter;

mod artifact_store;
mod prism_client;
mod tools;

use artifact_store::ArtifactStore;
use prism_client::PrismClient;
use tools::PrismTools;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let prism_url =
        std::env::var("PRISM_API_URL").unwrap_or_else(|_| "http://ps-server:50051".to_string());
    let token = std::env::var("SERVICE_TOKEN").unwrap_or_default();
    let session_id = std::env::var("SESSION_ID").unwrap_or_default();
    let s3_endpoint = std::env::var("S3_ENDPOINT").ok();
    let s3_bucket = std::env::var("S3_BUCKET").unwrap_or_else(|_| "ps-artifacts".to_string());

    let client = PrismClient::connect(&prism_url, &token)?;
    let artifacts = ArtifactStore::new(s3_endpoint.as_deref(), &s3_bucket, &session_id);
    let tools = PrismTools::new(client, artifacts);

    tracing::info!("ps-agent-mcp starting on stdio");
    let server = tools.serve(stdio()).await.inspect_err(|e| {
        tracing::error!(error = %e, "MCP server failed");
    })?;
    server.waiting().await?;
    Ok(())
}
