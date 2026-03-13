use anyhow::Result;
use ps_proto::prism::v1::{CreateBackupRequest, admin_service_client::AdminServiceClient};
use tokio::io::AsyncWriteExt;
use tonic::transport::Channel;

use crate::client::AuthInterceptor;

pub async fn backup(
    channel: &Channel,
    auth: &AuthInterceptor,
    output: Option<String>,
) -> Result<()> {
    let mut client = AdminServiceClient::with_interceptor(channel.clone(), auth.clone());

    let response = client.create_backup(CreateBackupRequest {}).await?;
    let mut stream = response.into_inner();

    let filename = output.unwrap_or_else(|| {
        let date = time::OffsetDateTime::now_utc().date();
        format!("prism-backup-{date}.ps-backup")
    });

    let mut file = tokio::fs::File::create(&filename).await?;
    let mut total_bytes: u64 = 0;

    while let Some(chunk) = stream.message().await? {
        file.write_all(&chunk.chunk).await?;
        total_bytes += chunk.chunk.len() as u64;
    }

    file.flush().await?;
    println!("Backup saved to {filename} ({total_bytes} bytes).");
    Ok(())
}
