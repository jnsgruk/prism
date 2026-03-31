use ps_core::crypto;
use ps_proto::canonical::prism::v1::{
    AiModelInfo, AiSettings, AiTaskConfig as ProtoAiTaskConfig, GetAiSettingsRequest,
    GetAiSettingsResponse, GetStorageHealthRequest, GetStorageHealthResponse, ListAiModelsRequest,
    ListAiModelsResponse, RefreshModelCatalogueRequest, RefreshModelCatalogueResponse,
    SetProviderSecretRequest, SetProviderSecretResponse, TestProviderRequest, TestProviderResponse,
    UpdateAiSettingsRequest, UpdateAiSettingsResponse,
};
use ps_reasoning::types::{AiConfig, AiTaskConfig};
use tonic::{Request, Response, Status};
use tracing::{error, info};
use uuid::Uuid;

use super::super::common::{
    ai_provider_to_proto, db_err, proto_to_ai_provider_str, require_auth, to_timestamp,
};
use super::ReasoningServiceImpl;

/// Load AI config from `global_settings`, falling back to defaults.
pub async fn load_ai_config(svc: &ReasoningServiceImpl) -> Result<AiConfig, Status> {
    let settings = svc
        .repos
        .config
        .list_global_settings("ai.")
        .await
        .map_err(db_err)?;

    let mut config = AiConfig::default();

    for s in &settings {
        match s.key.as_str() {
            "ai.tasks.enrichment" => {
                if let Ok(tc) = serde_json::from_value(s.value.clone()) {
                    config.tasks.enrichment = tc;
                }
            }
            "ai.tasks.insights" => {
                if let Ok(tc) = serde_json::from_value(s.value.clone()) {
                    config.tasks.insights = tc;
                }
            }
            "ai.tasks.agentic" => {
                if let Ok(tc) = serde_json::from_value(s.value.clone()) {
                    config.tasks.agentic = tc;
                }
            }
            "ai.tasks.embeddings" => {
                if let Ok(tc) = serde_json::from_value(s.value.clone()) {
                    config.tasks.embeddings = tc;
                }
            }
            "ai.tasks.image_generation" => {
                if let Ok(tc) = serde_json::from_value(s.value.clone()) {
                    config.image_generation = Some(tc);
                }
            }
            "ai.budget_cap_usd" => {
                if let Some(cap) = s.value.as_f64() {
                    config.budget_cap_usd = Some(cap);
                }
            }
            _ => {}
        }
    }

    Ok(config)
}

/// Build the proto `AiSettings` from current config + secret status.
pub async fn build_ai_settings(svc: &ReasoningServiceImpl) -> Result<AiSettings, Status> {
    let config = load_ai_config(svc).await?;
    let secret_keys = svc
        .repos
        .config
        .list_global_secret_keys()
        .await
        .map_err(db_err)?;

    let mut provider_secret_status = std::collections::HashMap::new();
    provider_secret_status.insert(
        "google".into(),
        secret_keys.contains(&"google_api_key".to_string()),
    );
    provider_secret_status.insert(
        "openrouter".into(),
        secret_keys.contains(&"openrouter_api_key".to_string()),
    );

    Ok(AiSettings {
        enrichment: Some(task_config_to_proto(&config.tasks.enrichment)),
        insights: Some(task_config_to_proto(&config.tasks.insights)),
        agentic: Some(task_config_to_proto(&config.tasks.agentic)),
        embeddings: Some(task_config_to_proto(&config.tasks.embeddings)),
        budget_cap_usd: config.budget_cap_usd,
        provider_secret_status,
        image_generation: config.image_generation.as_ref().map(task_config_to_proto),
    })
}

/// Fire-and-forget trigger of the `ModelCatalogueHandler` via Restate.
pub async fn trigger_catalogue_refresh(svc: &ReasoningServiceImpl) -> bool {
    let url = format!(
        "{}/ModelCatalogueHandler/refresh_catalogue/send",
        svc.restate_url,
    );
    match svc.http_client.post(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            info!("triggered model catalogue refresh via Restate");
            true
        }
        Ok(resp) => {
            let status = resp.status();
            tracing::warn!(%status, "failed to trigger model catalogue refresh");
            false
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to reach Restate for catalogue refresh");
            false
        }
    }
}

/// Load AI config and provider API keys from the database into the router.
///
/// Called at startup so that provider keys survive server restarts.
pub async fn load_providers_from_db_impl(svc: &ReasoningServiceImpl) {
    // Load config
    match load_ai_config(svc).await {
        Ok(config) => {
            svc.router.write().await.update_config(config);
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to load AI config from database");
        }
    }

    // Load provider API keys
    for (provider, secret_key_name) in [
        ("google", "google_api_key"),
        ("openrouter", "openrouter_api_key"),
    ] {
        match svc.repos.config.get_global_secret(secret_key_name).await {
            Ok(Some(encrypted)) => match ps_core::crypto::decrypt(&svc.secret_key, &encrypted) {
                Ok(decrypted) => {
                    if let Ok(api_key) = String::from_utf8(decrypted) {
                        let mut router = svc.router.write().await;
                        match provider {
                            "google" => router.set_google(&api_key),
                            "openrouter" => router.set_openrouter(&api_key),
                            _ => {}
                        }
                        info!(provider, "loaded AI provider key from database");
                    }
                }
                Err(e) => {
                    tracing::warn!(provider, error = %e, "failed to decrypt provider key");
                }
            },
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(provider, error = %e, "failed to load provider key");
            }
        }
    }
}

