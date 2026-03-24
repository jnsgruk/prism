use std::sync::Arc;

use ps_core::crypto;
use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::reasoning_service_server::ReasoningService;
use ps_proto::canonical::prism::v1::{
    AiModelInfo, AiSettings, AiTaskConfig as ProtoAiTaskConfig, AskQuestionRequest,
    AskQuestionResponse, DeleteEnrichmentsByTypeRequest, DeleteEnrichmentsByTypeResponse,
    Enrichment as ProtoEnrichment, EnrichmentTypeCount, FindSimilarRequest, FindSimilarResponse,
    GetAiSettingsRequest, GetAiSettingsResponse, GetArtifactDownloadUrlRequest,
    GetArtifactDownloadUrlResponse, GetConversationRequest, GetConversationResponse,
    GetCostSummaryRequest, GetCostSummaryResponse, GetEmbeddingStatusRequest,
    GetEmbeddingStatusResponse, GetEnrichmentPipelineStatusRequest,
    GetEnrichmentPipelineStatusResponse, GetEnrichmentsByContributionsRequest,
    GetEnrichmentsByContributionsResponse, GetEnrichmentsRequest, GetEnrichmentsResponse,
    GetStorageHealthRequest, GetStorageHealthResponse, ListAiModelsRequest, ListAiModelsResponse,
    ListConversationsRequest, ListConversationsResponse, RefreshModelCatalogueRequest,
    RefreshModelCatalogueResponse, SaveInsightFromConversationRequest,
    SaveInsightFromConversationResponse, SearchByTextRequest, SearchByTextResponse,
    SetProviderSecretRequest, SetProviderSecretResponse, SimilarItem as ProtoSimilarItem,
    TestProviderRequest, TestProviderResponse, UpdateAiSettingsRequest, UpdateAiSettingsResponse,
};
use ps_reasoning::types::{AiConfig, AiTaskConfig};
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::common::{
    ai_provider_to_proto, contribution_state_to_proto, contribution_type_to_proto, db_err,
    enrichment_type_to_proto, platform_to_proto, proto_to_ai_provider_str,
    proto_to_enrichment_type_str, proto_to_platform_str, require_auth, to_timestamp,
};

pub struct ReasoningServiceImpl {
    repos: Repos,
    secret_key: Zeroizing<[u8; 32]>,
    router: Arc<RwLock<ps_reasoning::routing::TaskRouter>>,
    artifact_store: Option<Arc<dyn ps_core::ArtifactStore>>,
    container_manager: Option<Arc<ps_agent::ContainerManager>>,
    restate_url: String,
    http_client: reqwest::Client,
}

impl ReasoningServiceImpl {
    pub fn new(
        repos: Repos,
        secret_key: Zeroizing<[u8; 32]>,
        router: Arc<RwLock<ps_reasoning::routing::TaskRouter>>,
        artifact_store: Option<Arc<dyn ps_core::ArtifactStore>>,
        container_manager: Option<Arc<ps_agent::ContainerManager>>,
        restate_url: String,
    ) -> Self {
        Self {
            repos,
            secret_key,
            router,
            artifact_store,
            container_manager,
            restate_url,
            http_client: reqwest::Client::new(),
        }
    }

