use std::collections::HashMap;
use std::io::Read as _;

use ps_core::backup::{BackupManifest, validate_secret_key_canary};
use ps_proto::canonical::prism::v1::{PreviewBackupRequest, PreviewBackupResponse};
use tonic::{Request, Response, Status, Streaming};
use tracing::error;

use crate::common::to_timestamp;
use crate::interceptor::AuthContext;

use super::BackupServiceImpl;

pub async fn preview_backup(
    svc: &BackupServiceImpl,
    request: Request<Streaming<PreviewBackupRequest>>,
) -> Result<Response<PreviewBackupResponse>, Status> {
    // On live (initialised) instances the interceptor will have attached an
    // AuthContext.  Verify the caller is an admin — viewers must not be able
    // to inspect backup metadata.  On uninitialised instances there is no
    // AuthContext, which is intentional.
    if let Some(ctx) = request.extensions().get::<AuthContext>()
        && ctx.role != ps_core::models::Role::Admin
    {
        return Err(Status::permission_denied("admin role required"));
    }

    let mut stream = request.into_inner();
    let tmp: tempfile::NamedTempFile =
        BackupServiceImpl::stream_to_tempfile(&mut stream, |r| r.chunk).await?;

    // Read manifest.json from the gzipped tar archive
    let manifest = read_manifest_from_archive(tmp.path())?;

    // Validate format version
    if manifest.format_version < 2 {
        return Err(Status::invalid_argument(
            "v1 JSONL backups are not supported; use a matching Prism version",
        ));
    }

    let exported_at = to_timestamp(manifest.exported_at);

    // Validate secret key canary
    let (secret_key_valid, secret_key_warning) =
        match validate_secret_key_canary(&manifest.secret_key_canary, &svc.secret_key) {
            Ok(()) => (true, String::new()),
            Err(e) => (false, e.to_string()),
        };

    Ok(Response::new(PreviewBackupResponse {
        schema_version: manifest.format_version,
        exported_at: Some(exported_at),
        table_counts: HashMap::new(),
        source_names: vec![],
        watermarks: HashMap::new(),
        workspace_file_count: manifest.workspace_file_count,
        workspace_total_bytes: manifest.workspace_total_bytes,
        secret_key_valid,
        secret_key_warning,
        checksum_valid: true, // pg_dump custom format has internal checksums
        checksum_warning: String::new(),
    }))
}

/// Read and parse `manifest.json` from a v2 `.ps-backup` gzipped tar archive.
#[allow(clippy::result_large_err)]
pub(super) fn read_manifest_from_archive(
    archive_path: &std::path::Path,
) -> Result<BackupManifest, Status> {
    let file = std::fs::File::open(archive_path).map_err(|e| {
        error!(error = %e, "failed to open backup archive");
        Status::internal("internal error")
    })?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    for entry in archive
        .entries()
        .map_err(|e| Status::invalid_argument(format!("invalid backup archive: {e}")))?
    {
        let mut entry =
            entry.map_err(|e| Status::invalid_argument(format!("invalid backup entry: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| Status::invalid_argument(format!("invalid entry path: {e}")))?;
        if path.to_string_lossy() == "manifest.json" {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| Status::invalid_argument(format!("failed to read manifest: {e}")))?;
            let manifest: BackupManifest = serde_json::from_slice(&buf)
                .map_err(|e| Status::invalid_argument(format!("invalid manifest JSON: {e}")))?;
            return Ok(manifest);
        }
    }

    Err(Status::invalid_argument(
        "backup archive does not contain manifest.json",
    ))
}
