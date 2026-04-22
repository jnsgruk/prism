use std::path::PathBuf;

use ps_core::auth::{generate_token, hash_token};
use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::admin_service_server::AdminService;
use ps_proto::canonical::prism::v1::{
    ApiTokenInfo, CreateApiTokenRequest, CreateApiTokenResponse, GetSystemInfoRequest,
    GetSystemInfoResponse, ListApiTokensRequest, ListApiTokensResponse, ResetDataRequest,
    ResetDataResponse, RevokeApiTokenRequest, RevokeApiTokenResponse,
};
use tonic::{Request, Response, Status};
use tracing::info;
use uuid::Uuid;

use crate::common::{db_err, require_admin, require_auth, to_timestamp};

pub struct AdminServiceImpl {
    repos: Repos,
    workspaces_path: Option<PathBuf>,
    /// Configured PVC capacity in bytes (from `WORKSPACES_CAPACITY_BYTES` env var).
    /// Falls back to `statvfs` if not set, which may report host filesystem size
    /// on Docker Desktop with hostpath provisioner.
    workspaces_capacity_bytes: Option<i64>,
}

impl AdminServiceImpl {
    pub fn new(
        repos: Repos,
        workspaces_path: Option<PathBuf>,
        workspaces_capacity_bytes: Option<i64>,
    ) -> Self {
        Self {
            repos,
            workspaces_path,
            workspaces_capacity_bytes,
        }
    }
}

#[tonic::async_trait]
impl AdminService for AdminServiceImpl {
    async fn create_api_token(
        &self,
        request: Request<CreateApiTokenRequest>,
    ) -> Result<Response<CreateApiTokenResponse>, Status> {
        let ctx = require_admin(&request)?;
        let req = request.into_inner();

        if req.name.is_empty() {
            return Err(Status::invalid_argument("token name is required"));
        }

        let raw_token = generate_token();
        let token_hash = hash_token(&raw_token);
        let token_id = Uuid::now_v7();
        let user_id = ctx.user_id;

        self.repos
            .auth
            .create_session(
                token_id,
                user_id,
                &token_hash,
                "api_token",
                None,
                Some(&req.name),
            )
            .await
            .map_err(db_err)?;

        info!(token_id = %token_id, name = %req.name, "API token created");

        Ok(Response::new(CreateApiTokenResponse {
            token_id: token_id.to_string(),
            token: raw_token,
            name: req.name,
        }))
    }

    async fn list_api_tokens(
        &self,
        request: Request<ListApiTokensRequest>,
    ) -> Result<Response<ListApiTokensResponse>, Status> {
        let ctx = require_auth(&request)?;

        let tokens = self
            .repos
            .auth
            .list_api_tokens(ctx.user_id)
            .await
            .map_err(db_err)?;

        let token_infos = tokens
            .into_iter()
            .map(|t| ApiTokenInfo {
                token_id: t.id.to_string(),
                name: t.token_name.unwrap_or_default(),
                created_at: Some(to_timestamp(t.created_at)),
                last_used_at: Some(to_timestamp(t.last_active_at)),
            })
            .collect();

        Ok(Response::new(ListApiTokensResponse {
            tokens: token_infos,
        }))
    }

    async fn revoke_api_token(
        &self,
        request: Request<RevokeApiTokenRequest>,
    ) -> Result<Response<RevokeApiTokenResponse>, Status> {
        let ctx = require_auth(&request)?;
        let req = request.into_inner();

        let token_id: Uuid = req
            .token_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid token_id format"))?;

        let deleted = self
            .repos
            .auth
            .delete_api_token(token_id, ctx.user_id)
            .await
            .map_err(db_err)?;

        if !deleted {
            return Err(Status::not_found("token not found"));
        }

        info!(token_id = %token_id, "API token revoked");

        Ok(Response::new(RevokeApiTokenResponse {}))
    }

    async fn reset_data(
        &self,
        request: Request<ResetDataRequest>,
    ) -> Result<Response<ResetDataResponse>, Status> {
        let ctx = require_admin(&request)?;
        let req = request.into_inner();

        if !req.confirm {
            return Err(Status::invalid_argument(
                "confirm must be true to reset data",
            ));
        }

        info!(user = %ctx.username, "resetting all ingested data");

        let contributions_deleted = self.repos.activity.reset_all().await.map_err(db_err)?;

        let (people_deleted, teams_deleted) = self.repos.org.reset_all().await.map_err(db_err)?;

        #[allow(clippy::cast_possible_truncation)]
        let resp = ResetDataResponse {
            people_deleted: people_deleted as i32,
            teams_deleted: teams_deleted as i32,
            contributions_deleted: contributions_deleted as i32,
        };

        info!(
            people = people_deleted,
            teams = teams_deleted,
            contributions = contributions_deleted,
            "data reset complete"
        );

        Ok(Response::new(resp))
    }

    async fn get_system_info(
        &self,
        request: Request<GetSystemInfoRequest>,
    ) -> Result<Response<GetSystemInfoResponse>, Status> {
        let _ctx = require_admin(&request)?;

        let database_size_bytes = self
            .repos
            .config
            .database_size_bytes()
            .await
            .map_err(db_err)?;

        let (workspace_used_bytes, workspace_total_bytes) =
            if let Some(ref path) = self.workspaces_path {
                let used = dir_size_bytes(path).await;
                let total = self
                    .workspaces_capacity_bytes
                    .unwrap_or_else(|| statvfs_total_bytes(path).unwrap_or(0));
                (used, total)
            } else {
                (0, 0)
            };

        Ok(Response::new(GetSystemInfoResponse {
            database_size_bytes,
            workspace_used_bytes,
            workspace_total_bytes,
        }))
    }
}

/// Walk a directory tree and sum file sizes. Returns 0 on error.
async fn dir_size_bytes(path: &std::path::Path) -> i64 {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || dir_size_sync(&path))
        .await
        .unwrap_or(0)
}

fn dir_size_sync(path: &std::path::Path) -> i64 {
    let mut total: u64 = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    #[allow(clippy::cast_possible_wrap)]
    let result = total as i64;
    result
}

/// Get total filesystem capacity via `statvfs`. Used as fallback when
/// `WORKSPACES_CAPACITY_BYTES` is not set.
fn statvfs_total_bytes(path: &std::path::Path) -> Option<i64> {
    let stat = nix::sys::statvfs::statvfs(path).ok()?;
    #[allow(clippy::cast_possible_wrap)]
    let total = stat.blocks() as i64 * stat.fragment_size() as i64;
    Some(total)
}
