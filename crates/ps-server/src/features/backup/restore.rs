use std::collections::HashMap;

use ps_core::backup::validate_secret_key_canary;
use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::{RestoreBackupRequest, RestoreBackupResponse};
use rand::Rng as _;
use tonic::{Request, Response, Status, Streaming};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::common::{db_err, to_timestamp};
use crate::interceptor::AuthContext;

use super::BackupServiceImpl;
use super::generator::BackupJobStatus;
use super::preview::read_manifest_from_archive;

pub async fn restore_backup(
    svc: &BackupServiceImpl,
    request: Request<Streaming<RestoreBackupRequest>>,
) -> Result<Response<RestoreBackupResponse>, Status> {
    // On live (initialised) instances the interceptor will have attached an
    // AuthContext.  Verify the caller is an admin.  On uninitialised instances
    // there is no AuthContext, which is intentional.
    if let Some(ctx) = request.extensions().get::<AuthContext>()
        && ctx.role != ps_core::models::Role::Admin
    {
        return Err(Status::permission_denied("admin role required"));
    }

    let mut stream = request.into_inner();
    let tmp: tempfile::NamedTempFile =
        BackupServiceImpl::stream_to_tempfile(&mut stream, |r| r.chunk).await?;

    // --- Validate manifest before wiping any data ---
    let manifest = read_manifest_from_archive(tmp.path())?;

    if manifest.format_version < 2 {
        return Err(Status::failed_precondition(
            "v1 JSONL backups are not supported; use a matching Prism version",
        ));
    }

    // Validate secret key canary before wiping any data
    validate_secret_key_canary(&manifest.secret_key_canary, &svc.secret_key)
        .map_err(|e| Status::failed_precondition(e.to_string()))?;
    info!("secret key canary validated successfully");

    // --- Save archive to backups PVC for the restore Job ---
    let restore_id = Uuid::now_v7().to_string();
    let backups_path = svc
        .backups_path
        .as_ref()
        .ok_or_else(|| Status::internal("backups_path not configured — cannot run restore job"))?;

    let archive_dest = backups_path.join(format!("{restore_id}.ps-backup"));
    std::fs::copy(tmp.path(), &archive_dest).map_err(|e| {
        error!(error = %e, "failed to copy archive to backups PVC");
        Status::internal("failed to prepare restore")
    })?;
    info!(
        restore_id = %restore_id,
        path = %archive_dest.display(),
        "archive staged on backups PVC"
    );

    // --- Launch restore Job ---
    svc.generator.start_restore(&restore_id).await?;

    // --- Poll until the Job completes ---
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        match svc.generator.poll_status(&restore_id).await? {
            BackupJobStatus::Succeeded => break,
            BackupJobStatus::Failed(msg) => {
                // Clean up the staged archive
                let _ = std::fs::remove_file(&archive_dest);
                return Err(Status::internal(format!("restore failed: {msg}")));
            }
            BackupJobStatus::Running => {}
        }
    }

    info!("restore Job completed successfully");

    // --- Find/create admin user and generate session token ---
    let admin_user = svc
        .repos
        .auth
        .find_first_admin_user()
        .await
        .map_err(db_err)?;

    let (session_token, expires_at, generated_password) = if let Some(user_id) = admin_user {
        info!(user_id = %user_id, "restore complete, using restored admin user");
        let (token, exp) = create_restore_session(&svc.repos, user_id).await?;
        (token, exp, String::new())
    } else {
        // No users in the backup — create a fresh admin
        warn!("backup contained no users; creating emergency admin account");
        let password: String = rand::rng()
            .sample_iter(&rand::distr::Alphanumeric)
            .take(24)
            .map(char::from)
            .collect();
        let hash = ps_core::auth::hash_password(&password).map_err(|e| {
            error!(error = %e, "password hashing failed");
            Status::internal("internal error")
        })?;
        let user_id = Uuid::now_v7();
        svc.repos
            .auth
            .create_user(
                user_id,
                "admin",
                "Administrator",
                &hash,
                ps_core::models::Role::Admin,
            )
            .await
            .map_err(db_err)?;
        let (token, exp) = create_restore_session(&svc.repos, user_id).await?;
        (token, exp, password)
    };

    info!("restore complete");

    Ok(Response::new(RestoreBackupResponse {
        session_token,
        expires_at: Some(expires_at),
        tables_restored: HashMap::new(), // v2 doesn't track per-table counts
        generated_password,
    }))
}

/// Create a session for a user after restore, returning the raw token and expiry.
async fn create_restore_session(
    repos: &Repos,
    user_id: Uuid,
) -> Result<(String, prost_types::Timestamp), Status> {
    let raw_token = ps_core::auth::generate_token();
    let token_hash = ps_core::auth::hash_token(&raw_token);
    let session_id = Uuid::now_v7();
    let expires_at = time::OffsetDateTime::now_utc() + time::Duration::days(7);

    repos
        .auth
        .create_session(
            session_id,
            user_id,
            &token_hash,
            "browser",
            Some(expires_at),
            None,
        )
        .await
        .map_err(db_err)?;

    Ok((raw_token, to_timestamp(expires_at)))
}
