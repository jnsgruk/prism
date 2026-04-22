mod cancel;
mod create;
pub mod generator;
mod preview;
mod restore;

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::backup_service_server::BackupService;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status, Streaming};
use tracing::error;
use zeroize::Zeroizing;

pub use generator::{BackupGenerator, BackupJobStatus};

pub struct BackupServiceImpl {
    repos: Repos,
    secret_key: Zeroizing<[u8; 32]>,
    backups_path: Option<PathBuf>,
    generator: Arc<dyn BackupGenerator>,
}

impl BackupServiceImpl {
    pub fn new(
        repos: Repos,
        secret_key: Zeroizing<[u8; 32]>,
        backups_path: Option<PathBuf>,
        generator: Arc<dyn BackupGenerator>,
    ) -> Self {
        Self {
            repos,
            secret_key,
            backups_path,
            generator,
        }
    }

    /// Collect all bytes from a client-streaming gRPC request into a temp file.
    ///
    /// Returns a `tempfile::NamedTempFile` containing the full upload. Using a
    /// temp file avoids holding the entire backup in memory and removes the old
    /// hard-coded 100 MB cap.
    async fn stream_to_tempfile<T, F>(
        stream: &mut Streaming<T>,
        extract_chunk: F,
    ) -> Result<tempfile::NamedTempFile, Status>
    where
        F: Fn(T) -> Vec<u8>,
    {
        let mut tmp = tempfile::NamedTempFile::new().map_err(|e| {
            error!(error = %e, "failed to create temp file for backup upload");
            Status::internal("internal error")
        })?;

        while let Some(msg) = stream.next().await {
            let msg = msg?;
            let chunk = extract_chunk(msg);
            tmp.write_all(&chunk).map_err(|e| {
                error!(error = %e, "failed to write backup chunk to temp file");
                Status::internal("internal error")
            })?;
        }

        tmp.flush().map_err(|e| {
            error!(error = %e, "failed to flush backup temp file");
            Status::internal("internal error")
        })?;

        Ok(tmp)
    }
}

#[tonic::async_trait]
impl BackupService for BackupServiceImpl {
    type CreateBackupStream = create::CreateBackupStream;

    async fn create_backup(
        &self,
        request: Request<ps_proto::canonical::prism::v1::CreateBackupRequest>,
    ) -> Result<Response<Self::CreateBackupStream>, Status> {
        create::create_backup(self, request).await
    }

    async fn preview_backup(
        &self,
        request: Request<Streaming<ps_proto::canonical::prism::v1::PreviewBackupRequest>>,
    ) -> Result<Response<ps_proto::canonical::prism::v1::PreviewBackupResponse>, Status> {
        preview::preview_backup(self, request).await
    }

    async fn restore_backup(
        &self,
        request: Request<Streaming<ps_proto::canonical::prism::v1::RestoreBackupRequest>>,
    ) -> Result<Response<ps_proto::canonical::prism::v1::RestoreBackupResponse>, Status> {
        restore::restore_backup(self, request).await
    }

    async fn cancel_backup(
        &self,
        request: Request<ps_proto::canonical::prism::v1::CancelBackupRequest>,
    ) -> Result<Response<ps_proto::canonical::prism::v1::CancelBackupResponse>, Status> {
        cancel::cancel_backup(self, request).await
    }
}