    /// Load AI config and provider API keys from the database into the router.
    ///
    /// Called at startup so that provider keys survive server restarts.
    pub async fn load_providers_from_db(&self) {
        // Load config
        match self.load_ai_config().await {
            Ok(config) => {
                self.router.write().await.update_config(config);
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
            match self.repos.config.get_global_secret(secret_key_name).await {
                Ok(Some(encrypted)) => {
                    match ps_core::crypto::decrypt(&self.secret_key, &encrypted) {
                        Ok(decrypted) => {
                            if let Ok(api_key) = String::from_utf8(decrypted) {
                                let mut router = self.router.write().await;
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
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(provider, error = %e, "failed to load provider key");
                }
            }
        }
    }

    /// Load AI config from `global_settings`, falling back to defaults.
    async fn load_ai_config(&self) -> Result<AiConfig, Status> {
        let settings = self
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

    /// Fire-and-forget trigger of the `ModelCatalogueHandler` via Restate.
    async fn trigger_catalogue_refresh(&self) -> bool {
        let url = format!(
            "{}/ModelCatalogueHandler/refresh_catalogue/send",
            self.restate_url,
        );
        match self.http_client.post(&url).send().await {
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

    /// Build the proto `AiSettings` from current config + secret status.
    async fn build_ai_settings(&self) -> Result<AiSettings, Status> {
        let config = self.load_ai_config().await?;
        let secret_keys = self
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
        })
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

#[tonic::async_trait]
impl ReasoningService for ReasoningServiceImpl {
    type AskQuestionStream =
        tokio_stream::wrappers::ReceiverStream<Result<AskQuestionResponse, Status>>;
    async fn get_ai_settings(
        &self,
        request: Request<GetAiSettingsRequest>,
    ) -> Result<Response<GetAiSettingsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let settings = self.build_ai_settings().await?;
        Ok(Response::new(GetAiSettingsResponse {
            settings: Some(settings),
        }))
    }

    async fn update_ai_settings(
        &self,
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
            self.repos
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
            self.repos
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
            self.repos
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
            self.repos
                .config
                .set_global_setting("ai.tasks.embeddings", &value)
                .await
                .map_err(db_err)?;
        }
        if let Some(cap) = req.budget_cap_usd {
            let value = serde_json::json!(cap);
            self.repos
                .config
                .set_global_setting("ai.budget_cap_usd", &value)
                .await
                .map_err(db_err)?;
        }

        // Reload config into the router
        let config = self.load_ai_config().await?;
        self.router.write().await.update_config(config);

        info!("AI settings updated");

        let settings = self.build_ai_settings().await?;
        Ok(Response::new(UpdateAiSettingsResponse {
            settings: Some(settings),
        }))
    }

    async fn set_provider_secret(
        &self,
        request: Request<SetProviderSecretRequest>,
    ) -> Result<Response<SetProviderSecretResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let (provider_name, secret_key_name) = provider_secret_key(req.provider)?;

        if req.secret_value.is_empty() {
            return Err(Status::invalid_argument("secret_value is required"));
        }

        let encrypted =
            crypto::encrypt(&self.secret_key, req.secret_value.as_bytes()).map_err(|e| {
                error!(error = %e, "secret encryption failed");
                Status::internal("internal error")
            })?;

        let id = Uuid::now_v7();
        self.repos
            .config
            .upsert_global_secret(id, secret_key_name, &encrypted)
            .await
            .map_err(db_err)?;

        // Update the router with the new Rig provider client
        {
            let mut router = self.router.write().await;
            match provider_name {
                "google" => router.set_google(&req.secret_value),
                "openrouter" => router.set_openrouter(&req.secret_value),
                _ => {}
            }
        }

        info!(provider = %provider_name, "provider secret set");

        // Auto-trigger model catalogue refresh so the admin gets up-to-date models
        self.trigger_catalogue_refresh().await;

        Ok(Response::new(SetProviderSecretResponse {}))
    }

    async fn list_ai_models(
        &self,
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

        let models = self
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
        let settings = self
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
                let dt = time::OffsetDateTime::parse(
                    iso,
                    &time::format_description::well_known::Rfc3339,
                )
                .ok()?;
                Some((provider_name.to_string(), to_timestamp(dt)))
            })
            .collect();

        Ok(Response::new(ListAiModelsResponse {
            models: proto_models,
            last_refreshed,
        }))
    }

    async fn refresh_model_catalogue(
        &self,
        request: Request<RefreshModelCatalogueRequest>,
    ) -> Result<Response<RefreshModelCatalogueResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let started = self.trigger_catalogue_refresh().await;

        Ok(Response::new(RefreshModelCatalogueResponse { started }))
    }

    async fn test_provider(
        &self,
        request: Request<TestProviderRequest>,
    ) -> Result<Response<TestProviderResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let provider_str = proto_to_ai_provider_str(req.provider)
            .ok_or_else(|| Status::invalid_argument("unknown provider"))?;
        let provider: ps_core::models::AiProvider = provider_str
            .parse()
            .map_err(|_| Status::invalid_argument(format!("unknown provider: {provider_str}")))?;

        let router = self.router.read().await;
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

    async fn get_storage_health(
        &self,
        request: Request<GetStorageHealthRequest>,
    ) -> Result<Response<GetStorageHealthResponse>, Status> {
        let _ctx = require_auth(&request)?;

        match &self.artifact_store {
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

    async fn get_cost_summary(
        &self,
        request: Request<GetCostSummaryRequest>,
    ) -> Result<Response<GetCostSummaryResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let days = if req.days > 0 { req.days } else { 7 };

        let today = time::OffsetDateTime::now_utc().date();
        let since = today - time::Duration::days(i64::from(days) - 1);

        let (today_spend, daily_series, task_breakdown, model_breakdown, config) = tokio::try_join!(
            async {
                self.repos
                    .reasoning
                    .get_daily_spend(today)
                    .await
                    .map_err(db_err)
            },
            async {
                self.repos
                    .reasoning
                    .get_daily_spend_series(since, today)
                    .await
                    .map_err(db_err)
            },
            async {
                self.repos
                    .reasoning
                    .get_daily_spend_by_task(today)
                    .await
                    .map_err(db_err)
            },
            async {
                let since_dt = since.midnight().assume_utc();
                let until_dt = (today + time::Duration::days(1)).midnight().assume_utc();
                self.repos
                    .reasoning
                    .get_spend_summary(since_dt, until_dt)
                    .await
                    .map_err(db_err)
            },
            async { self.load_ai_config().await },
        )?;

        let daily_spend: Vec<ps_proto::canonical::prism::v1::DailySpend> = daily_series
            .into_iter()
            .map(|d| ps_proto::canonical::prism::v1::DailySpend {
                date: d.date.to_string(),
                cost_usd: d.total_cost_usd,
                request_count: d.request_count,
            })
            .collect();

        let task_breakdown: Vec<ps_proto::canonical::prism::v1::TaskSpend> = task_breakdown
            .into_iter()
            .map(|t| ps_proto::canonical::prism::v1::TaskSpend {
                task_type: t.task_type,
                cost_usd: t.total_cost_usd,
                prompt_tokens: t.total_prompt_tokens,
                completion_tokens: t.total_completion_tokens,
                request_count: t.request_count,
            })
            .collect();

        let model_breakdown: Vec<ps_proto::canonical::prism::v1::ModelSpend> = model_breakdown
            .into_iter()
            .map(|m| ps_proto::canonical::prism::v1::ModelSpend {
                provider: ai_provider_to_proto(&m.provider),
                model: m.model,
                task_type: m.task_type,
                cost_usd: m.total_cost_usd,
                prompt_tokens: m.total_prompt_tokens,
                completion_tokens: m.total_completion_tokens,
                request_count: m.request_count,
            })
            .collect();

        Ok(Response::new(GetCostSummaryResponse {
            today_spend_usd: today_spend,
            budget_cap_usd: config.budget_cap_usd,
            daily_spend,
            task_breakdown,
            model_breakdown,
        }))
    }

    // -------------------------------------------------------------------
    // Enrichments
    // -------------------------------------------------------------------

    async fn get_enrichments(
        &self,
        request: Request<GetEnrichmentsRequest>,
    ) -> Result<Response<GetEnrichmentsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let contribution_id: Uuid = req
            .contribution_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid contribution_id"))?;

        let enrichments = self
            .repos
            .reasoning
            .get_enrichments_for_contribution(contribution_id)
            .await
            .map_err(db_err)?;

        Ok(Response::new(GetEnrichmentsResponse {
            enrichments: enrichments.into_iter().map(enrichment_to_proto).collect(),
        }))
    }

    async fn get_enrichments_by_contributions(
        &self,
        request: Request<GetEnrichmentsByContributionsRequest>,
    ) -> Result<Response<GetEnrichmentsByContributionsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let ids: Vec<Uuid> = req
            .contribution_ids
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();

        if ids.is_empty() {
            return Ok(Response::new(GetEnrichmentsByContributionsResponse {
                enrichments: vec![],
            }));
        }

        let enrichments = self
            .repos
            .reasoning
            .get_enrichments_for_contributions(&ids)
            .await
            .map_err(db_err)?;

        Ok(Response::new(GetEnrichmentsByContributionsResponse {
            enrichments: enrichments.into_iter().map(enrichment_to_proto).collect(),
        }))
    }

