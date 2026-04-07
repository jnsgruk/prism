use ps_core::models::{AiProvider, TaskType};
use rig::client::{CompletionClient as _, EmbeddingsClient as _};
use rig::completion::CompletionModel as _;
#[allow(deprecated)]
use rig::embeddings::EmbeddingModelDyn;
use rig::providers::gemini;
use tracing::info;

use crate::types::{AiConfig, AiTaskConfig, AiTaskRouting};

/// Lightweight model used for Google connection tests.
const GOOGLE_TEST_MODEL: &str = "gemini-2.5-flash";

/// Minimal prompt for connection tests.
const TEST_PROMPT: &str = "Say hello in one word.";

/// Error type for provider routing.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("provider not configured: {0}")]
    NotConfigured(String),

    #[error("provider error: {0}")]
    Completion(#[from] rig::completion::CompletionError),

    #[error("budget exceeded: daily spend ${current:.2} >= cap ${cap:.2}")]
    BudgetExceeded { current: f64, cap: f64 },
}

/// Routes AI tasks to the Gemini provider client based on configuration.
///
/// Holds the Rig Gemini client and the current routing config. Call `update_config`
/// when the admin changes AI settings.
pub struct TaskRouter {
    google: Option<gemini::Client>,
    config: AiConfig,
    /// Raw API key — kept alongside client so we can inject it into agent
    /// container env vars without needing to re-decrypt from the database.
    google_key: Option<String>,
}

impl TaskRouter {
    /// Create a new router with the given config but no provider yet.
    /// Call `set_google` to register the provider.
    pub fn new(config: AiConfig) -> Self {
        Self {
            google: None,
            config,
            google_key: None,
        }
    }

    /// Set the Google Gemini provider client.
    pub fn set_google(&mut self, api_key: &str) {
        match gemini::Client::new(api_key) {
            Ok(client) => {
                self.google = Some(client);
                self.google_key = Some(api_key.to_string());
            }
            Err(e) => tracing::warn!(error = %e, "failed to create Gemini client"),
        }
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

    /// Return provider API keys as `(ENV_VAR_NAME, value)` pairs suitable for
    /// injecting into agent container Pods.
    pub fn provider_env_vars(&self) -> Vec<(String, String)> {
        let mut vars = Vec::new();
        if let Some(key) = &self.google_key {
            vars.push(("GOOGLE_API_KEY".to_string(), key.clone()));
            vars.push(("GEMINI_API_KEY".to_string(), key.clone()));
            vars.push(("GOOGLE_GENERATIVE_AI_API_KEY".to_string(), key.clone()));
        }
        vars
    }

    /// Get the task config for a given task type.
    pub fn task_config(&self, task: TaskType) -> &AiTaskConfig {
        self.config.tasks.get(task)
    }

    /// Get the Google Gemini client, if configured.
    pub fn google_client(&self) -> Result<&gemini::Client, ProviderError> {
        self.google.as_ref().ok_or_else(|| {
            ProviderError::NotConfigured("Google provider not configured (missing API key)".into())
        })
    }

    /// Resolve the provider for a given task type.
    pub fn resolve_provider(&self, _task: TaskType) -> Result<&gemini::Client, ProviderError> {
        self.google_client()
    }

    /// Build an embedding model for the configured embeddings task.
    ///
    /// Returns a boxed `EmbeddingModelDyn` for dynamic dispatch.
    #[allow(deprecated)]
    pub fn embedding_model(&self) -> Result<Box<dyn EmbeddingModelDyn>, ProviderError> {
        let task_config = self.config.tasks.get(TaskType::Embeddings);
        let client = self.google_client()?;
        let model = client.embedding_model(&task_config.model);
        Ok(Box::new(model))
    }

    /// Test the provider's connection by making a minimal completion request.
    pub async fn test_provider(&self, _provider: AiProvider) -> Result<(), ProviderError> {
        let client = self.google_client()?;
        let model = client.completion_model(GOOGLE_TEST_MODEL);
        let req = model.completion_request(TEST_PROMPT).max_tokens(10).build();
        let _resp = model.completion(req).await?;
        Ok(())
    }
}
