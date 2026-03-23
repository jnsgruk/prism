use anyhow::Result;
use ps_proto::canonical::prism::v1::CreateBackupRequest;
use tokio::io::AsyncWriteExt;

use crate::client::Clients;

pub async fn backup(clients: &mut Clients, output: Option<String>) -> Result<()> {
    let response = clients.admin.create_backup(CreateBackupRequest {}).await?;
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

    // Restrict backup file permissions to owner-only on Unix (contains sensitive data).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        tokio::fs::set_permissions(&filename, perms).await?;
    }

    println!("Backup saved to {filename} ({total_bytes} bytes).");
    Ok(())
}
