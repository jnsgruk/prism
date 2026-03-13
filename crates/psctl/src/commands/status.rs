use anyhow::Result;
use ps_proto::prism::v1::{GetStatusRequest, ingestion_service_client::IngestionServiceClient};
use tonic::transport::Channel;

use crate::client::AuthInterceptor;
use crate::format;

pub async fn status(channel: &Channel, auth: &AuthInterceptor) -> Result<()> {
    let mut client = IngestionServiceClient::with_interceptor(channel.clone(), auth.clone());
    let response = client.get_status(GetStatusRequest {}).await?.into_inner();

    if response.sources.is_empty() {
        println!("No sources configured.");
        return Ok(());
    }

    println!(
        "{:<20} {:<10} {:<12} {:<22} {:>6}",
        "SOURCE", "TYPE", "STATE", "LAST RUN", "ITEMS"
    );
    println!("{}", "─".repeat(74));

    for source in &response.sources {
        println!(
            "{:<20} {:<10} {:<12} {:<22} {:>6}",
            source.name,
            source.source_type,
            format::source_state(source.state),
            format::timestamp(source.last_run.as_ref()),
            source.items_collected,
        );

        if !source.rate_limit_info.is_empty() {
            for (key, value) in &source.rate_limit_info {
                println!("  {key}: {value}");
            }
        }
    }

    Ok(())
}
