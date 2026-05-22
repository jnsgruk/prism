mod cancel;
mod create;
pub mod generator;
mod preview;
mod restore;
mod upload;

use std::path::PathBuf;
use std::sync::Arc;

use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::backup_service_server::BackupService;
use tonic::{Request, Response, Status};
use zeroize::Zeroizing;

pub use generator::{BackupGenerator, BackupJobStatus};

/// Hook invoked after a successful restore to reload in-memory state
/// (e.g. AI provider keys) from the freshly-restored database.
pub type PostRestoreHook =
    Arc<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

pub struct BackupServiceImpl {
    repos: Repos,
    secret_key: Zeroizing<[u8; 32]>,
    backups_path: Option<PathBuf>,
    generator: Arc<dyn BackupGenerator>,
    post_restore_hook: Option<PostRestoreHook>,
}

impl BackupServiceImpl {
    pub fn new(
        repos: Repos,
        secret_key: Zeroizing<[u8; 32]>,
        backups_path: Option<PathBuf>,
        generator: Arc<dyn BackupGenerator>,
        post_restore_hook: Option<PostRestoreHook>,
    ) -> Self {
        Self {
            repos,
            secret_key,
            backups_path,
            generator,
            post_restore_hook,
        }
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

    async fn upload_backup_chunk(
        &self,
        request: Request<ps_proto::canonical::prism::v1::UploadBackupChunkRequest>,
    ) -> Result<Response<ps_proto::canonical::prism::v1::UploadBackupChunkResponse>, Status> {
        upload::upload_backup_chunk(self, request).await
    }

    async fn preview_backup(
        &self,
        request: Request<ps_proto::canonical::prism::v1::PreviewBackupRequest>,
    ) -> Result<Response<ps_proto::canonical::prism::v1::PreviewBackupResponse>, Status> {
        preview::preview_backup(self, request).await
    }

    async fn restore_backup(
        &self,
        request: Request<ps_proto::canonical::prism::v1::RestoreBackupRequest>,
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
