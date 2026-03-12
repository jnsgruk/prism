use std::collections::{BTreeMap, HashMap};

use ps_core::crypto;
use ps_proto::prism::v1::config_service_server::ConfigService;
use ps_proto::prism::v1::{
    CreateSourceRequest, CreateSourceResponse, DeleteSourceRequest, DeleteSourceResponse,
    GetSourceRequest, GetSourceResponse, ListSourcesRequest, ListSourcesResponse, SetSecretRequest,
    SetSecretResponse, SourceConfig, TestConnectionRequest, TestConnectionResponse,
    UpdateSourceRequest, UpdateSourceResponse,
};
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use tracing::info;
use uuid::Uuid;

use super::common::{db_err, require_auth, to_timestamp};

pub struct ConfigServiceImpl {
    pool: PgPool,
    secret_key: [u8; 32],
}

impl ConfigServiceImpl {
    pub fn new(pool: PgPool, secret_key: [u8; 32]) -> Self {
        Self { pool, secret_key }
    }
}

/// Build a `SourceConfig` proto from a DB row + secret status map.
#[allow(clippy::too_many_arguments)]
fn build_source_proto(
    id: Uuid,
    source_type: String,
    name: String,
    enabled: bool,
    settings: serde_json::Value,
    schedule_cron: Option<String>,
    created_at: time::OffsetDateTime,
    updated_at: time::OffsetDateTime,
    secret_status: HashMap<String, bool>,
) -> SourceConfig {
    let settings_struct = serde_json_to_prost_struct(&settings);

    SourceConfig {
        id: id.to_string(),
        source_type,
        name,
        enabled,
        settings: Some(settings_struct),
        secret_status,
        schedule_cron,
        created_at: Some(to_timestamp(created_at)),
        updated_at: Some(to_timestamp(updated_at)),
    }
}

/// Convert `serde_json::Value` to `prost_types::Struct`.
fn serde_json_to_prost_struct(value: &serde_json::Value) -> prost_types::Struct {
    match value {
        serde_json::Value::Object(map) => {
            let fields = map
                .iter()
                .map(|(k, v)| (k.clone(), serde_json_to_prost_value(v)))
                .collect();
            prost_types::Struct { fields }
        }
        _ => prost_types::Struct {
            fields: BTreeMap::new(),
        },
    }
}

fn serde_json_to_prost_value(value: &serde_json::Value) -> prost_types::Value {
    let kind = match value {
        serde_json::Value::Null => Some(prost_types::value::Kind::NullValue(0)),
        serde_json::Value::Bool(b) => Some(prost_types::value::Kind::BoolValue(*b)),
        serde_json::Value::Number(n) => Some(prost_types::value::Kind::NumberValue(
            n.as_f64().unwrap_or(0.0),
        )),
        serde_json::Value::String(s) => Some(prost_types::value::Kind::StringValue(s.clone())),
        serde_json::Value::Array(arr) => {
            let values = arr.iter().map(serde_json_to_prost_value).collect();
            Some(prost_types::value::Kind::ListValue(
                prost_types::ListValue { values },
            ))
        }
        serde_json::Value::Object(_) => Some(prost_types::value::Kind::StructValue(
            serde_json_to_prost_struct(value),
        )),
    };
    prost_types::Value { kind }
}

/// Convert `prost_types::Struct` back to `serde_json::Value`.
fn prost_struct_to_serde_json(s: &prost_types::Struct) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = s
        .fields
        .iter()
        .map(|(k, v)| (k.clone(), prost_value_to_serde_json(v)))
        .collect();
    serde_json::Value::Object(map)
}

