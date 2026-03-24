use ps_core::models::{AiProvider, TaskType};
use rig::client::{CompletionClient as _, EmbeddingsClient as _};
use rig::completion::CompletionModel as _;
#[allow(deprecated)]
use rig::embeddings::EmbeddingModelDyn;
use rig::providers::{gemini, openrouter};
use tracing::info;

use crate::types::{AiConfig, AiTaskConfig, AiTaskRouting};

/// Lightweight model used for Google connection tests.
const GOOGLE_TEST_MODEL: &str = "gemini-2.5-flash";

/// Lightweight model used for `OpenRouter` connection tests.
const OPENROUTER_TEST_MODEL: &str = "openai/gpt-4.1-nano";

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

/// Routes AI tasks to the correct Rig provider client based on configuration.
///
/// Holds Rig provider clients and the current routing config. Call `update_config`
/// when the admin changes AI settings.
pub struct TaskRouter {
    google: Option<gemini::Client>,
    openrouter: Option<openrouter::Client>,
    config: AiConfig,
    /// Raw API keys — kept alongside clients so we can inject them into agent
    /// container env vars without needing to re-decrypt from the database.
    google_key: Option<String>,
    openrouter_key: Option<String>,
}

impl TaskRouter {
    /// Create a new router with the given config but no providers yet.
    /// Call `set_google` / `set_openrouter` to register providers.
    pub fn new(config: AiConfig) -> Self {
        Self {
            google: None,
            openrouter: None,
            config,
            google_key: None,
            openrouter_key: None,
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

    /// Set the `OpenRouter` provider client.
    pub fn set_openrouter(&mut self, api_key: &str) {
        match openrouter::Client::new(api_key) {
            Ok(client) => {
                self.openrouter = Some(client);
                self.openrouter_key = Some(api_key.to_string());
            }
            Err(e) => tracing::warn!(error = %e, "failed to create OpenRouter client"),
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
        if let Some(key) = &self.openrouter_key {
            vars.push(("OPENROUTER_API_KEY".to_string(), key.clone()));
        }
        vars
    }

    /// Get the configured budget cap in USD (daily).
    pub fn budget_cap_usd(&self) -> Option<f64> {
        self.config.budget_cap_usd
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

    /// Get the `OpenRouter` client, if configured.
    pub fn openrouter_client(&self) -> Result<&openrouter::Client, ProviderError> {
        self.openrouter.as_ref().ok_or_else(|| {
            ProviderError::NotConfigured(
                "OpenRouter provider not configured (missing API key)".into(),
            )
        })
    }

    /// Resolve the provider for a given task type as a `ResolvedProvider`.
    pub fn resolve_provider(&self, task: TaskType) -> Result<ResolvedProvider<'_>, ProviderError> {
        let task_config = self.config.tasks.get(task);
        match task_config.provider {
            AiProvider::Google => Ok(ResolvedProvider::Google(self.google_client()?)),
            AiProvider::OpenRouter => Ok(ResolvedProvider::OpenRouter(self.openrouter_client()?)),
        }
    }

    /// Build an embedding model for the configured embeddings task.
    ///
    /// Returns a boxed `EmbeddingModelDyn` for dynamic dispatch (the concrete
    /// Gemini/OpenRouter embedding model types differ).
    #[allow(deprecated)]
    pub fn embedding_model(&self) -> Result<Box<dyn EmbeddingModelDyn>, ProviderError> {
        let task_config = self.config.tasks.get(TaskType::Embeddings);
        match task_config.provider {
            AiProvider::Google => {
                let client = self.google_client()?;
                let model = client.embedding_model(&task_config.model);
                Ok(Box::new(model))
            }
            AiProvider::OpenRouter => {
                // OpenRouter doesn't support embeddings — fall back to Google
                // or return an error. For now, error out clearly.
                Err(ProviderError::NotConfigured(
                    "OpenRouter does not support embedding models; configure Google for embeddings"
                        .into(),
                ))
            }
        }
    }

    /// Test a specific provider's connection by making a minimal completion request.
    pub async fn test_provider(&self, provider: AiProvider) -> Result<(), ProviderError> {
        match provider {
            AiProvider::Google => {
                let client = self.google_client()?;
                let model = client.completion_model(GOOGLE_TEST_MODEL);
                let req = model.completion_request(TEST_PROMPT).max_tokens(10).build();
                let _resp = model.completion(req).await?;
                Ok(())
            }
            AiProvider::OpenRouter => {
                let client = self.openrouter_client()?;
                let model = client.completion_model(OPENROUTER_TEST_MODEL);
                let req = model.completion_request(TEST_PROMPT).max_tokens(10).build();
                let _resp = model.completion(req).await?;
                Ok(())
            }
        }
    }
}

/// Enum for dispatching to the correct provider client.
///
/// Callers match on this to get the concrete client type and build
/// Rig completion models, agents, extractors, etc. from it.
pub enum ResolvedProvider<'a> {
    Google(&'a gemini::Client),
    OpenRouter(&'a openrouter::Client),
}
