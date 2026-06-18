use std::io::Write as _;
use std::path::PathBuf;

use ps_proto::canonical::prism::v1::{UploadBackupChunkRequest, UploadBackupChunkResponse};
use tonic::{Request, Response, Status};
use tracing::error;
use uuid::Uuid;

use crate::interceptor::AuthContext;

use super::BackupServiceImpl;

pub async fn upload_backup_chunk(
    svc: &BackupServiceImpl,
    request: Request<UploadBackupChunkRequest>,
) -> Result<Response<UploadBackupChunkResponse>, Status> {
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

    let (upload_id, mut file) = if inner.upload_id.is_empty() {
        let id = Uuid::now_v7().to_string();
        let uploads_dir = backups_path.join("uploads");
        std::fs::create_dir_all(&uploads_dir).map_err(|e| {
            error!(error = %e, "failed to create uploads directory");
            Status::internal("internal error")
        })?;
        let path = upload_path(backups_path, &id);
        let f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|e| {
                error!(error = %e, "failed to create upload file");
                Status::internal("internal error")
            })?;
        (id, f)
    } else {
        validate_upload_id(&inner.upload_id)?;
        let path = upload_path(backups_path, &inner.upload_id);
        if !path.exists() {
            return Err(Status::not_found("upload not found"));
        }
        let f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| {
                error!(error = %e, "failed to open upload file for append");
                Status::internal("internal error")
            })?;
        (inner.upload_id.clone(), f)
    };

    file.write_all(&inner.chunk).map_err(|e| {
        error!(error = %e, "failed to write chunk to upload file");
        Status::internal("internal error")
    })?;

    #[allow(clippy::cast_possible_wrap)]
    let received_bytes = file.metadata().map_or(0, |m| m.len()) as i64;

    if inner.is_final {
        file.flush().map_err(|e| {
            error!(error = %e, "failed to flush upload file");
            Status::internal("internal error")
        })?;

        let staging_path = upload_path(backups_path, &upload_id);
        let final_path = uploaded_path(backups_path, &upload_id);
        std::fs::rename(&staging_path, &final_path).map_err(|e| {
            error!(error = %e, "failed to finalize upload");
            Status::internal("internal error")
        })?;
    }

    Ok(Response::new(UploadBackupChunkResponse {
        upload_id,
        received_bytes,
    }))
}

fn upload_path(backups_path: &std::path::Path, upload_id: &str) -> PathBuf {
    backups_path
        .join("uploads")
        .join(format!("{upload_id}.uploading"))
}

fn uploaded_path(backups_path: &std::path::Path, upload_id: &str) -> PathBuf {
    backups_path
        .join("uploads")
        .join(format!("{upload_id}.uploaded"))
}

#[allow(clippy::result_large_err)]
pub fn get_uploaded_file_path(
    backups_path: &std::path::Path,
    upload_id: &str,
) -> Result<PathBuf, Status> {
    validate_upload_id(upload_id)?;
    let path = uploaded_path(backups_path, upload_id);
    if !path.exists() {
        return Err(Status::not_found("upload not found or not finalized"));
    }
    Ok(path)
}

#[allow(clippy::result_large_err)]
fn validate_upload_id(id: &str) -> Result<(), Status> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| Status::invalid_argument("invalid upload_id"))
}
