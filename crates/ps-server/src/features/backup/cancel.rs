use ps_proto::canonical::prism::v1::{CancelBackupRequest, CancelBackupResponse};
use tonic::{Request, Response, Status};

use crate::common::require_admin;

use super::BackupServiceImpl;

pub async fn cancel_backup(
    svc: &BackupServiceImpl,
    request: Request<CancelBackupRequest>,
) -> Result<Response<CancelBackupResponse>, Status> {
    let _ctx = require_admin(&request)?;

    let cancelled = svc.generator.cancel_backup().await?;

    Ok(Response::new(CancelBackupResponse { cancelled }))
}
