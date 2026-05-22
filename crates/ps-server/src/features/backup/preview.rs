use std::collections::HashMap;
use std::io::Read as _;

use ps_core::backup::{BackupManifest, validate_secret_key_canary};
use ps_proto::canonical::prism::v1::PreviewBackupRequest;
use tonic::{Request, Response, Status};
use tracing::error;

use crate::common::to_timestamp;
use crate::interceptor::AuthContext;

use super::BackupServiceImpl;

pub async fn preview_backup(
    svc: &BackupServiceImpl,
    request: Request<PreviewBackupRequest>,
) -> Result<Response<ps_proto::canonical::prism::v1::PreviewBackupResponse>, Status> {
    if let Some(ctx) = request.extensions().get::<AuthContext>()
        && ctx.role != ps_core::models::Role::Admin
    {
        return Err(Status::permission_denied("admin role required"));
    }

    let backups_path = svc
        .backups_path
        .as_ref()
        .ok_or_else(|| Status::internal("backups_path not configured"))?;

    let inner = request.into_inner();
    let archive_path = super::upload::get_uploaded_file_path(backups_path, &inner.upload_id)?;

    let manifest = read_manifest_from_archive(&archive_path)?;

    if manifest.format_version < 2 {
        return Err(Status::invalid_argument(
            "v1 JSONL backups are not supported; use a matching Prism version",
        ));
    }

    let exported_at = to_timestamp(manifest.exported_at);

    let (secret_key_valid, secret_key_warning) =
        match validate_secret_key_canary(&manifest.secret_key_canary, &svc.secret_key) {
            Ok(()) => (true, String::new()),
            Err(e) => (false, e.to_string()),
        };

    Ok(Response::new(
        ps_proto::canonical::prism::v1::PreviewBackupResponse {
            schema_version: manifest.format_version,
            exported_at: Some(exported_at),
            table_counts: HashMap::new(),
            source_names: vec![],
            watermarks: HashMap::new(),
            workspace_file_count: manifest.workspace_file_count,
            workspace_total_bytes: manifest.workspace_total_bytes,
            secret_key_valid,
            secret_key_warning,
            checksum_valid: true,
            checksum_warning: String::new(),
        },
    ))
}

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