    async fn get_enrichment_pipeline_status(
        &self,
        request: Request<GetEnrichmentPipelineStatusRequest>,
    ) -> Result<Response<GetEnrichmentPipelineStatusResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let status = self
            .repos
            .reasoning
            .get_enrichment_status()
            .await
            .map_err(db_err)?;

        Ok(Response::new(GetEnrichmentPipelineStatusResponse {
            pending_count: status.pending_count,
            total_enrichments: status.total_enrichments,
            last_enrichment_at: status.last_enrichment_at.map(to_timestamp),
            by_type: status
                .by_type
                .into_iter()
                .map(|t| EnrichmentTypeCount {
                    enrichment_type: enrichment_type_to_proto(&t.enrichment_type),
                    count: t.total_count,
                })
                .collect(),
        }))
    }

    async fn delete_enrichments_by_type(
        &self,
        request: Request<DeleteEnrichmentsByTypeRequest>,
    ) -> Result<Response<DeleteEnrichmentsByTypeResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let enrichment_type_str = proto_to_enrichment_type_str(req.enrichment_type)
            .ok_or_else(|| Status::invalid_argument("enrichment_type is required"))?;

        let deleted = self
            .repos
            .reasoning
            .delete_enrichments_by_type(&enrichment_type_str)
            .await
            .map_err(db_err)?;

        info!(enrichment_type = %enrichment_type_str, deleted, "enrichments deleted for re-enrichment");

        Ok(Response::new(DeleteEnrichmentsByTypeResponse {
            #[allow(clippy::cast_possible_wrap)]
            deleted_count: deleted as i64,
        }))
    }

    // -------------------------------------------------------------------
    // Similarity (embeddings)
    // -------------------------------------------------------------------

    async fn find_similar(
        &self,
        request: Request<FindSimilarRequest>,
    ) -> Result<Response<FindSimilarResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let contribution_id: Uuid = req
            .contribution_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid contribution_id"))?;

        let limit = i64::from(req.limit.clamp(1, 50));
        let platform_str = proto_to_platform_str(req.platform, req.platform_instance.as_deref());

        let results = self
            .repos
            .reasoning
            .find_similar_to_contribution(contribution_id, limit, platform_str.as_deref())
            .await
            .map_err(db_err)?;

        Ok(Response::new(FindSimilarResponse {
            items: results.into_iter().map(similar_to_proto).collect(),
        }))
    }

    async fn search_by_text(
        &self,
        request: Request<SearchByTextRequest>,
    ) -> Result<Response<SearchByTextResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        if req.query_text.is_empty() {
            return Err(Status::invalid_argument("query_text is required"));
        }

        let limit = i64::from(req.limit.clamp(1, 50));
        let platform_str = proto_to_platform_str(req.platform, req.platform_instance.as_deref());

        // Embed the query text on-the-fly. Drop the router lock before the
        // API call so concurrent UpdateAiSettings writes aren't blocked.
        let model = {
            let router = self.router.read().await;
            router
                .embedding_model()
                .map_err(|e| Status::unavailable(format!("embedding model not available: {e}")))?
        };

        #[allow(deprecated)]
        let embedding = model.embed_text(&req.query_text).await.map_err(|e| {
            error!(error = %e, "failed to embed query text");
            Status::internal("failed to generate query embedding")
        })?;

        let truncated = ps_reasoning::features::embeddings::truncate_embedding(&embedding);

        let results = self
            .repos
            .reasoning
            .find_similar(&truncated, limit, platform_str.as_deref(), None)
            .await
            .map_err(db_err)?;

        Ok(Response::new(SearchByTextResponse {
            items: results.into_iter().map(similar_to_proto).collect(),
        }))
    }

    async fn get_embedding_status(
        &self,
        request: Request<GetEmbeddingStatusRequest>,
    ) -> Result<Response<GetEmbeddingStatusResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let status = self
            .repos
            .reasoning
            .get_embedding_status()
            .await
            .map_err(db_err)?;

        #[allow(clippy::cast_precision_loss)]
        let coverage = if status.total_eligible > 0 {
            status.embedded_count as f64 / status.total_eligible as f64 * 100.0
        } else {
            0.0
        };

        Ok(Response::new(GetEmbeddingStatusResponse {
            queued_count: status.queued_count,
            embedded_count: status.embedded_count,
            total_eligible: status.total_eligible,
            last_embedded_at: status.last_embedded_at.map(to_timestamp),
            coverage_percent: coverage,
        }))
    }

    // -----------------------------------------------------------------------
    // Agentic query interface
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    async fn ask_question(
        &self,
        request: Request<AskQuestionRequest>,
    ) -> Result<Response<Self::AskQuestionStream>, Status> {
        use ps_agent::event_mapper;
        use ps_core::repo::reasoning::{CreateConversationParams, CreateMessageParams};

        let ctx = require_auth(&request)?;
        let req = request.into_inner();

        // Validate question.
        if req.question.trim().is_empty() {
            return Err(Status::invalid_argument("question must not be empty"));
        }
        if req.question.len() > 4000 {
            return Err(Status::invalid_argument(
                "question must be at most 4000 characters",
            ));
        }

        let cm = self
            .container_manager
            .as_ref()
            .ok_or_else(|| Status::unavailable("agent containers not configured"))?
            .clone();

        // Create or resume conversation — fetch existing if conversation_id provided.
        let existing_conv = if let Some(ref id) = req.conversation_id {
            let conv_id = id
                .parse::<Uuid>()
                .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;
            self.repos
                .reasoning
                .get_conversation(conv_id)
                .await
                .map_err(db_err)?
        } else {
            None
        };

        let conversation_id = if let Some(ref conv) = existing_conv {
            conv.id
        } else {
            let model_name = {
                let router = self.router.read().await;
                let config = router.config();
                format!(
                    "{}/{}",
                    config.tasks.agentic.provider.as_str(),
                    config.tasks.agentic.model
                )
            };
            let conv = self
                .repos
                .reasoning
                .create_conversation(&CreateConversationParams {
                    user_id: ctx.user_id,
                    title: Some(&req.question.chars().take(100).collect::<String>()),
                    model_name: &model_name,
                })
                .await
                .map_err(db_err)?;
            conv.id
        };

        // Store the user message.
        self.repos
            .reasoning
            .create_message(&CreateMessageParams {
                conversation_id,
                role: "user",
                content: &req.question,
                reasoning_trace: None,
                supporting_data: None,
                prompt_tokens: 0,
                completion_tokens: 0,
            })
            .await
            .map_err(db_err)?;

        let repos = self.repos.clone();
        let conv_id = conversation_id;

        // Extract existing pod/session info for session reuse.
        let existing_pod_name = existing_conv
            .as_ref()
            .and_then(|c| c.container_pod_name.clone());
        let existing_opencode_session = existing_conv
            .as_ref()
            .and_then(|c| c.opencode_session_id.clone());

        // Build per-pod overrides: service token + model config + provider keys.
        let service_token = ps_core::auth::generate_token();
        let token_hash = ps_core::auth::hash_token(&service_token);
        let token_session_id = Uuid::now_v7();
        self.repos
            .auth
            .create_session(
                token_session_id,
                ctx.user_id,
                &token_hash,
                "agent_service",
                Some(time::OffsetDateTime::now_utc() + time::Duration::hours(3)),
                Some("agent-container"),
            )
            .await
            .map_err(db_err)?;

        let pod_overrides = {
            let router = self.router.read().await;
            let config = router.config();
            let model = format!(
                "{}/{}",
                config.tasks.agentic.provider.as_str(),
                config.tasks.agentic.model
            );
            ps_agent::PodOverrides {
                service_token,
                token_session_id: token_session_id.to_string(),
                model: model.clone(),
                small_model: model,
                provider_keys: router.provider_env_vars(),
            }
        };

        let (tx, rx) = tokio::sync::mpsc::channel(64);

        // Spawn the streaming orchestration task.
        tokio::spawn(async move {
            // 1. Ensure container is running.
            let _ = tx
                .send(Ok(event_mapper::container_status_event(
                    "creating",
                    "Starting agent container...",
                )))
                .await;

            if let Err(e) = cm.ensure_pod(&conv_id.to_string(), &pod_overrides).await {
                error!(error = %e, "Failed to create agent pod");
                let _ = tx
                    .send(Ok(event_mapper::container_status_event(
                        "error",
                        &format!("Failed to create container: {e}"),
                    )))
                    .await;
                return;
            }

            // 2. Wait for pod to become ready (poll for up to 60s).
            let Some((pod_ip, pod_name)) = wait_for_pod_ready(&cm, &conv_id.to_string(), &tx).await
            else {
                return; // Error already sent on channel.
            };

            let _ = tx
                .send(Ok(event_mapper::container_status_event(
                    "ready",
                    "Agent ready",
                )))
                .await;

            // Detect stale session: if the pod was recreated (name changed),
            // the old OpenCode session is invalid — force a new one.
            let mut reusable_session = existing_opencode_session.clone();
            if let Some(ref old_name) = existing_pod_name
                && *old_name != pod_name
            {
                info!(
                    old_pod = %old_name,
                    new_pod = %pod_name,
                    "Pod recreated, clearing stale OpenCode session"
                );
                reusable_session = None;
                // Clear the stale session ID in the DB.
                let _ = repos
                    .reasoning
                    .update_container_status(conv_id, Some(&pod_name), "active", None)
                    .await;
            }

            // 3. Connect to OpenCode and stream.
            info!(pod_ip = %pod_ip, "Connecting to OpenCode");
            let client = match ps_agent::ContainerManager::opencode_client(&pod_ip) {
                Ok(c) => c,
                Err(e) => {
                    error!(error = %e, "Failed to create OpenCode client");
                    let _ = tx
                        .send(Ok(event_mapper::container_status_event(
                            "error",
                            &format!("Failed to connect to agent: {e}"),
                        )))
                        .await;
                    return;
                }
            };

            // Reuse existing OpenCode session for follow-up questions, or create new.
            let opencode_session_id = if let Some(ref oc_sid) = reusable_session {
                info!(session_id = %oc_sid, "Reusing existing OpenCode session");
                oc_sid.clone()
            } else {
                info!("Creating new OpenCode session");
                match client.create_session_with_title(&req.question).await {
                    Ok(s) => {
                        info!(session_id = %s.id, "OpenCode session created");
                        // Store the session ID so follow-up questions reuse it.
                        let _ = repos
                            .reasoning
                            .update_container_status(
                                conv_id,
                                Some(&pod_name),
                                "active",
                                Some(&s.id),
                            )
                            .await;
                        s.id
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to create OpenCode session");
                        let _ = tx
                            .send(Ok(event_mapper::container_status_event(
                                "error",
                                &format!("Failed to create agent session: {e}"),
                            )))
                            .await;
                        return;
                    }
                }
            };

            // 4. Subscribe to global events (not session-filtered) before sending prompt.
            // Session-filtered subscription via subscribe_session can miss events
            // that arrive before the router fully initialises.
            info!("Subscribing to OpenCode events");
            let mut subscription = match client.subscribe().await {
                Ok(s) => {
                    info!("SSE subscription established");
                    s
                }
                Err(e) => {
                    error!(error = %e, "Failed to subscribe to OpenCode events");
                    let _ = tx
                        .send(Ok(event_mapper::container_status_event(
                            "error",
                            &format!("Failed to subscribe to agent events: {e}"),
                        )))
                        .await;
                    return;
                }
            };

            // 5. Send the question, specifying the "prism" agent so OpenCode
            //    uses our custom system prompt and MCP tool configuration.
            info!("Sending question to OpenCode");
            let prompt = ps_agent::opencode_sdk::types::message::PromptRequest::text(&req.question)
                .with_agent("prism");
            if let Err(e) = client
                .messages()
                .prompt_async(&opencode_session_id, &prompt)
                .await
            {
                error!(error = %e, "Failed to send prompt to OpenCode");
                let _ = tx
                    .send(Ok(event_mapper::container_status_event(
                        "error",
                        &format!("Failed to send question: {e}"),
                    )))
                    .await;
                return;
            }
            info!("Question sent, streaming events");

            // 6. Stream events until idle or timeout (5 minutes — long scripts may need it).
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
            let mut answer_text = String::new();
            let mut tool_calls = 0i32;

            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }

                let event = match tokio::time::timeout(remaining, subscription.recv()).await {
                    Ok(Some(event)) => event,
                    Ok(None) => {
                        let stats = subscription.stats();
                        info!(
                            events_in = stats.events_in,
                            events_out = stats.events_out,
                            reconnects = stats.reconnects,
                            "SSE subscription closed (None)"
                        );
                        break;
                    }
                    Err(_) => {
                        let stats = subscription.stats();
                        warn!(
                            events_in = stats.events_in,
                            events_out = stats.events_out,
                            reconnects = stats.reconnects,
                            "SSE stream timed out"
                        );
                        break;
                    }
                };

                // Check for idle/completion.
                if matches!(
                    event,
                    ps_agent::opencode_sdk::types::event::Event::SessionIdle { .. }
                ) {
                    info!("Session idle, finishing");
                    break;
                }

                // Intercept artifact uploads: when the upload_artifact MCP tool
                // completes, register the artifact in the DB and emit an event.
                if let ps_agent::opencode_sdk::types::event::Event::MessagePartUpdated {
                    properties,
                } = &event
                    && let Some(ps_agent::opencode_sdk::types::message::Part::Tool {
                        tool,
                        state:
                            Some(ps_agent::opencode_sdk::types::message::ToolState::Completed(
                                completed,
                            )),
                        ..
                    }) = properties.part.as_ref()
                    && tool == "prism_upload_artifact"
                    && let Ok(result) = serde_json::from_str::<serde_json::Value>(&completed.output)
                {
                    // The MCP tool returns keys like "conversations/{session}/{file}"
                    // but ArtifactKey::new(Conversations, path) already prepends
                    // "conversations/", so strip the prefix to avoid doubling it.
                    let raw_key = result
                        .get("artifact_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let artifact_key = raw_key.strip_prefix("conversations/").unwrap_or(raw_key);
                    let display_name = result
                        .get("display_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("artifact");
                    let content_type = result.get("content_type").and_then(|v| v.as_str());
                    let size_bytes = result
                        .get("size_bytes")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0);

                    match repos
                        .reasoning
                        .create_artifact(&ps_core::repo::reasoning::CreateArtifactParams {
                            conversation_id: conv_id,
                            message_id: None,
                            artifact_key,
                            display_name,
                            content_type,
                            size_bytes,
                        })
                        .await
                    {
                        Ok(artifact) => {
                            let uploaded = ps_proto::canonical::prism::v1::AgentArtifactUploaded {
                                artifact_id: artifact.id.to_string(),
                                display_name: display_name.to_string(),
                                content_type: content_type
                                    .unwrap_or("application/octet-stream")
                                    .to_string(),
                                size_bytes,
                                download_url: String::new(),
                            };
                            let _ = tx
                                .send(Ok(AskQuestionResponse {
                                    event: Some(
                                        ps_proto::canonical::prism::v1::ask_question_response::Event::ArtifactUploaded(uploaded),
                                    ),
                                }))
                                .await;
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to register artifact in DB");
                        }
                    }
                }

                // Map to proto and send.
                if let Some(proto_event) = event_mapper::map_event(&event) {
                    // Track answer text and tool calls.
                    if let Some(ref evt) = proto_event.event {
                        match evt {
                            ps_proto::canonical::prism::v1::ask_question_response::Event::PartialAnswer(a) => {
                                answer_text.clone_from(&a.text);
                            }
                            ps_proto::canonical::prism::v1::ask_question_response::Event::ToolCallCompleted(_) => {
                                tool_calls += 1;
                            }
                            _ => {}
                        }
                    }
                    if tx.send(Ok(proto_event)).await.is_err() {
                        break; // Client disconnected.
                    }
                }
            }

            // Update pod activity to prevent premature reaping.
            if let Err(e) = cm.update_activity(&conv_id.to_string()).await {
                warn!(error = %e, "Failed to update pod activity");
            }

            // 7. Store assistant message and update totals.
            let trace = serde_json::json!({
                "tool_call_count": tool_calls,
            });
            let _ = repos
                .reasoning
                .create_message(&CreateMessageParams {
                    conversation_id: conv_id,
                    role: "assistant",
                    content: &answer_text,
                    reasoning_trace: Some(&trace),
                    supporting_data: None,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                })
                .await;
            let _ = repos
                .reasoning
                .update_conversation_totals(conv_id, tool_calls, 0, 0, 0.0)
                .await;

            // 8. Send final answer.
            let _ = tx
                .send(Ok(AskQuestionResponse {
                    event: Some(
                        ps_proto::canonical::prism::v1::ask_question_response::Event::FinalAnswer(
                            ps_proto::canonical::prism::v1::AgentFinalAnswer {
                                answer: answer_text,
                                conversation_id: conv_id.to_string(),
                                supporting_data_json: String::new(),
                                prompt_tokens: 0,
                                completion_tokens: 0,
                                estimated_cost_usd: 0.0,
                                tool_call_count: tool_calls,
                                duration_ms: 0,
                                artifacts: vec![],
                            },
                        ),
                    ),
                }))
                .await;
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    async fn list_conversations(
        &self,
        request: Request<ListConversationsRequest>,
    ) -> Result<Response<ListConversationsResponse>, Status> {
        let ctx = require_auth(&request)?;
        let req = request.into_inner();

        let page_size = i64::from(req.page_size.clamp(1, 100));
        let offset = i64::from((req.page - 1).max(0)) * page_size;

        let (convs, total) = self
            .repos
            .reasoning
            .list_conversations(ctx.user_id, page_size, offset)
            .await
            .map_err(db_err)?;

        let conversations = convs
            .into_iter()
            .map(|c| ps_proto::canonical::prism::v1::ConversationSummary {
                id: c.id.to_string(),
                title: c.title,
                status: c.status,
                model_name: c.model_name,
                container_status: c.container_status,
                total_tool_calls: c.total_tool_calls,
                total_estimated_cost_usd: c.total_estimated_cost_usd,
                message_count: c.message_count.try_into().unwrap_or(0),
                artifact_count: c.artifact_count.try_into().unwrap_or(0),
                created_at: Some(to_timestamp(c.created_at)),
                last_activity_at: Some(to_timestamp(c.last_activity_at)),
            })
            .collect();

        Ok(Response::new(ListConversationsResponse {
            conversations,
            total_count: total.try_into().unwrap_or(0),
        }))
    }

    async fn get_conversation(
        &self,
        request: Request<GetConversationRequest>,
    ) -> Result<Response<GetConversationResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let conv_id: Uuid = req
            .conversation_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

        let conv = self
            .repos
            .reasoning
            .get_conversation(conv_id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("conversation not found"))?;

        let (messages_list, artifacts_list) = tokio::try_join!(
            async {
                self.repos
                    .reasoning
                    .list_messages(conv_id)
                    .await
                    .map_err(db_err)
            },
            async {
                self.repos
                    .reasoning
                    .list_artifacts(conv_id)
                    .await
                    .map_err(db_err)
            },
        )?;

        let summary = ps_proto::canonical::prism::v1::ConversationSummary {
            id: conv.id.to_string(),
            title: conv.title,
            status: conv.status,
            model_name: conv.model_name,
            container_status: conv.container_status,
            total_tool_calls: conv.total_tool_calls,
            total_estimated_cost_usd: conv.total_estimated_cost_usd,
            message_count: messages_list.len().try_into().unwrap_or(0),
            artifact_count: artifacts_list.len().try_into().unwrap_or(0),
            created_at: Some(to_timestamp(conv.created_at)),
            last_activity_at: Some(to_timestamp(conv.last_activity_at)),
        };

        let messages = messages_list
            .into_iter()
            .map(|m| ps_proto::canonical::prism::v1::ConversationMessage {
                id: m.id.to_string(),
                role: m.role,
                content: m.content,
                reasoning_trace_json: m.reasoning_trace.map(|v| v.to_string()),
                supporting_data_json: m.supporting_data.map(|v| v.to_string()),
                prompt_tokens: m.prompt_tokens,
                completion_tokens: m.completion_tokens,
                created_at: Some(to_timestamp(m.created_at)),
            })
            .collect();

        let artifacts = artifacts_list
            .into_iter()
            .map(|a| ps_proto::canonical::prism::v1::ConversationArtifact {
                id: a.id.to_string(),
                message_id: a.message_id.map(|id| id.to_string()),
                artifact_key: a.artifact_key,
                display_name: a.display_name,
                content_type: a.content_type,
                size_bytes: a.size_bytes,
                created_at: Some(to_timestamp(a.created_at)),
            })
            .collect();

        Ok(Response::new(GetConversationResponse {
            conversation: Some(summary),
            messages,
            artifacts,
        }))
    }

    async fn save_insight_from_conversation(
        &self,
        _request: Request<SaveInsightFromConversationRequest>,
    ) -> Result<Response<SaveInsightFromConversationResponse>, Status> {
        // This requires the insights repo integration which is a deeper
        // integration — stub for now until the insight creation flow is defined.
        Err(Status::unimplemented(
            "SaveInsightFromConversation will be available in a future update",
        ))
    }

    async fn get_artifact_download_url(
        &self,
        request: Request<GetArtifactDownloadUrlRequest>,
    ) -> Result<Response<GetArtifactDownloadUrlResponse>, Status> {
        use base64::Engine;
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let artifact_id: Uuid = req
            .artifact_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid artifact_id"))?;

        let artifact = self
            .repos
            .reasoning
            .get_artifact(artifact_id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Status::not_found("artifact not found"))?;

        let store = self
            .artifact_store
            .as_ref()
            .ok_or_else(|| Status::unavailable("artifact storage not configured"))?;

        let key = ps_core::artifact_store::ArtifactKey::new(
            ps_core::artifact_store::ArtifactCategory::Conversations,
            &artifact.artifact_key,
        );

        // Proxy the download: read bytes from S3 and return as a data URL.
        // Presigned URLs don't work because the internal S3 hostname isn't
        // reachable from the browser.
        let data = store.get(&key).await.map_err(|e| {
            error!(error = %e, "Failed to read artifact from S3");
            Status::internal("failed to read artifact")
        })?;

        let content_type = artifact
            .content_type
            .as_deref()
            .unwrap_or("application/octet-stream");

        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
        let download_url = format!("data:{content_type};base64,{b64}");

        Ok(Response::new(GetArtifactDownloadUrlResponse {
            download_url,
            expires_in_seconds: 0,
        }))
    }
}

