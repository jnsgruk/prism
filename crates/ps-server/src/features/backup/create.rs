use std::pin::Pin;

use ps_proto::canonical::prism::v1::{
    BackupProgress, CreateBackupRequest, CreateBackupResponse, create_backup_response::Payload,
};
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};

use crate::common::require_admin;

use super::BackupServiceImpl;
use super::generator::BackupJobStatus;

pub type CreateBackupStream =
    Pin<Box<dyn Stream<Item = Result<CreateBackupResponse, Status>> + Send>>;

pub async fn create_backup(
    svc: &BackupServiceImpl,
    request: Request<CreateBackupRequest>,
) -> Result<Response<CreateBackupStream>, Status> {
    let _ctx = require_admin(&request)?;
    let req = request.into_inner();

    // --- Concurrent backup guard ---
    let active = svc.generator.is_backup_active().await?;
    if active {
        if req.force {
            svc.generator.force_cancel().await?;
            warn!("force-cancelled existing backup job(s)");
        } else {
            return Err(Status::already_exists(
                "a backup is already in progress (use --force to override)",
            ));
        }
    }

    let backup_id = uuid::Uuid::now_v7().to_string();
    let generator = svc.generator.clone();
    let backups_path = svc.backups_path.clone();

    let stream = async_stream::try_stream! {
        // 1. Create K8s Job (or run pg_dump directly in tests)
        generator.start_backup(&backup_id, req.exclude_workspaces).await?;
        info!(backup_id = %backup_id, "backup started");

        // 2. Poll for completion
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            match generator.poll_status(&backup_id).await? {
                BackupJobStatus::Running => {
                    yield CreateBackupResponse {
                        payload: Some(Payload::Progress(BackupProgress {
                            phase: "generating".into(),
                        })),
                    };
                }
                BackupJobStatus::Succeeded => break,
                BackupJobStatus::Failed(msg) => {
                    Err(Status::internal(format!("backup failed: {msg}")))?;
                }
            }
        }

        // 3. Stream the backup file from backups PVC
        let file_path = backups_path
            .as_ref()
            .ok_or_else(|| Status::internal("BACKUPS_PATH not configured"))?
            .join(format!("{backup_id}.ps-backup"));

        let mut reader = tokio::fs::File::open(&file_path).await.map_err(|e| {
            error!(error = %e, "failed to open backup file for streaming");
            Status::internal("internal error")
        })?;

        let mut chunk_buf = vec![0u8; 256 * 1024];
        loop {
            let n = tokio::io::AsyncReadExt::read(&mut reader, &mut chunk_buf).await.map_err(|e| {
                error!(error = %e, "failed to read backup file");
                Status::internal("internal error")
            })?;
            if n == 0 { break; }
            let chunk = chunk_buf.get(..n).ok_or_else(|| {
                Status::internal("internal error")
            })?.to_vec();
            yield CreateBackupResponse {
                payload: Some(Payload::Chunk(chunk)),
            };
        }

        // 4. Cleanup
        if let Err(e) = tokio::fs::remove_file(&file_path).await {
            tracing::warn!(error = %e, "failed to delete backup file after streaming");
        }
    };

    Ok(Response::new(Box::pin(stream)))
}
