use std::io::Write;

use anyhow::{Result, bail};
use ps_proto::prism::v1::{
    PreviewBackupRequest, RestoreBackupRequest, auth_service_client::AuthServiceClient,
};
use tonic::transport::Channel;

use crate::client::AuthInterceptor;
use crate::format;

const CHUNK_SIZE: usize = 64 * 1024;

pub async fn restore(channel: &Channel, auth: &AuthInterceptor, file_path: &str) -> Result<()> {
    let data = tokio::fs::read(file_path).await?;
    let mut client = AuthServiceClient::with_interceptor(channel.clone(), auth.clone());

    // Preview the backup before restoring
    let chunks: Vec<PreviewBackupRequest> = data
        .chunks(CHUNK_SIZE)
        .map(|c| PreviewBackupRequest { chunk: c.to_vec() })
        .collect();

    let preview = client
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
        tables.sort_by_key(|(k, _)| (*k).clone());
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
        marks.sort_by_key(|(k, _)| (*k).clone());
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

    // Stream the restore
    let chunks: Vec<RestoreBackupRequest> = data
        .chunks(CHUNK_SIZE)
        .map(|c| RestoreBackupRequest { chunk: c.to_vec() })
        .collect();

    let response = client
        .restore_backup(tokio_stream::iter(chunks))
        .await?
        .into_inner();

    println!("Restore complete.");
    println!("  Session token: {}", response.session_token);

    if !response.tables_restored.is_empty() {
        println!("  Tables restored:");
        let mut tables: Vec<_> = response.tables_restored.iter().collect();
        tables.sort_by_key(|(k, _)| (*k).clone());
        for (table, count) in &tables {
            println!("    {table}: {count} rows");
        }
    }

    Ok(())
}
