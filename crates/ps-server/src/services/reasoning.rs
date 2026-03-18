use std::sync::Arc;

use ps_core::crypto;
use ps_core::repo::Repos;
use ps_proto::prism::v1::reasoning_service_server::ReasoningService;
use ps_proto::prism::v1::{
    AiSettings, AiTaskConfig as ProtoAiTaskConfig, DeleteEnrichmentsByTypeRequest,
    DeleteEnrichmentsByTypeResponse, Enrichment as ProtoEnrichment, EnrichmentTypeCount,
    GetAiSettingsRequest, GetAiSettingsResponse, GetCostSummaryRequest, GetCostSummaryResponse,
    GetEnrichmentPipelineStatusRequest, GetEnrichmentPipelineStatusResponse,
    GetEnrichmentsByContributionsRequest, GetEnrichmentsByContributionsResponse,
    GetEnrichmentsRequest, GetEnrichmentsResponse, GetStorageHealthRequest,
    GetStorageHealthResponse, SetProviderSecretRequest, SetProviderSecretResponse,
    TestProviderRequest, TestProviderResponse, TriggerEnrichmentRequest, TriggerEnrichmentResponse,
    UpdateAiSettingsRequest, UpdateAiSettingsResponse,
};
use ps_reasoning::types::{AiConfig, AiTaskConfig};
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::{error, info};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::common::{db_err, require_auth};

pub struct ReasoningServiceImpl {
    repos: Repos,
    secret_key: Zeroizing<[u8; 32]>,
    router: Arc<RwLock<ps_reasoning::routing::TaskRouter>>,
    artifact_store: Option<Arc<dyn ps_core::ArtifactStore>>,
}

impl ReasoningServiceImpl {
    pub fn new(
        repos: Repos,
        secret_key: Zeroizing<[u8; 32]>,
        router: Arc<RwLock<ps_reasoning::routing::TaskRouter>>,
        artifact_store: Option<Arc<dyn ps_core::ArtifactStore>>,
    ) -> Self {
        Self {
            repos,
            secret_key,
            router,
            artifact_store,
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
        provider: tc.provider.to_string(),
        model: tc.model.clone(),
    }
}

fn proto_to_task_config(p: &ProtoAiTaskConfig) -> Option<AiTaskConfig> {
    let provider = p.provider.parse().ok()?;
    Some(AiTaskConfig {
        provider,
        model: p.model.clone(),
    })
}

/// Secret key name for a provider.
#[allow(clippy::result_large_err)]
fn provider_secret_key(provider: &str) -> Result<&'static str, Status> {
    match provider {
        "google" => Ok("google_api_key"),
        "openrouter" => Ok("openrouter_api_key"),
        _ => Err(Status::invalid_argument(format!(
            "unknown provider: {provider}"
        ))),
    }
}

#[tonic::async_trait]
impl ReasoningService for ReasoningServiceImpl {
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

        let secret_key_name = provider_secret_key(&req.provider)?;

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
            match req.provider.as_str() {
                "google" => router.set_google(&req.secret_value),
                "openrouter" => router.set_openrouter(&req.secret_value),
                _ => {}
            }
        }

        info!(provider = %req.provider, "provider secret set");

        Ok(Response::new(SetProviderSecretResponse {}))
    }

    async fn test_provider(
        &self,
        request: Request<TestProviderRequest>,
    ) -> Result<Response<TestProviderResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let provider: ps_core::models::AiProvider = req
            .provider
            .parse()
            .map_err(|_| Status::invalid_argument(format!("unknown provider: {}", req.provider)))?;

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

        let daily_spend: Vec<ps_proto::prism::v1::DailySpend> = daily_series
            .into_iter()
            .map(|d| ps_proto::prism::v1::DailySpend {
                date: d.date.to_string(),
                cost_usd: d.total_cost_usd,
                request_count: d.request_count,
            })
            .collect();

        let task_breakdown: Vec<ps_proto::prism::v1::TaskSpend> = task_breakdown
            .into_iter()
            .map(|t| ps_proto::prism::v1::TaskSpend {
                task_type: t.task_type,
                cost_usd: t.total_cost_usd,
                prompt_tokens: t.total_prompt_tokens,
                completion_tokens: t.total_completion_tokens,
                request_count: t.request_count,
            })
            .collect();

        let model_breakdown: Vec<ps_proto::prism::v1::ModelSpend> = model_breakdown
            .into_iter()
            .map(|m| ps_proto::prism::v1::ModelSpend {
                provider: m.provider,
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
            last_enrichment_at: status.last_enrichment_at.map(|t| {
                t.format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default()
            }),
            by_type: status
                .by_type
                .into_iter()
                .map(|t| EnrichmentTypeCount {
                    enrichment_type: t.enrichment_type,
                    count: t.total_count,
                })
                .collect(),
        }))
    }

    async fn trigger_enrichment(
        &self,
        request: Request<TriggerEnrichmentRequest>,
    ) -> Result<Response<TriggerEnrichmentResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let batch_size = if req.batch_size > 0 {
            i64::from(req.batch_size)
        } else {
            50
        };

        // Create a handler run record so enrichment shows in the runs tables
        let run_id = Uuid::now_v7();
        if let Err(e) = self
            .repos
            .activity
            .create_run(run_id, "_enrichment", "EnrichmentHandler", "run_cycle")
            .await
        {
            error!(error = %e, "failed to create enrichment run record");
        }

        let router = self.router.read().await;
        let cost_tracker = ps_reasoning::cost::CostTracker::new(self.repos.reasoning.clone());

        let results = ps_reasoning::features::enrichment::run_enrichment_cycle(
            &router,
            &self.repos.reasoning,
            &cost_tracker,
            batch_size,
        )
        .await;

        let total_processed: usize = results.iter().map(|r| r.processed).sum();
        let total_errors: usize = results.iter().map(|r| r.errors).sum();

        let message = format!("processed {total_processed}, errors {total_errors}");

        // Complete or fail the run record
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        if total_errors > 0 && total_processed == 0 {
            let _ = self.repos.activity.fail_run(run_id, &message).await;
        } else {
            let _ = self
                .repos
                .activity
                .complete_run(run_id, total_processed as i32)
                .await;
        }

        info!(
            processed = total_processed,
            errors = total_errors,
            "enrichment cycle complete"
        );

        Ok(Response::new(TriggerEnrichmentResponse {
            triggered: true,
            message,
        }))
    }

    async fn delete_enrichments_by_type(
        &self,
        request: Request<DeleteEnrichmentsByTypeRequest>,
    ) -> Result<Response<DeleteEnrichmentsByTypeResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        if req.enrichment_type.is_empty() {
            return Err(Status::invalid_argument("enrichment_type is required"));
        }

        let deleted = self
            .repos
            .reasoning
            .delete_enrichments_by_type(&req.enrichment_type)
            .await
            .map_err(db_err)?;

        info!(enrichment_type = %req.enrichment_type, deleted, "enrichments deleted for re-enrichment");

        Ok(Response::new(DeleteEnrichmentsByTypeResponse {
            #[allow(clippy::cast_possible_wrap)]
            deleted_count: deleted as i64,
        }))
    }
}

fn enrichment_to_proto(e: ps_core::repo::reasoning::EnrichmentRecord) -> ProtoEnrichment {
    ProtoEnrichment {
        id: e.id.to_string(),
        contribution_id: e.contribution_id.to_string(),
        enrichment_type: e.enrichment_type,
        value_json: e.value.to_string(),
        model_name: e.model_name,
        confidence: e.confidence,
        input_hash: e.input_hash,
        input_preview: e.input_preview,
        created_at: e
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    }
}
