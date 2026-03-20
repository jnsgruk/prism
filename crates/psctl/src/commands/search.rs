use anyhow::Result;
use ps_proto::prism::v1::SearchByTextRequest;

use crate::client::Clients;

pub async fn search(
    clients: &mut Clients,
    query: &str,
    limit: i32,
    platform: Option<&str>,
) -> Result<()> {
    let resp = clients
        .reasoning
        .search_by_text(SearchByTextRequest {
            query_text: query.to_string(),
            limit,
            platform: platform.map(String::from),
        })
        .await?
        .into_inner();

    if resp.items.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    super::similar::print_similar_items(&resp.items);
    Ok(())
}
