use anyhow::Result;
use ps_proto::prism::v1::{ListRunsRequest, ingestion_service_client::IngestionServiceClient};
use tonic::transport::Channel;

use crate::client::AuthInterceptor;
use crate::format;

pub async fn runs(channel: &Channel, auth: &AuthInterceptor, source: Option<String>) -> Result<()> {
    let mut client = IngestionServiceClient::with_interceptor(channel.clone(), auth.clone());
    let response = client
        .list_runs(ListRunsRequest {
            source_name: source,
        })
        .await?
        .into_inner();

    if response.runs.is_empty() {
        println!("No ingestion runs found.");
        return Ok(());
    }

    println!(
        "{:<10} {:<18} {:<22} {:<12} {:>6} {:>10}",
        "ID", "SOURCE", "STARTED", "STATUS", "ITEMS", "DURATION"
    );
    println!("{}", "─".repeat(82));

    for run in &response.runs {
        let short_id = if run.id.len() > 8 {
            &run.id[..8]
        } else {
            &run.id
        };

        println!(
            "{:<10} {:<18} {:<22} {:<12} {:>6} {:>10}",
            short_id,
            run.source_name,
            format::timestamp(run.started_at.as_ref()),
            run.status,
            run.items_collected,
            format::duration_between(run.started_at.as_ref(), run.completed_at.as_ref()),
        );

        if let Some(ref err) = run.error_message {
            println!("  error: {err}");
        }
    }

    Ok(())
}
