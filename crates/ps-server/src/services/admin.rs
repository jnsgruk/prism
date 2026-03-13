use std::collections::HashMap;
use std::pin::Pin;

use ps_core::auth::{generate_token, hash_token};
use ps_core::backup::{BackupManifest, BackupWriter};
use ps_core::repo::Repos;
use ps_proto::prism::v1::admin_service_server::AdminService;
use ps_proto::prism::v1::{
    ApiTokenInfo, CreateApiTokenRequest, CreateApiTokenResponse, CreateBackupRequest,
    CreateBackupResponse, ListApiTokensRequest, ListApiTokensResponse, RevokeApiTokenRequest,
    RevokeApiTokenResponse,
};
use time::OffsetDateTime;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::info;
use uuid::Uuid;

use super::common::{backup_err, db_err, require_auth, to_timestamp};

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
        let _ctx = require_auth(&request)?;

        let repos = self.repos.clone();

        let stream = async_stream::try_stream! {
            let mut buf = Vec::new();

            // Gather table counts and write backup
            let source_count = repos.config.count_sources().await.map_err(db_err)?;

            // org and auth counts — will move to OrgRepo/AuthRepo in T4/T5
            let people_count = sqlx::query_scalar!("SELECT COUNT(*) FROM org.people")
                .fetch_one(repos.org.pool())
                .await
                .map_err(db_err)?
                .unwrap_or(0);

            let team_count = sqlx::query_scalar!("SELECT COUNT(*) FROM org.teams")
                .fetch_one(repos.org.pool())
                .await
                .map_err(db_err)?
                .unwrap_or(0);

            let user_count = sqlx::query_scalar!("SELECT COUNT(*) FROM auth.users")
                .fetch_one(repos.auth.pool())
                .await
                .map_err(db_err)?
                .unwrap_or(0);

            let mut table_counts = HashMap::new();
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

            // Export people (will move to OrgRepo in T4)
            let people = sqlx::query!(
                "SELECT id, name, email, level, directory_id, created_at, updated_at FROM org.people"
            )
            .fetch_all(repos.org.pool())
            .await
            .map_err(db_err)?;

            let people_rows: Vec<serde_json::Value> = people
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "id": p.id,
                        "name": p.name,
                        "email": p.email,
                        "level": p.level,
                        "directory_id": p.directory_id,
                        "created_at": p.created_at.to_string(),
                        "updated_at": p.updated_at.to_string(),
                    })
                })
                .collect();

            writer.write_table("people", &people_rows)
                .map_err(backup_err)?;

            // Export teams (will move to OrgRepo in T4)
            let teams = sqlx::query!(
                "SELECT id, name, org_name, parent_team_id, lead_id, github_team_slug, created_at FROM org.teams"
            )
            .fetch_all(repos.org.pool())
            .await
            .map_err(db_err)?;

            let team_rows: Vec<serde_json::Value> = teams
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id,
                        "name": t.name,
                        "org_name": t.org_name,
                        "parent_team_id": t.parent_team_id,
                        "lead_id": t.lead_id,
                        "github_team_slug": t.github_team_slug,
                        "created_at": t.created_at.to_string(),
                    })
                })
                .collect();

            writer.write_table("teams", &team_rows)
                .map_err(backup_err)?;

            // Export users (without password hashes for security; will move to AuthRepo in T5)
            let users = sqlx::query!(
                "SELECT id, username, display_name, role, is_active, person_id, created_at, updated_at FROM auth.users"
            )
            .fetch_all(repos.auth.pool())
            .await
            .map_err(db_err)?;

            let user_rows: Vec<serde_json::Value> = users
                .iter()
                .map(|u| {
                    serde_json::json!({
                        "id": u.id,
                        "username": u.username,
                        "display_name": u.display_name,
                        "role": u.role,
                        "is_active": u.is_active,
                        "person_id": u.person_id,
                        "created_at": u.created_at.to_string(),
                        "updated_at": u.updated_at.to_string(),
                    })
                })
                .collect();

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
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        if req.name.is_empty() {
            return Err(Status::invalid_argument("token name is required"));
        }

        // API tokens are stored as sessions with type 'api_token'
        let raw_token = generate_token();
        let token_hash = hash_token(&raw_token);
        let token_id = Uuid::now_v7();

        // Use the authenticated user's ID (will move to AuthRepo in T5)
        let user_id = _ctx.user_id;

        sqlx::query!(
            r#"
            INSERT INTO auth.sessions (id, user_id, token_hash, session_type, token_name)
            VALUES ($1, $2, $3, 'api_token', $4)
            "#,
            token_id,
            user_id,
            token_hash,
            req.name,
        )
        .execute(self.repos.auth.pool())
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

        let tokens = sqlx::query!(
            r#"
            SELECT id, token_name, created_at, last_active_at
            FROM auth.sessions
            WHERE user_id = $1 AND session_type = 'api_token'
            ORDER BY created_at DESC
            "#,
            ctx.user_id,
        )
        .fetch_all(self.repos.auth.pool())
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

        let result = sqlx::query!(
            r#"
            DELETE FROM auth.sessions
            WHERE id = $1 AND user_id = $2 AND session_type = 'api_token'
            "#,
            token_id,
            ctx.user_id,
        )
        .execute(self.repos.auth.pool())
        .await
        .map_err(db_err)?;

        if result.rows_affected() == 0 {
            return Err(Status::not_found("token not found"));
        }

        info!(token_id = %token_id, "API token revoked");

        Ok(Response::new(RevokeApiTokenResponse {}))
    }
}
