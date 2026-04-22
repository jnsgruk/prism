use std::io::Write;

use anyhow::{Result, bail};
use async_stream::stream;
use ps_proto::canonical::prism::v1::{PreviewBackupRequest, RestoreBackupRequest};
use tokio::io::AsyncReadExt;

use crate::client::Clients;
use crate::format;

const CHUNK_SIZE: usize = 256 * 1024;

/// Return a lazy async stream that reads `file_path` in `CHUNK_SIZE` chunks,
/// mapping each chunk with `f`. Only one chunk is held in memory at a time.
fn file_stream<T: 'static>(
    file_path: String,
    f: impl Fn(Vec<u8>) -> T + 'static,
) -> impl tokio_stream::Stream<Item = T> {
    stream! {
        match tokio::fs::File::open(&file_path).await {
            Err(_) => return,
            Ok(mut file) => {
                loop {
                    let mut buf = vec![0u8; CHUNK_SIZE];
                    match file.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buf.truncate(n);
                            yield f(buf);
                        }
                    }
                }
            }
        }
    }
}

pub async fn restore(clients: &mut Clients, file_path: &str) -> Result<()> {
    // Preview: stream from disk lazily — never more than one chunk in memory
    let preview_stream = file_stream(file_path.to_string(), |chunk| PreviewBackupRequest {
        chunk,
    });

    let preview = clients
        .backup
        .preview_backup(preview_stream)
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

    // Restore: stream from disk lazily again — file is re-read, not buffered
    let restore_stream = file_stream(file_path.to_string(), |chunk| RestoreBackupRequest {
        chunk,
    });

    let response = clients
        .backup
        .restore_backup(restore_stream)
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