fn task_config_to_proto(tc: &AiTaskConfig) -> ProtoAiTaskConfig {
    ProtoAiTaskConfig {
        provider: ai_provider_to_proto(tc.provider.as_str()),
        model: tc.model.clone(),
    }
}

fn proto_to_task_config(p: &ProtoAiTaskConfig) -> Option<AiTaskConfig> {
    let provider_str = proto_to_ai_provider_str(p.provider)?;
    let provider = provider_str.parse().ok()?;
    Some(AiTaskConfig {
        provider,
        model: p.model.clone(),
    })
}

/// Secret key name for a provider (given proto enum i32).
#[allow(clippy::result_large_err)]
fn provider_secret_key(provider: i32) -> Result<(&'static str, &'static str), Status> {
    let provider_str = proto_to_ai_provider_str(provider)
        .ok_or_else(|| Status::invalid_argument("unknown provider"))?;
    match provider_str.as_str() {
        "google" => Ok(("google", "google_api_key")),
        "openrouter" => Ok(("openrouter", "openrouter_api_key")),
        _ => Err(Status::invalid_argument(format!(
            "unknown provider: {provider_str}"
        ))),
    }
}

pub async fn get_ai_settings(
    svc: &ReasoningServiceImpl,
    request: Request<GetAiSettingsRequest>,
) -> Result<Response<GetAiSettingsResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let settings = build_ai_settings(svc).await?;
    Ok(Response::new(GetAiSettingsResponse {
        settings: Some(settings),
    }))
}

pub async fn update_ai_settings(
    svc: &ReasoningServiceImpl,
    request: Request<UpdateAiSettingsRequest>,
) -> Result<Response<UpdateAiSettingsResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    // Save each provided task config
    if let Some(tc) = &req.enrichment
        && let Some(config) = proto_to_task_config(tc)
    {
        let value = serde_json::to_value(&config).map_err(|e| {
            error!(error = %e, "failed to serialize task config");
            Status::internal("internal error")
        })?;
        svc.repos
            .config
            .set_global_setting("ai.tasks.enrichment", &value)
            .await
            .map_err(db_err)?;
    }
    if let Some(tc) = &req.insights
        && let Some(config) = proto_to_task_config(tc)
    {
        let value = serde_json::to_value(&config).map_err(|e| {
            error!(error = %e, "failed to serialize task config");
            Status::internal("internal error")
        })?;
        svc.repos
            .config
            .set_global_setting("ai.tasks.insights", &value)
            .await
            .map_err(db_err)?;
    }
    if let Some(tc) = &req.agentic
        && let Some(config) = proto_to_task_config(tc)
    {
        let value = serde_json::to_value(&config).map_err(|e| {
            error!(error = %e, "failed to serialize task config");
            Status::internal("internal error")
        })?;
        svc.repos
            .config
            .set_global_setting("ai.tasks.agentic", &value)
            .await
            .map_err(db_err)?;
    }
    if let Some(tc) = &req.embeddings
        && let Some(config) = proto_to_task_config(tc)
    {
        let value = serde_json::to_value(&config).map_err(|e| {
            error!(error = %e, "failed to serialize task config");
            Status::internal("internal error")
        })?;
        svc.repos
            .config
            .set_global_setting("ai.tasks.embeddings", &value)
            .await
            .map_err(db_err)?;
    }
    if let Some(tc) = &req.image_generation
        && let Some(config) = proto_to_task_config(tc)
    {
        let value = serde_json::to_value(&config).map_err(|e| {
            error!(error = %e, "failed to serialize task config");
            Status::internal("internal error")
        })?;
        svc.repos
            .config
            .set_global_setting("ai.tasks.image_generation", &value)
            .await
            .map_err(db_err)?;
    }
    if let Some(cap) = req.budget_cap_usd {
        let value = serde_json::json!(cap);
        svc.repos
            .config
            .set_global_setting("ai.budget_cap_usd", &value)
            .await
            .map_err(db_err)?;
    }

    // Reload config into the router
    let config = load_ai_config(svc).await?;
    svc.router.write().await.update_config(config);

    info!("AI settings updated");

    let settings = build_ai_settings(svc).await?;
    Ok(Response::new(UpdateAiSettingsResponse {
        settings: Some(settings),
    }))
}

