use anyhow::Result;
use ps_proto::prism::v1::GetEmbeddingStatusRequest;

use crate::client::Clients;

pub async fn embed_status(clients: &mut Clients) -> Result<()> {
    let resp = clients
        .reasoning
        .get_embedding_status(GetEmbeddingStatusRequest {})
        .await?
        .into_inner();

    println!("Embedding Pipeline Status");
    println!("  Queued:    {}", resp.queued_count);
    println!("  Embedded:  {}", resp.embedded_count);
    println!("  Eligible:  {}", resp.total_eligible);
    println!("  Coverage:  {:.1}%", resp.coverage_percent);
    if let Some(last) = &resp.last_embedded_at {
        println!("  Last run:  {last}");
    }

    Ok(())
}
