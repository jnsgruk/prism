use anyhow::Result;
use ps_proto::canonical::prism::v1::{
    CancelBackupRequest, CreateBackupRequest, create_backup_response::Payload,
};
use tokio::io::AsyncWriteExt;

use crate::client::Clients;

pub async fn backup(
    clients: &mut Clients,
    output: Option<String>,
    no_workspaces: bool,
    force: bool,
) -> Result<()> {
    let response = clients
        .backup
        .create_backup(CreateBackupRequest {
            exclude_workspaces: no_workspaces,
            force,
        })
        .await?;
    let mut stream = response.into_inner();

    let filename = output.unwrap_or_else(|| {
        let date = time::OffsetDateTime::now_utc().date();
        format!("prism-backup-{date}.ps-backup")
    });

    let mut file = tokio::fs::File::create(&filename).await?;
    let mut total_bytes: u64 = 0;
    let mut streaming_started = false;
    let mut last_phase = String::new();

    eprintln!("Creating backup...");

    // Clone a client for the Ctrl+C handler
    let mut cancel_client = clients.backup.clone();

    let result = tokio::select! {
        result = async {
            while let Some(msg) = stream.message().await? {
                match msg.payload {
                    Some(Payload::Progress(p)) => {
                        if p.phase != last_phase {
                            eprintln!("  {}", p.phase);
                            last_phase = p.phase;
                        }
                    }
                    Some(Payload::Chunk(chunk)) => {
                        if !streaming_started {
                            streaming_started = true;
                            eprintln!("  streaming");
                        }
                        file.write_all(&chunk).await?;
                        total_bytes += chunk.len() as u64;
                    }
                    None => {}
                }
            }
            Ok::<_, anyhow::Error>(())
        } => result,

        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nCancelling backup...");
            let _ = cancel_client
                .cancel_backup(CancelBackupRequest {})
                .await;
            // Clean up partial file
            drop(file);
            let _ = tokio::fs::remove_file(&filename).await;
            anyhow::bail!("backup cancelled");
        }
    };

    result?;

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
