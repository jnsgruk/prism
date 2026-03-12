use std::collections::HashMap;

use ps_core::auth::{generate_token, hash_password, hash_token, verify_password};
use ps_core::backup::BackupReader;
use ps_proto::prism::v1::auth_service_server::AuthService;
use ps_proto::prism::v1::{
    CompleteSetupRequest, CompleteSetupResponse, GetCurrentUserRequest, GetCurrentUserResponse,
    GetSetupStatusRequest, GetSetupStatusResponse, LoginRequest, LoginResponse, LogoutRequest,
    LogoutResponse, PreviewBackupRequest, PreviewBackupResponse, RestoreBackupRequest,
    RestoreBackupResponse,
};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status, Streaming};
use tracing::info;
use uuid::Uuid;

use crate::interceptor::AuthContext;

pub struct AuthServiceImpl {
    pool: PgPool,
}

impl AuthServiceImpl {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[allow(clippy::result_large_err)]
fn require_auth<T>(request: &Request<T>) -> Result<AuthContext, Status> {
    request
        .extensions()
        .get::<AuthContext>()
        .cloned()
        .ok_or_else(|| Status::unauthenticated("not authenticated"))
}

#[tonic::async_trait]
impl AuthService for AuthServiceImpl {
    async fn get_setup_status(
        &self,
        _request: Request<GetSetupStatusRequest>,
    ) -> Result<Response<GetSetupStatusResponse>, Status> {
        let exists = sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM auth.users)")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?
            .unwrap_or(false);

        Ok(Response::new(GetSetupStatusResponse {
            setup_complete: exists,
        }))
    }

    async fn complete_setup(
        &self,
        request: Request<CompleteSetupRequest>,
    ) -> Result<Response<CompleteSetupResponse>, Status> {
        let req = request.into_inner();

        if req.username.is_empty() || req.password.is_empty() || req.display_name.is_empty() {
            return Err(Status::invalid_argument(
                "username, display_name, and password are required",
            ));
        }

        let exists = sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM auth.users)")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?
            .unwrap_or(false);

        if exists {
            return Err(Status::failed_precondition("setup already complete"));
        }

        let password_hash = hash_password(&req.password)
            .map_err(|e| Status::internal(format!("password hashing failed: {e}")))?;

        let user_id = Uuid::now_v7();
        sqlx::query!(
            r#"
            INSERT INTO auth.users (id, username, display_name, password_hash, role)
            VALUES ($1, $2, $3, $4, 'admin')
            "#,
            user_id,
            req.username,
            req.display_name,
            password_hash,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to create user: {e}")))?;

        let raw_token = generate_token();
        let token_hash = hash_token(&raw_token);
        let session_id = Uuid::now_v7();
        let expires_at = OffsetDateTime::now_utc() + time::Duration::days(7);

        sqlx::query!(
            r#"
            INSERT INTO auth.sessions (id, user_id, token_hash, session_type, expires_at)
            VALUES ($1, $2, $3, 'browser', $4)
            "#,
            session_id,
            user_id,
            token_hash,
            expires_at,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to create session: {e}")))?;

        info!(user_id = %user_id, username = %req.username, "initial admin user created");

        Ok(Response::new(CompleteSetupResponse {
            session_token: raw_token,
        }))
    }

    async fn login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        let req = request.into_inner();

        let user = sqlx::query!(
            r#"
            SELECT id, password_hash, is_active
            FROM auth.users
            WHERE username = $1
            "#,
            req.username,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("database error: {e}")))?
        .ok_or_else(|| Status::unauthenticated("invalid credentials"))?;

        if !user.is_active {
            return Err(Status::unauthenticated("account is disabled"));
        }

        verify_password(&req.password, &user.password_hash)
            .map_err(|_| Status::unauthenticated("invalid credentials"))?;

        let raw_token = generate_token();
        let token_hash = hash_token(&raw_token);
        let session_id = Uuid::now_v7();
        let expires_at = OffsetDateTime::now_utc() + time::Duration::days(7);

        sqlx::query!(
            r#"
            INSERT INTO auth.sessions (id, user_id, token_hash, session_type, expires_at)
            VALUES ($1, $2, $3, 'browser', $4)
            "#,
            session_id,
            user.id,
            token_hash,
            expires_at,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to create session: {e}")))?;

        let timestamp = prost_types::Timestamp {
            seconds: expires_at.unix_timestamp(),
            nanos: 0,
        };

        Ok(Response::new(LoginResponse {
            session_token: raw_token,
            expires_at: Some(timestamp),
        }))
    }

