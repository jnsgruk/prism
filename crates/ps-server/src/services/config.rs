use std::collections::HashMap;

use base64::Engine;
use ps_core::crypto;
use ps_core::repo::Repos;
use ps_proto::prism::v1::config_service_server::ConfigService;
use ps_proto::prism::v1::{
    CreateSourceRequest, CreateSourceResponse, DeleteSourceRequest, DeleteSourceResponse,
    GetSourceRequest, GetSourceResponse, ListSourcesRequest, ListSourcesResponse, SetSecretRequest,
    SetSecretResponse, SourceConfig, TestConnectionRequest, TestConnectionResponse,
    UpdateSourceRequest, UpdateSourceResponse,
};
use tonic::{Request, Response, Status};
use tracing::info;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::common::{
    db_err, prost_struct_to_serde_json, require_auth, serde_json_to_prost_struct, to_timestamp,
};

pub struct ConfigServiceImpl {
    repos: Repos,
    secret_key: Zeroizing<[u8; 32]>,
    http_client: reqwest::Client,
}

impl ConfigServiceImpl {
    pub fn new(repos: Repos, secret_key: Zeroizing<[u8; 32]>) -> Self {
        Self {
            repos,
            secret_key,
            http_client: reqwest::Client::new(),
        }
    }
}

/// Build a `SourceConfig` proto from a DB row + secret status map.
fn build_source_proto(
    source: &ps_core::models::SourceConfig,
    secret_status: HashMap<String, bool>,
) -> SourceConfig {
    let settings_struct = serde_json_to_prost_struct(&source.settings);

    SourceConfig {
        id: source.id.to_string(),
        source_type: source.source_type.to_string(),
        name: source.name.clone(),
        enabled: source.enabled,
        settings: Some(settings_struct),
        secret_status,
        schedule_cron: source.schedule_cron.clone(),
        created_at: Some(to_timestamp(source.created_at)),
        updated_at: Some(to_timestamp(source.updated_at)),
    }
}

/// Derive a URL-safe slug from a source name for use as a platform suffix.
///
/// "Canonical Discourse" → "canonical-discourse", "Ubuntu" → "ubuntu"
fn slugify_source_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

async fn fetch_secret_status(
    repos: &Repos,
    source_id: Uuid,
) -> Result<HashMap<String, bool>, Status> {
    let keys = repos
        .config
        .list_secret_keys(source_id)
        .await
        .map_err(db_err)?;
    Ok(keys.into_iter().map(|k| (k, true)).collect())
}

#[tonic::async_trait]
impl ConfigService for ConfigServiceImpl {
    async fn list_sources(
        &self,
        request: Request<ListSourcesRequest>,
    ) -> Result<Response<ListSourcesResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let (sources, all_secrets) = tokio::try_join!(
            async { self.repos.config.list_sources().await.map_err(db_err) },
            async {
                self.repos
                    .config
                    .list_all_secret_keys()
                    .await
                    .map_err(db_err)
            },
        )?;

        let result: Vec<_> = sources
            .iter()
            .map(|s| {
                let secret_status: HashMap<String, bool> = all_secrets
                    .get(&s.id)
                    .map(|keys| keys.iter().map(|k| (k.clone(), true)).collect())
                    .unwrap_or_default();
                build_source_proto(s, secret_status)
            })
            .collect();

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

        let s = self
            .repos
            .config
            .get_source(source_id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("source not found"))?;

        let secret_status = fetch_secret_status(&self.repos, s.id).await?;