/// Poll for Pod readiness with backoff, sending status events on the channel.
///
/// Returns `(pod_ip, pod_name)` on success, or `None` if the pod failed to start.
async fn wait_for_pod_ready(
    cm: &ps_agent::ContainerManager,
    session_id: &str,
    tx: &tokio::sync::mpsc::Sender<Result<AskQuestionResponse, Status>>,
) -> Option<(String, String)> {
    use ps_agent::{PodStatus, event_mapper};

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

    loop {
        interval.tick().await;
        if tokio::time::Instant::now() >= deadline {
            let _ = tx
                .send(Ok(event_mapper::container_status_event(
                    "error",
                    "Timed out waiting for agent container",
                )))
                .await;
            return None;
        }

        match cm.get_pod_status(session_id).await {
            Ok(PodStatus::Running { pod_ip, pod_name }) => return Some((pod_ip, pod_name)),
            Ok(PodStatus::Pending) => {}
            Ok(PodStatus::Gone) => {
                let _ = tx
                    .send(Ok(event_mapper::container_status_event(
                        "error",
                        "Agent container failed to start",
                    )))
                    .await;
                return None;
            }
            Err(e) => {
                error!(error = %e, "Error checking pod status");
                let _ = tx
                    .send(Ok(event_mapper::container_status_event(
                        "error",
                        &format!("Error checking container status: {e}"),
                    )))
                    .await;
                return None;
            }
        }
    }
}

fn similar_to_proto(s: ps_core::repo::reasoning::SimilarContribution) -> ProtoSimilarItem {
    let (platform, platform_instance) = platform_to_proto(&s.platform);
    ProtoSimilarItem {
        contribution_id: s.contribution_id.to_string(),
        title: s.title.unwrap_or_default(),
        platform,
        contribution_type: contribution_type_to_proto(&s.contribution_type),
        state: contribution_state_to_proto(s.state.as_deref().unwrap_or("")),
        platform_instance,
        author_name: s.author_name.unwrap_or_default(),
        external_url: s.external_url.unwrap_or_default(),
        distance: s.distance,
        created_at: Some(to_timestamp(s.created_at)),
    }
}

fn enrichment_to_proto(e: ps_core::repo::reasoning::EnrichmentRecord) -> ProtoEnrichment {
    ProtoEnrichment {
        id: e.id.to_string(),
        contribution_id: e.contribution_id.to_string(),
        enrichment_type: enrichment_type_to_proto(&e.enrichment_type),
        value_json: e.value.to_string(),
        model_name: e.model_name,
        confidence: e.confidence,
        input_hash: e.input_hash,
        input_preview: e.input_preview,
        created_at: Some(to_timestamp(e.created_at)),
    }
}
