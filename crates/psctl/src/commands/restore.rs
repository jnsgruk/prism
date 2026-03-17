use std::io::Write;

use anyhow::{Result, bail};
use ps_proto::prism::v1::{PreviewBackupRequest, RestoreBackupRequest};
use tokio::io::AsyncReadExt;

use crate::client::Clients;
use crate::format;

const CHUNK_SIZE: usize = 64 * 1024;

/// Stream a file from disk as gRPC request chunks, avoiding loading the
/// entire file into memory at once.
async fn stream_file<T>(file_path: &str, f: impl Fn(Vec<u8>) -> T) -> Result<Vec<T>> {
    let mut file = tokio::fs::File::open(file_path).await?;
    let mut chunks = Vec::new();
    loop {
        let mut buf = vec![0u8; CHUNK_SIZE];
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        buf.truncate(n);
        chunks.push(f(buf));
    }
    Ok(chunks)
}

pub async fn restore(clients: &mut Clients, file_path: &str) -> Result<()> {
    // Preview: stream from disk
    let chunks = stream_file(file_path, |chunk| PreviewBackupRequest { chunk }).await?;

    let preview = clients
        .auth
        .preview_backup(tokio_stream::iter(chunks))
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

    // Confirm
    println!();
    eprint!("Restore this backup? [y/N] ");
    std::io::stderr().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") {
        bail!("Restore cancelled.");
    }

    // Restore: stream from disk again (don't keep preview data in memory)
    let chunks = stream_file(file_path, |chunk| RestoreBackupRequest { chunk }).await?;

    let response = clients
        .auth
        .restore_backup(tokio_stream::iter(chunks))
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
