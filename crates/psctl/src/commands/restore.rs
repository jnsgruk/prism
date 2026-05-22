use anyhow::{Result, bail};
use ps_proto::canonical::prism::v1::{
    PreviewBackupRequest, RestoreBackupRequest, UploadBackupChunkRequest,
};
use std::io::Write as _;
use tokio::io::AsyncReadExt;

use crate::client::Clients;
use crate::format;

const CHUNK_SIZE: usize = 256 * 1024;

/// Upload a backup file via chunked `UploadBackupChunk` calls and return the `upload_id`.
async fn upload_file(clients: &mut Clients, file_path: &str) -> Result<String> {
    let mut file = tokio::fs::File::open(file_path).await?;
    let mut upload_id = String::new();
    let mut total_bytes: u64 = 0;

    loop {
        let mut buf = vec![0u8; CHUNK_SIZE];
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        buf.truncate(n);

        let is_first = upload_id.is_empty();
        let is_final = {
            let mut probe = [0u8; 1];
            let peek = file.read(&mut probe).await?;
            if peek > 0 {
                buf.extend_from_slice(probe.as_slice());
                total_bytes += peek as u64;
                false
            } else {
                true
            }
        };

        let resp = clients
            .backup
            .upload_backup_chunk(UploadBackupChunkRequest {
                upload_id: upload_id.clone(),
                chunk: buf,
                is_final,
            })
            .await?
            .into_inner();

        if is_first {
            upload_id = resp.upload_id;
        }
        total_bytes += n as u64;
    }

    // If the file was exactly a multiple of CHUNK_SIZE, we may not have sent is_final=true.
    // Send an empty final chunk to finalize.
    if !upload_id.is_empty() {
        let resp = clients
            .backup
            .upload_backup_chunk(UploadBackupChunkRequest {
                upload_id: upload_id.clone(),
                chunk: vec![],
                is_final: true,
            })
            .await?
            .into_inner();
        if resp.received_bytes as u64 != total_bytes {
            bail!(
                "upload size mismatch: sent {total_bytes} bytes but server received {}",
                resp.received_bytes
            );
        }
    }

    Ok(upload_id)
}

pub async fn restore(clients: &mut Clients, file_path: &str) -> Result<()> {
    eprintln!("Uploading backup...");
    let upload_id = upload_file(clients, file_path).await?;

    let preview = clients
        .backup
        .preview_backup(PreviewBackupRequest {
            upload_id: upload_id.clone(),
        })
        .await?
        .into_inner();

    println!("Backup preview:");
    println!("  Schema version: {}", preview.schema_version);
    println!(
        "  Exported at:    {}",
        format::timestamp(preview.exported_at.as_ref())
    );

    if !preview.table_counts.is_empty() {
        println!("  Tables:");
        let mut tables: Vec<_> = preview.table_counts.iter().collect();
        tables.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (table, count) in &tables {
            println!("    {table}: {count} rows");
        }
    }

    if !preview.source_names.is_empty() {
        println!("  Sources: {}", preview.source_names.join(", "));
    }

    if !preview.watermarks.is_empty() {
        println!("  Watermarks:");
        let mut marks: Vec<_> = preview.watermarks.iter().collect();
        marks.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (source, watermark) in &marks {
            println!("    {source}: {watermark}");
        }
    }

    if preview.workspace_file_count > 0 {
        let bytes = preview.workspace_total_bytes;
        #[allow(clippy::cast_precision_loss)]
        let human = if bytes >= 1_073_741_824 {
            format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
        } else if bytes >= 1_048_576 {
            format!("{:.1} MB", bytes as f64 / 1_048_576.0)
        } else if bytes >= 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.0)
        } else {
            format!("{bytes} B")
        };
        println!(
            "  Workspace files: {} files ({})",
            preview.workspace_file_count, human
        );
    }

    // Secret key validation
    if preview.secret_key_valid {
        println!("  Secret key:     Valid");
    } else {
        eprintln!("  Secret key:     INVALID - {}", preview.secret_key_warning);
        bail!(
            "Restore aborted: secret key mismatch. Use the same PS_SECRET_KEY that was used when the backup was created."
        );
    }

    // Integrity checksum validation
    if preview.checksum_valid {
        println!("  Checksum:       Valid");
    } else {
        eprintln!("  Checksum:       INVALID - {}", preview.checksum_warning);
        bail!(
            "Restore aborted: backup integrity check failed. The file may be corrupted or tampered with."
        );
    }

    // Confirm
    println!();
    eprint!("Restore this backup? [y/N] ");
    std::io::stderr().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") {
        bail!("Restore cancelled.");
    }

    let response = clients
        .backup
        .restore_backup(RestoreBackupRequest { upload_id })
        .await?
        .into_inner();

    println!("Restore complete.");
    if !response.generated_password.is_empty() {
        eprintln!(
            "  Generated admin password: {}",
            response.generated_password
        );
        eprintln!("  (change this password immediately via the web UI)");
    }

    if !response.tables_restored.is_empty() {
        println!("  Tables restored:");
        let mut tables: Vec<_> = response.tables_restored.iter().collect();
        tables.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (table, count) in &tables {
            println!("    {table}: {count} rows");
        }
    }

    Ok(())
}
