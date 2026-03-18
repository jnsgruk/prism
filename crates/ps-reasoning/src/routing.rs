use std::sync::Arc;

use ps_core::models::{AiProvider, TaskType};
use tracing::info;

use crate::provider::{ModelProvider, ProviderError};
use crate::providers::google::GoogleProvider;
use crate::providers::openrouter::OpenRouterProvider;
use crate::types::{AiConfig, AiTaskConfig, AiTaskRouting, CompletionRequest, CompletionResponse};

/// Routes AI tasks to the correct provider and model based on configuration.
///
/// Holds provider instances and the current routing config. Call `update_config`
/// when the admin changes AI settings.
pub struct TaskRouter {
    google: Option<Arc<GoogleProvider>>,
    openrouter: Option<Arc<OpenRouterProvider>>,
    config: AiConfig,
}

impl TaskRouter {
    /// Create a new router with the given config but no providers yet.
    /// Call `set_google` / `set_openrouter` to register providers.
    pub fn new(config: AiConfig) -> Self {
        Self {
            google: None,
            openrouter: None,
            config,
        }
    }

    /// Set the Google provider (typically after decrypting the API key).
    pub fn set_google(&mut self, provider: GoogleProvider) {
        self.google = Some(Arc::new(provider));
    }

    /// Set the `OpenRouter` provider.
    pub fn set_openrouter(&mut self, provider: OpenRouterProvider) {
        self.openrouter = Some(Arc::new(provider));
    }

    /// Update the routing configuration.
    pub fn update_config(&mut self, config: AiConfig) {
        info!("AI task routing config updated");
        self.config = config;
    }

    /// Get the current AI config.
    pub fn config(&self) -> &AiConfig {
        &self.config
    }

    /// Get the task routing config.
    pub fn routing(&self) -> &AiTaskRouting {
        &self.config.tasks
    }

    /// Get the configured budget cap in USD (daily).
    pub fn budget_cap_usd(&self) -> Option<f64> {
        self.config.budget_cap_usd
    }

    /// Resolve the provider for a given task type.
    fn resolve_provider(&self, task: TaskType) -> Result<&dyn ModelProvider, ProviderError> {
        let task_config = self.config.tasks.get(task);
        match task_config.provider {
            AiProvider::Google => self
                .google
                .as_ref()
                .map(|p| p.as_ref() as &dyn ModelProvider)
                .ok_or_else(|| {
                    ProviderError::Other("Google provider not configured (missing API key)".into())
                }),
            AiProvider::OpenRouter => self
                .openrouter
                .as_ref()
                .map(|p| p.as_ref() as &dyn ModelProvider)
                .ok_or_else(|| {
                    ProviderError::Other(
                        "OpenRouter provider not configured (missing API key)".into(),
                    )
                }),
        }
    }

    /// Get the task config for a given task type.
    pub fn task_config(&self, task: TaskType) -> &AiTaskConfig {
        self.config.tasks.get(task)
    }

    /// Send a completion request for a specific task type, routing to
    /// the configured provider and model.
    pub async fn complete(
        &self,
        task: TaskType,
        mut req: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let task_config = self.config.tasks.get(task);
        let provider = self.resolve_provider(task)?;

        // Override model with the configured one for this task
        req.model = task_config.model.clone();

        provider.complete(req).await
    }

    /// Generate embeddings using the configured embeddings provider/model.
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
        let task_config = self.config.tasks.get(TaskType::Embeddings);
        let provider = self.resolve_provider(TaskType::Embeddings)?;
        provider.embed(&task_config.model, texts).await
    }

    /// Test a specific provider's connection.
    pub async fn test_provider(&self, provider: AiProvider) -> Result<(), ProviderError> {
        match provider {
            AiProvider::Google => {
                let p = self
                    .google
                    .as_ref()
                    .ok_or_else(|| ProviderError::Other("Google provider not configured".into()))?;
                p.test_connection().await
            }
            AiProvider::OpenRouter => {
                let p = self.openrouter.as_ref().ok_or_else(|| {
                    ProviderError::Other("OpenRouter provider not configured".into())
                })?;
                p.test_connection().await
            }
        }
    }
}
