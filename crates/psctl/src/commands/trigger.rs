use anyhow::Result;
use ps_proto::prism::v1::{TriggerBackfillRequest, TriggerRunRequest};

use crate::client::Clients;

pub async fn trigger(clients: &mut Clients, source: &str) -> Result<()> {
    clients
        .handlers
        .trigger_run(TriggerRunRequest {
            source_name: source.to_string(),
        })
        .await?;
    println!("Triggered ingestion run for '{source}'.");
    Ok(())
}

pub async fn backfill(clients: &mut Clients, source: &str, since: &str) -> Result<()> {
    clients
        .handlers
        .trigger_backfill(TriggerBackfillRequest {
            source_name: source.to_string(),
            since_date: since.to_string(),
        })
        .await?;
    println!("Triggered backfill for '{source}' since {since}.");
    Ok(())
}