    async fn logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> Result<Response<LogoutResponse>, Status> {
        let ctx = require_auth(&request)?;

        sqlx::query!("DELETE FROM auth.sessions WHERE id = $1", ctx.session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?;

        Ok(Response::new(LogoutResponse {}))
    }

    async fn get_current_user(
        &self,
        request: Request<GetCurrentUserRequest>,
    ) -> Result<Response<GetCurrentUserResponse>, Status> {
        let ctx = require_auth(&request)?;

        Ok(Response::new(GetCurrentUserResponse {
            user_id: ctx.user_id.to_string(),
            username: ctx.username,
            display_name: ctx.display_name,
            role: ctx.role,
        }))
    }

    async fn preview_backup(
        &self,
        request: Request<Streaming<PreviewBackupRequest>>,
    ) -> Result<Response<PreviewBackupResponse>, Status> {
        // Collect all chunks from the client stream
        let mut stream = request.into_inner();
        let mut data = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            data.extend_from_slice(&chunk.chunk);
        }

        // Parse the backup archive
        let mut reader = BackupReader::new(data.as_slice());
        let manifest = reader
            .read_manifest()
            .map_err(|e| Status::invalid_argument(format!("invalid backup: {e}")))?;

        let exported_at = prost_types::Timestamp {
            seconds: manifest.exported_at.unix_timestamp(),
            nanos: 0,
        };

        // Convert table_counts from HashMap<String, i32> to proto map
        let table_counts: HashMap<String, i32> = manifest.table_counts;

        Ok(Response::new(PreviewBackupResponse {
            schema_version: manifest.schema_version,
            exported_at: Some(exported_at),
            table_counts,
            source_names: vec![],
            watermarks: HashMap::new(),
        }))
    }

    async fn restore_backup(
        &self,
        request: Request<Streaming<RestoreBackupRequest>>,
    ) -> Result<Response<RestoreBackupResponse>, Status> {
        // Ensure no users exist (restore only works on fresh instance)
        let exists = sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM auth.users)")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("database error: {e}")))?
            .unwrap_or(false);

        if exists {
            return Err(Status::failed_precondition(
                "restore only allowed on fresh instance with no users",
            ));
        }

        // Collect all chunks from the client stream
        let mut stream = request.into_inner();
        let mut data = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            data.extend_from_slice(&chunk.chunk);
        }

        // Parse backup
        let mut reader = BackupReader::new(data.as_slice());
        let manifest = reader
            .read_manifest()
            .map_err(|e| Status::invalid_argument(format!("invalid backup: {e}")))?;

        info!(
            schema_version = manifest.schema_version,
            tables = ?manifest.table_counts,
            "restoring backup"
        );

        // For now, return the manifest info — full table restore will be
        // implemented when we have concrete table schemas to import into.
        let tables_restored: HashMap<String, i32> = manifest.table_counts;

        // Create admin user for the restored instance
        let password_hash = hash_password("changeme")
            .map_err(|e| Status::internal(format!("password hashing failed: {e}")))?;

        let user_id = Uuid::now_v7();
        sqlx::query!(
            r#"
            INSERT INTO auth.users (id, username, display_name, password_hash, role)
            VALUES ($1, 'admin', 'Administrator', $2, 'admin')
            "#,
            user_id,
            password_hash,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to create admin user: {e}")))?;

        let raw_token = generate_token();
        let token_hash = hash_token(&raw_token);
        let session_id = Uuid::now_v7();
        let expires_at = OffsetDateTime::now_utc() + time::Duration::days(7);

        sqlx::query!(
            r#"
            INSERT INTO auth.sessions (id, user_id, token_hash, session_type, expires_at)
            VALUES ($1, $2, $3, 'browser', $4)
            "#,
            session_id,
            user_id,
            token_hash,
            expires_at,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to create session: {e}")))?;

        let timestamp = prost_types::Timestamp {
            seconds: expires_at.unix_timestamp(),
            nanos: 0,
        };

        info!("backup restored, admin user created");

        Ok(Response::new(RestoreBackupResponse {
            session_token: raw_token,
            expires_at: Some(timestamp),
            tables_restored,
        }))
    }
}
