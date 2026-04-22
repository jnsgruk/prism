use ps_core::auth::{generate_token, hash_token, verify_password};
use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::auth_service_server::AuthService;
use ps_proto::canonical::prism::v1::{
    CompleteSetupRequest, CompleteSetupResponse, GetCurrentUserRequest, GetCurrentUserResponse,
    GetSetupStatusRequest, GetSetupStatusResponse, LoginRequest, LoginResponse, LogoutRequest,
    LogoutResponse,
};
use time::OffsetDateTime;
use tonic::{Request, Response, Status};
use tracing::{error, info};
use uuid::Uuid;

use crate::common::{db_err, require_auth, to_timestamp};

pub struct AuthServiceImpl {
    repos: Repos,
}

impl AuthServiceImpl {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    /// Create a session for a user, returning the raw token and expiry timestamp.
    async fn create_user_session(
        &self,
        user_id: Uuid,
        session_type: &str,
    ) -> Result<(String, prost_types::Timestamp), Status> {
        let raw_token = generate_token();
        let token_hash = hash_token(&raw_token);
        let session_id = Uuid::now_v7();
        let expires_at = OffsetDateTime::now_utc() + time::Duration::days(7);

        self.repos
            .auth
            .create_session(
                session_id,
                user_id,
                &token_hash,
                session_type,
                Some(expires_at),
                None,
            )
            .await
            .map_err(db_err)?;

        Ok((raw_token, to_timestamp(expires_at)))
    }
}

#[tonic::async_trait]
impl AuthService for AuthServiceImpl {
    async fn get_setup_status(
        &self,
        _request: Request<GetSetupStatusRequest>,
    ) -> Result<Response<GetSetupStatusResponse>, Status> {
        let exists = self.repos.auth.any_users_exist().await.map_err(db_err)?;

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

        let exists = self.repos.auth.any_users_exist().await.map_err(db_err)?;

        if exists {
            return Err(Status::failed_precondition("setup already complete"));
        }

        let password_hash = ps_core::auth::hash_password(&req.password).map_err(|e| {
            error!(error = %e, "password hashing failed");
            Status::internal("internal error")
        })?;

        let user_id = Uuid::now_v7();
        self.repos
            .auth
            .create_user(
                user_id,
                &req.username,
                &req.display_name,
                &password_hash,
                ps_core::models::Role::Admin,
            )
            .await
            .map_err(db_err)?;

        let (session_token, _) = self.create_user_session(user_id, "browser").await?;

        info!(user_id = %user_id, username = %req.username, "initial admin user created");

        Ok(Response::new(CompleteSetupResponse { session_token }))
    }

    async fn login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        let req = request.into_inner();

        let user = self
            .repos
            .auth
            .find_user_by_username(&req.username)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::unauthenticated("invalid credentials"))?;

        if !user.is_active {
            return Err(Status::unauthenticated("account is disabled"));
        }

        verify_password(&req.password, &user.password_hash)
            .map_err(|_| Status::unauthenticated("invalid credentials"))?;

        let (session_token, expires_at) = self.create_user_session(user.id, "browser").await?;

        Ok(Response::new(LoginResponse {
            session_token,
            expires_at: Some(expires_at),
        }))
    }

    async fn logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> Result<Response<LogoutResponse>, Status> {
        let ctx = require_auth(&request)?;

        self.repos
            .auth
            .delete_session(ctx.session_id)
            .await
            .map_err(db_err)?;

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
            role: ctx.role.to_string(),
        }))
    }
}
