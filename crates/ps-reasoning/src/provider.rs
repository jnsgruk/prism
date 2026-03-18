use crate::types::{CompletionRequest, CompletionResponse};

/// Abstraction over AI model providers (Google Gemini, `OpenRouter`, etc.).
///
/// Each provider implementation translates our unified request/response types
/// into the provider's native API format.
#[async_trait::async_trait]
pub trait ModelProvider: Send + Sync {
    /// Send a completion (chat) request.
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, ProviderError>;

    /// Generate embeddings for a batch of texts.
    async fn embed(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError>;

    /// Provider name (e.g. "google", "openrouter").
    fn name(&self) -> &str;
}

/// Errors from provider API calls.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("failed to parse response: {0}")]
    Parse(String),

    #[error("rate limited — retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    #[error("provider does not support this operation")]
    Unsupported,

    #[error("budget exceeded: daily spend ${current:.2} >= cap ${cap:.2}")]
    BudgetExceeded { current: f64, cap: f64 },

    #[error("{0}")]
    Other(String),
}
