use anyhow::Result;
use ps_proto::canonical::prism::v1::{FindSimilarRequest, SimilarItem};

use crate::client::Clients;
use crate::format;

pub async fn similar(
    clients: &mut Clients,
    contribution_id: &str,
    limit: i32,
    platform: Option<&str>,
) -> Result<()> {
    let resp = clients
        .reasoning
        .find_similar(FindSimilarRequest {
            contribution_id: contribution_id.to_string(),
            limit,
            platform: super::contributions::platform_str_to_proto(platform.unwrap_or("")),
            platform_instance: None,
        })
        .await?
        .into_inner();

    if resp.items.is_empty() {
        println!("No similar contributions found.");
        return Ok(());
    }

    print_similar_items(&resp.items);
    Ok(())
}

pub fn print_similar_items(items: &[SimilarItem]) {
    println!(
        "{:<38} {:<10} {:<16} {:<8} TITLE",
        "CONTRIBUTION ID", "PLATFORM", "TYPE", "DIST"
    );
    for item in items {
        println!(
            "{:<38} {:<10} {:<16} {:<8.3} {}",
            item.contribution_id,
            super::contributions::proto_platform_display(item.platform),
            super::contributions::proto_contribution_type_display(item.contribution_type),
            item.distance,
            format::truncate(&item.title, 50),
        );
    }
}