pub async fn set_provider_secret(
    svc: &ReasoningServiceImpl,
    request: Request<SetProviderSecretRequest>,
) -> Result<Response<SetProviderSecretResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let (provider_name, secret_key_name) = provider_secret_key(req.provider)?;

    if req.secret_value.is_empty() {
        return Err(Status::invalid_argument("secret_value is required"));
    }

    let encrypted = crypto::encrypt(&svc.secret_key, req.secret_value.as_bytes()).map_err(|e| {
        error!(error = %e, "secret encryption failed");
        Status::internal("internal error")
    })?;

    let id = Uuid::now_v7();
    svc.repos
        .config
        .upsert_global_secret(id, secret_key_name, &encrypted)
        .await
        .map_err(db_err)?;

    // Update the router with the new Rig provider client
    {
        let mut router = svc.router.write().await;
        match provider_name {
            "google" => router.set_google(&req.secret_value),
            "openrouter" => router.set_openrouter(&req.secret_value),
            _ => {}
        }
    }

    info!(provider = %provider_name, "provider secret set");

    // Auto-trigger model catalogue refresh so the admin gets up-to-date models
    trigger_catalogue_refresh(svc).await;

    Ok(Response::new(SetProviderSecretResponse {}))
}

pub async fn list_ai_models(
    svc: &ReasoningServiceImpl,
    request: Request<ListAiModelsRequest>,
) -> Result<Response<ListAiModelsResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let provider_str = proto_to_ai_provider_str(req.provider);
    let capability = if req.capability.is_empty() {
        None
    } else {
        Some(req.capability.as_str())
    };

    let models = svc
        .repos
        .config
        .list_ai_models(provider_str.as_deref(), capability)
        .await
        .map_err(db_err)?;

    let proto_models: Vec<AiModelInfo> = models
        .into_iter()
        .map(|m| AiModelInfo {
            id: m.id,
            provider: ai_provider_to_proto(m.provider.as_str()),
            display_name: m.display_name,
            description: m.description.unwrap_or_default(),
            context_length: m.context_length.unwrap_or(0),
            input_price_per_million: m.input_price,
            output_price_per_million: m.output_price,
            capabilities: m.capabilities,
        })
        .collect();

    // Fetch last-refreshed timestamps
    let settings = svc
        .repos
        .config
        .list_global_settings("ai.models_refreshed.")
        .await
        .map_err(db_err)?;
    let last_refreshed: std::collections::HashMap<String, prost_types::Timestamp> = settings
        .into_iter()
        .filter_map(|s| {
            let provider_name = s.key.strip_prefix("ai.models_refreshed.")?;
            let iso = s.value.as_str()?;
            let dt =
                time::OffsetDateTime::parse(iso, &time::format_description::well_known::Rfc3339)
                    .ok()?;
            Some((provider_name.to_string(), to_timestamp(dt)))
        })
        .collect();

    Ok(Response::new(ListAiModelsResponse {
        models: proto_models,
        last_refreshed,
    }))
}

pub async fn refresh_model_catalogue(
    svc: &ReasoningServiceImpl,
    request: Request<RefreshModelCatalogueRequest>,
) -> Result<Response<RefreshModelCatalogueResponse>, Status> {
    let _ctx = require_auth(&request)?;

    let started = trigger_catalogue_refresh(svc).await;

    Ok(Response::new(RefreshModelCatalogueResponse { started }))
}

pub async fn test_provider(
    svc: &ReasoningServiceImpl,
    request: Request<TestProviderRequest>,
) -> Result<Response<TestProviderResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let provider_str = proto_to_ai_provider_str(req.provider)
        .ok_or_else(|| Status::invalid_argument("unknown provider"))?;
    let provider: ps_core::models::AiProvider = provider_str
        .parse()
        .map_err(|_| Status::invalid_argument(format!("unknown provider: {provider_str}")))?;

    let router = svc.router.read().await;
    match router.test_provider(provider).await {
        Ok(()) => Ok(Response::new(TestProviderResponse {
            success: true,
            error_message: String::new(),
        })),
        Err(e) => Ok(Response::new(TestProviderResponse {
            success: false,
            error_message: e.to_string(),
        })),
    }
}

pub async fn get_storage_health(
    svc: &ReasoningServiceImpl,
    request: Request<GetStorageHealthRequest>,
) -> Result<Response<GetStorageHealthResponse>, Status> {
    let _ctx = require_auth(&request)?;

    match &svc.artifact_store {
        Some(store) => match store.health_check().await {
            Ok(()) => Ok(Response::new(GetStorageHealthResponse {
                healthy: true,
                error_message: String::new(),
            })),
            Err(e) => Ok(Response::new(GetStorageHealthResponse {
                healthy: false,
                error_message: e.to_string(),
            })),
        },
        None => Ok(Response::new(GetStorageHealthResponse {
            healthy: false,
            error_message: "object storage not configured".into(),
        })),
    }
}
