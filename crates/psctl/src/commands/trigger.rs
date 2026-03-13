use anyhow::Result;
use ps_proto::prism::v1::{
    TriggerBackfillRequest, TriggerRunRequest, ingestion_service_client::IngestionServiceClient,
};
use tonic::transport::Channel;

use crate::client::AuthInterceptor;

pub async fn trigger(channel: &Channel, auth: &AuthInterceptor, source: &str) -> Result<()> {
    let mut client = IngestionServiceClient::with_interceptor(channel.clone(), auth.clone());
    client
        .trigger_run(TriggerRunRequest {
            source_name: source.to_string(),
        })
        .await?;
    println!("Triggered ingestion run for '{source}'.");
    Ok(())
}

pub async fn backfill(
    channel: &Channel,
    auth: &AuthInterceptor,
    source: &str,
    since: &str,
) -> Result<()> {
    let mut client = IngestionServiceClient::with_interceptor(channel.clone(), auth.clone());
    client
        .trigger_backfill(TriggerBackfillRequest {
            source_name: source.to_string(),
            since_date: since.to_string(),
        })
        .await?;
    println!("Triggered backfill for '{source}' since {since}.");
    Ok(())
}
