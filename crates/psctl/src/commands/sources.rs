use anyhow::Result;
use ps_proto::canonical::prism::v1::ListSourcesRequest;

use crate::client::Clients;

pub async fn sources(clients: &mut Clients) -> Result<()> {
    let response = clients
        .config
        .list_sources(ListSourcesRequest {})
        .await?
        .into_inner();

    if response.sources.is_empty() {
        println!("No sources configured.");
        return Ok(());
    }

    println!(
        "{:<20} {:<10} {:<9} {:<16} SCHEDULE",
        "NAME", "TYPE", "ENABLED", "SECRETS"
    );
    println!("{}", "─".repeat(72));

    for source in &response.sources {
        let secrets: String = source
            .secret_status
            .iter()
            .map(|(k, v)| format!("{k}: {}", if *v { "set" } else { "missing" }))
            .collect::<Vec<_>>()
            .join(", ");

        println!(
            "{:<20} {:<10} {:<9} {:<16} {}",
            source.name,
            source.source_type,
            if source.enabled { "yes" } else { "no" },
            if secrets.is_empty() {
                "—".to_string()
            } else {
                secrets
            },
            source.schedule_cron.as_deref().unwrap_or("default"),
        );
    }

    Ok(())
}
