use std::collections::BTreeMap;
use std::pin::Pin;

use ps_core::auth::{generate_token, hash_token};
use ps_core::backup::{BackupManifest, BackupWriter};
use ps_core::repo::Repos;
use ps_proto::prism::v1::admin_service_server::AdminService;
use ps_proto::prism::v1::{
    ApiTokenInfo, CreateApiTokenRequest, CreateApiTokenResponse, CreateBackupRequest,
    CreateBackupResponse, ListApiTokensRequest, ListApiTokensResponse, ResetDataRequest,
    ResetDataResponse, RevokeApiTokenRequest, RevokeApiTokenResponse,
};
use time::OffsetDateTime;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::info;
use uuid::Uuid;

use super::common::{backup_err, db_err, require_admin, require_auth, to_timestamp};

pub struct AdminServiceImpl {
    repos: Repos,
}

impl AdminServiceImpl {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }
}

#[tonic::async_trait]
impl AdminService for AdminServiceImpl {
    type CreateBackupStream =
        Pin<Box<dyn Stream<Item = Result<CreateBackupResponse, Status>> + Send>>;

    async fn create_backup(
        &self,
        request: Request<CreateBackupRequest>,
    ) -> Result<Response<Self::CreateBackupStream>, Status> {
        let _ctx = require_admin(&request)?;

        let repos = self.repos.clone();

        let stream = async_stream::try_stream! {
            let mut buf = Vec::new();

            // Gather table counts in parallel
            let (source_count, people_count, team_count, user_count) = tokio::try_join!(
                async { repos.config.count_sources().await.map_err(db_err) },
                async { repos.org.count_people().await.map_err(db_err) },
                async { repos.org.count_teams().await.map_err(db_err) },
                async { repos.auth.count_users().await.map_err(db_err) },
            )?;

            let mut table_counts = BTreeMap::new();
            #[allow(clippy::cast_possible_truncation)]
            {
                table_counts.insert("source_configs".into(), source_count as i32);
                table_counts.insert("people".into(), people_count as i32);
                table_counts.insert("teams".into(), team_count as i32);
                table_counts.insert("users".into(), user_count as i32);
            }

            let manifest = BackupManifest {
                schema_version: 5,
                exported_at: OffsetDateTime::now_utc(),
                table_counts,
                app_version: env!("CARGO_PKG_VERSION").into(),
            };

            let mut writer = BackupWriter::new(&mut buf);
            writer.write_manifest(&manifest)
                .map_err(backup_err)?;

            // Export source configs
            let source_rows = repos.config.export_sources().await.map_err(db_err)?;

            writer.write_table("source_configs", &source_rows)
                .map_err(backup_err)?;

            let people_rows = repos.org.export_people().await.map_err(db_err)?;
            writer.write_table("people", &people_rows)
                .map_err(backup_err)?;

            let team_rows = repos.org.export_teams().await.map_err(db_err)?;
            writer.write_table("teams", &team_rows)
                .map_err(backup_err)?;

            let user_rows = repos.auth.export_users().await.map_err(db_err)?;
            writer.write_table("users", &user_rows)
                .map_err(backup_err)?;

            writer.finish()
                .map_err(backup_err)?;

            info!(size_bytes = buf.len(), "backup created");

            // Stream the backup in 64KB chunks
            for chunk in buf.chunks(65536) {
                yield CreateBackupResponse {
                    chunk: chunk.to_vec(),
                };
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    async fn create_api_token(
        &self,
        request: Request<CreateApiTokenRequest>,
    ) -> Result<Response<CreateApiTokenResponse>, Status> {
        let _ctx = require_admin(&request)?;
        let req = request.into_inner();

        if req.name.is_empty() {
            return Err(Status::invalid_argument("token name is required"));
        }

        let raw_token = generate_token();
        let token_hash = hash_token(&raw_token);
        let token_id = Uuid::now_v7();
        let user_id = _ctx.user_id;

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
            .map_err(|e| Status::internal(format!("failed to create token: {e}")))?;

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
}