        Ok(Response::new(GetSourceResponse {
            source: Some(build_source_proto(&s, secret_status)),
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

        // For Discourse, the source_type must be instance-qualified (e.g.
        // "discourse-ubuntu") so it parses into Platform::Discourse(instance).
        // The frontend sends "discourse"; derive the suffix from the source name.
        let effective_source_type = if req.source_type == "discourse" {
            let slug = slugify_source_name(&req.name);
            format!("discourse-{slug}")
        } else {
            req.source_type.clone()
        };

        let s = self
            .repos
            .config
            .create_source(
                source_id,
                &effective_source_type,
                &req.name,
                &settings,
                req.schedule_cron.as_deref(),
            )
            .await
            .map_err(|e| match e {
                ps_core::Error::Conflict(msg) => Status::already_exists(msg),
                other => db_err(other),
            })?;

        info!(source_id = %source_id, name = %req.name, source_type = %req.source_type, "source created");

        Ok(Response::new(CreateSourceResponse {
            source: Some(build_source_proto(&s, HashMap::new())),
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

        // Verify source exists
        if !self
            .repos
            .config
            .source_exists(source_id)
            .await
            .map_err(db_err)?
        {
            return Err(Status::not_found("source not found"));
        }

        // Apply partial updates
        if let Some(enabled) = req.enabled {
            self.repos
                .config
                .update_source_enabled(source_id, enabled)
                .await
                .map_err(db_err)?;
        }

        if let Some(settings) = &req.settings {
            let settings_json = prost_struct_to_serde_json(settings);
            self.repos
                .config
                .update_source_settings(source_id, &settings_json)
                .await
                .map_err(db_err)?;
        }

        if let Some(cron) = &req.schedule_cron {
            self.repos
                .config
                .update_source_schedule(source_id, cron)
                .await
                .map_err(db_err)?;
        }

        // Re-fetch the updated source
        let s = self
            .repos
            .config
            .get_source(source_id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("source not found"))?;

        let secret_status = fetch_secret_status(&self.repos, s.id).await?;

        info!(source_id = %source_id, "source updated");

        Ok(Response::new(UpdateSourceResponse {
            source: Some(build_source_proto(&s, secret_status)),
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

        let deleted = self
            .repos
            .config
            .delete_source(source_id)
            .await
            .map_err(db_err)?;

        if !deleted {
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
        if !self
            .repos
            .config
            .source_exists(source_id)
            .await
            .map_err(db_err)?
        {
            return Err(Status::not_found("source not found"));
        }

        let encrypted = crypto::encrypt(&self.secret_key, req.secret_value.as_bytes())
            .map_err(|e| Status::internal(format!("encryption error: {e}")))?;

        let secret_id = Uuid::now_v7();

        self.repos
            .config
            .upsert_secret(secret_id, source_id, &req.secret_key, &encrypted)
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

        let source = self
            .repos
            .config
            .get_source(source_id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("source not found"))?;

        // Check if required secrets are configured
        let secret_keys = self
            .repos
            .config
            .list_secret_keys(source_id)
            .await
            .map_err(db_err)?;

        let mut details = HashMap::new();
        details.insert("source_type".into(), source.source_type.to_string());
        details.insert("secrets_configured".into(), secret_keys.len().to_string());

        // Validate required secrets based on source type
        let required_secrets: &[&str] = match source.source_type {
            ps_core::models::Platform::Github | ps_core::models::Platform::Jira => &["api_token"],
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

        // Test connection per source type
        if source.source_type == ps_core::models::Platform::Jira {
            self.test_jira_connection(source_id, &source, &mut details)
                .await
        } else if source.source_type.is_discourse() {
            self.test_discourse_connection(source_id, &source, &mut details)
                .await
        } else {
            // Default: return success if all required secrets are present
            details.insert("status".into(), "secrets_validated".into());
            Ok(Response::new(TestConnectionResponse {
                success: true,
                error_message: String::new(),
                details,
            }))
        }
    }
}

impl ConfigServiceImpl {
    /// Test connection to a Discourse instance by calling `/about.json`.
    async fn test_discourse_connection(
        &self,
        source_id: Uuid,
        source: &ps_core::models::SourceConfig,
        details: &mut HashMap<String, String>,
    ) -> Result<Response<TestConnectionResponse>, Status> {
        let base_url = source
            .settings
            .get("base_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim_end_matches('/');

        if base_url.is_empty() {
            return Ok(Response::new(TestConnectionResponse {
                success: false,
                error_message: "base_url is not configured".into(),
                details: details.clone(),
            }));
        }

        // Decrypt API key (optional — Discourse public endpoints work without auth)
        let api_key = match self
            .repos
            .config
            .get_encrypted_secret(source_id, "api_key")
            .await
        {
            Ok(Some(enc)) => match crypto::decrypt(&self.secret_key, &enc) {
                Ok(dec) => String::from_utf8(dec).unwrap_or_default(),
                Err(e) => {
                    return Ok(Response::new(TestConnectionResponse {
                        success: false,
                        error_message: format!("failed to decrypt api_key: {e}"),
                        details: details.clone(),
                    }));
                }
            },
            Ok(None) => String::new(),
            Err(e) => {
                return Ok(Response::new(TestConnectionResponse {
                    success: false,
                    error_message: format!("db error: {e}"),
                    details: details.clone(),
                }));
            }
        };

        // Call Discourse /about.json
        let url = format!("{base_url}/about.json");
        let mut req = self.http_client.get(&url);
        if !api_key.is_empty() {
            req = req
                .header("Api-Key", &api_key)
                .header("Api-Username", "system");
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    let body: serde_json::Value = resp.json().await.unwrap_or_default();
                    let title = body
                        .get("about")
                        .and_then(|a| a.get("title"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let version = body
                        .get("about")
                        .and_then(|a| a.get("version"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    details.insert("status".into(), "connected".into());
                    details.insert("site_title".into(), title.to_string());
                    details.insert("discourse_version".into(), version.to_string());
                    Ok(Response::new(TestConnectionResponse {
                        success: true,
                        error_message: String::new(),
                        details: details.clone(),
                    }))
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    details.insert("response_body".into(), body);
                    Ok(Response::new(TestConnectionResponse {
                        success: false,
                        error_message: format!("Discourse returned {status}"),
                        details: details.clone(),
                    }))
                }
            }
            Err(e) => Ok(Response::new(TestConnectionResponse {
                success: false,
                error_message: format!("connection failed: {e}"),
                details: details.clone(),
            })),
        }
    }

    /// Test connection to a Jira instance by calling the `/rest/api/3/myself` endpoint.
    async fn test_jira_connection(
        &self,
        source_id: Uuid,
        source: &ps_core::models::SourceConfig,
        details: &mut HashMap<String, String>,
    ) -> Result<Response<TestConnectionResponse>, Status> {
        let base_url = source
            .settings
            .get("base_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim_end_matches('/');

        if base_url.is_empty() {
            return Ok(Response::new(TestConnectionResponse {
                success: false,
                error_message: "base_url is not configured".into(),
                details: details.clone(),
            }));
        }

        let api_mode = source
            .settings
            .get("api_mode")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("cloud");

        // Decrypt API token
        let token = match self
            .repos
            .config
            .get_encrypted_secret(source_id, "api_token")
            .await
        {
            Ok(Some(enc)) => match crypto::decrypt(&self.secret_key, &enc) {
                Ok(dec) => String::from_utf8(dec).unwrap_or_default(),
                Err(e) => {
                    return Ok(Response::new(TestConnectionResponse {
                        success: false,
                        error_message: format!("failed to decrypt api_token: {e}"),
                        details: details.clone(),
                    }));
                }
            },
            Ok(None) => {
                return Ok(Response::new(TestConnectionResponse {
                    success: false,
                    error_message: "api_token secret not found".into(),
                    details: details.clone(),
                }));
            }
            Err(e) => {
                return Ok(Response::new(TestConnectionResponse {
                    success: false,
                    error_message: format!("db error: {e}"),
                    details: details.clone(),
                }));
            }
        };

        // For Cloud mode, also decrypt email for Basic auth
        let auth_header = if api_mode == "server" {
            format!("Bearer {token}")
        } else {
            let email = match self
                .repos
                .config
                .get_encrypted_secret(source_id, "email")
                .await
            {
                Ok(Some(enc)) => match crypto::decrypt(&self.secret_key, &enc) {
                    Ok(dec) => String::from_utf8(dec).unwrap_or_default(),
                    Err(_) => String::new(),
                },
                _ => String::new(),
            };
            let credentials =
                base64::engine::general_purpose::STANDARD.encode(format!("{email}:{token}"));
            format!("Basic {credentials}")
        };

        // Call Jira /rest/api/3/myself
        let url = format!("{base_url}/rest/api/3/myself");
        match self
            .http_client
            .get(&url)
            .header("Authorization", &auth_header)
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    let body: serde_json::Value = resp.json().await.unwrap_or_default();
                    let display_name = body
                        .get("displayName")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    details.insert("status".into(), "connected".into());
                    details.insert("authenticated_as".into(), display_name.to_string());
                    Ok(Response::new(TestConnectionResponse {
                        success: true,
                        error_message: String::new(),
                        details: details.clone(),
                    }))
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    details.insert("response_body".into(), body);
                    Ok(Response::new(TestConnectionResponse {
                        success: false,
                        error_message: format!("Jira returned {status}"),
                        details: details.clone(),
                    }))
                }
            }
            Err(e) => Ok(Response::new(TestConnectionResponse {
                success: false,
                error_message: format!("connection failed: {e}"),
                details: details.clone(),
            })),
        }
    }
}