fn prost_value_to_serde_json(v: &prost_types::Value) -> serde_json::Value {
    match &v.kind {
        Some(prost_types::value::Kind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(prost_types::value::Kind::NumberValue(n)) => {
            serde_json::json!(n)
        }
        Some(prost_types::value::Kind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(prost_types::value::Kind::ListValue(list)) => {
            let arr: Vec<serde_json::Value> =
                list.values.iter().map(prost_value_to_serde_json).collect();
            serde_json::Value::Array(arr)
        }
        Some(prost_types::value::Kind::StructValue(s)) => prost_struct_to_serde_json(s),
        Some(prost_types::value::Kind::NullValue(_)) | None => serde_json::Value::Null,
    }
}

async fn fetch_secret_status(
    pool: &PgPool,
    source_id: Uuid,
) -> Result<HashMap<String, bool>, Status> {
    let secrets = sqlx::query!(
        "SELECT secret_key FROM config.secrets WHERE source_id = $1",
        source_id,
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    Ok(secrets.into_iter().map(|s| (s.secret_key, true)).collect())
}

#[tonic::async_trait]
impl ConfigService for ConfigServiceImpl {
    async fn list_sources(
        &self,
        request: Request<ListSourcesRequest>,
    ) -> Result<Response<ListSourcesResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let sources = sqlx::query!(
            r#"
            SELECT id, source_type, name, enabled, settings, schedule_cron,
                   created_at, updated_at
            FROM config.source_configs
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let mut result = Vec::with_capacity(sources.len());
        for s in sources {
            let secret_status = fetch_secret_status(&self.pool, s.id).await?;
            result.push(build_source_proto(
                s.id,
                s.source_type,
                s.name,
                s.enabled,
                s.settings,
                s.schedule_cron,
                s.created_at,
                s.updated_at,
                secret_status,
            ));
        }

        Ok(Response::new(ListSourcesResponse { sources: result }))
    }

    async fn get_source(
        &self,
        request: Request<GetSourceRequest>,
    ) -> Result<Response<GetSourceResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let source_id: Uuid = req
            .source_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid source_id"))?;

        let s = sqlx::query!(
            r#"
            SELECT id, source_type, name, enabled, settings, schedule_cron,
                   created_at, updated_at
            FROM config.source_configs
            WHERE id = $1
            "#,
            source_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("source not found"))?;

        let secret_status = fetch_secret_status(&self.pool, s.id).await?;

        Ok(Response::new(GetSourceResponse {
            source: Some(build_source_proto(
                s.id,
                s.source_type,
                s.name,
                s.enabled,
                s.settings,
                s.schedule_cron,
                s.created_at,
                s.updated_at,
                secret_status,
            )),
        }))
    }

    async fn create_source(
        &self,
        request: Request<CreateSourceRequest>,
    ) -> Result<Response<CreateSourceResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        if req.name.is_empty() {
            return Err(Status::invalid_argument("name is required"));
        }
        if req.source_type.is_empty() {
            return Err(Status::invalid_argument("source_type is required"));
        }

        let settings = match &req.settings {
            Some(s) => prost_struct_to_serde_json(s),
            None => serde_json::json!({}),
        };

        let source_id = Uuid::now_v7();

        let s = sqlx::query!(
            r#"
            INSERT INTO config.source_configs (id, source_type, name, settings, schedule_cron)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, source_type, name, enabled, settings, schedule_cron, created_at, updated_at
            "#,
            source_id,
            req.source_type,
            req.name,
            settings,
            req.schedule_cron,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("duplicate key") {
                Status::already_exists(format!("source '{}' already exists", req.name))
            } else {
                Status::internal(format!("database error: {e}"))
            }
        })?;

        info!(source_id = %source_id, name = %req.name, source_type = %req.source_type, "source created");

        Ok(Response::new(CreateSourceResponse {
            source: Some(build_source_proto(
                s.id,
                s.source_type,
                s.name,
                s.enabled,
                s.settings,
                s.schedule_cron,
                s.created_at,
                s.updated_at,
                HashMap::new(),
            )),
        }))
    }

    async fn update_source(
        &self,
        request: Request<UpdateSourceRequest>,
    ) -> Result<Response<UpdateSourceResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let source_id: Uuid = req
            .source_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid source_id"))?;

        // Fetch current state
        let current = sqlx::query!(
            "SELECT id FROM config.source_configs WHERE id = $1",
            source_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("source not found"))?;

        // Apply partial updates
        if let Some(enabled) = req.enabled {
            sqlx::query!(
                "UPDATE config.source_configs SET enabled = $1, updated_at = now() WHERE id = $2",
                enabled,
                current.id,
            )
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }

        if let Some(settings) = &req.settings {
            let settings_json = prost_struct_to_serde_json(settings);
            sqlx::query!(
                "UPDATE config.source_configs SET settings = $1, updated_at = now() WHERE id = $2",
                settings_json,
                current.id,
            )
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }

        if let Some(cron) = &req.schedule_cron {
            sqlx::query!(
                "UPDATE config.source_configs SET schedule_cron = $1, updated_at = now() WHERE id = $2",
                cron,
                current.id,
            )
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }

        // Re-fetch the updated source
        let s = sqlx::query!(
            r#"
            SELECT id, source_type, name, enabled, settings, schedule_cron,
                   created_at, updated_at
            FROM config.source_configs
            WHERE id = $1
            "#,
            source_id,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;

        let secret_status = fetch_secret_status(&self.pool, s.id).await?;

        info!(source_id = %source_id, "source updated");

        Ok(Response::new(UpdateSourceResponse {
            source: Some(build_source_proto(
                s.id,
                s.source_type,
                s.name,
                s.enabled,
                s.settings,
                s.schedule_cron,
                s.created_at,
                s.updated_at,
                secret_status,
            )),
        }))
    }

    async fn delete_source(
        &self,
        request: Request<DeleteSourceRequest>,
    ) -> Result<Response<DeleteSourceResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let source_id: Uuid = req
            .source_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid source_id"))?;

        let result = sqlx::query!("DELETE FROM config.source_configs WHERE id = $1", source_id,)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;

        if result.rows_affected() == 0 {
            return Err(Status::not_found("source not found"));
        }

        info!(source_id = %source_id, "source deleted");

        Ok(Response::new(DeleteSourceResponse {}))
    }

    async fn set_secret(
        &self,
        request: Request<SetSecretRequest>,
    ) -> Result<Response<SetSecretResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let source_id: Uuid = req
            .source_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid source_id"))?;

        if req.secret_key.is_empty() {
            return Err(Status::invalid_argument("secret_key is required"));
        }

        // Verify source exists
        sqlx::query_scalar!(
            "SELECT id FROM config.source_configs WHERE id = $1",
            source_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("source not found"))?;

        let encrypted = crypto::encrypt(&self.secret_key, req.secret_value.as_bytes())
            .map_err(|e| Status::internal(format!("encryption error: {e}")))?;

        let secret_id = Uuid::now_v7();

        sqlx::query!(
            r#"
            INSERT INTO config.secrets (id, source_id, secret_key, encrypted_value)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (source_id, secret_key)
            DO UPDATE SET encrypted_value = $4, updated_at = now()
            "#,
            secret_id,
            source_id,
            req.secret_key,
            encrypted,
        )
        .execute(&self.pool)
        .await
        .map_err(db_err)?;

        info!(source_id = %source_id, key = %req.secret_key, "secret set");

        Ok(Response::new(SetSecretResponse {}))
    }

    async fn test_connection(
        &self,
        request: Request<TestConnectionRequest>,
    ) -> Result<Response<TestConnectionResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let source_id: Uuid = req
            .source_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid source_id"))?;

        let source = sqlx::query!(
            "SELECT source_type, settings FROM config.source_configs WHERE id = $1",
            source_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("source not found"))?;

        // Check if required secrets are configured
        let secrets = sqlx::query!(
            "SELECT secret_key FROM config.secrets WHERE source_id = $1",
            source_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let secret_keys: Vec<String> = secrets.into_iter().map(|s| s.secret_key).collect();
        let mut details = HashMap::new();
        details.insert("source_type".into(), source.source_type.clone());
        details.insert("secrets_configured".into(), secret_keys.len().to_string());

        // For now, validate that required secrets exist based on source type
        let required_secrets: &[&str] = match source.source_type.as_str() {
            "github" | "jira" => &["api_token"],
            _ => &[],
        };

        let missing: Vec<&&str> = required_secrets
            .iter()
            .filter(|k| !secret_keys.contains(&k.to_string()))
            .collect();

        if !missing.is_empty() {
            let missing_str: Vec<String> = missing.iter().map(ToString::to_string).collect();
            return Ok(Response::new(TestConnectionResponse {
                success: false,
                error_message: format!("missing required secrets: {}", missing_str.join(", ")),
                details,
            }));
        }

        // TODO: actual connection testing per source type (HTTP calls to GitHub/Jira APIs)
        // For now, return success if all required secrets are present
        details.insert("status".into(), "secrets_validated".into());

        Ok(Response::new(TestConnectionResponse {
            success: true,
            error_message: String::new(),
            details,
        }))
    }
}
