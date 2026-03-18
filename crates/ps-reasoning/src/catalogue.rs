//! Fetch available models from AI provider APIs.
//!
//! Each provider has a `fetch_models` function that calls the provider's
//! model-listing endpoint and returns a normalized `Vec<AiModel>`.

use std::fmt::Write as _;

use ps_core::models::{AiModel, AiProvider};
use tracing::debug;

/// Errors that can occur when fetching a provider's model catalogue.
#[derive(Debug, thiserror::Error)]
pub enum CatalogueError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("unexpected response: {0}")]
    Parse(String),
}

/// Fetch the model catalogue for a given provider.
pub async fn fetch_models(
    http: &reqwest::Client,
    provider: AiProvider,
    api_key: &str,
) -> Result<Vec<AiModel>, CatalogueError> {
    match provider {
        AiProvider::Google => fetch_google_models(http, api_key).await,
        AiProvider::OpenRouter => fetch_openrouter_models(http, api_key).await,
    }
}

// ---------------------------------------------------------------------------
// Google Gemini
// ---------------------------------------------------------------------------

/// Response shape from the Gemini `models.list` endpoint.
#[derive(serde::Deserialize)]
struct GeminiListResponse {
    models: Vec<GeminiModel>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiModel {
    /// e.g. "models/gemini-2.5-flash"
    name: String,
    display_name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    input_token_limit: Option<i32>,
    #[serde(default)]
    #[allow(dead_code)]
    output_token_limit: Option<i32>,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
}

async fn fetch_google_models(
    http: &reqwest::Client,
    api_key: &str,
) -> Result<Vec<AiModel>, CatalogueError> {
    let mut all_models = Vec::new();
    let mut page_token: Option<String> = None;

    loop {
        let mut url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models?key={api_key}&pageSize=100"
        );
        if let Some(ref token) = page_token {
            let _ = write!(url, "&pageToken={token}");
        }

        let resp: GeminiListResponse = http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        for m in resp.models {
            // Strip "models/" prefix to get the usable model ID
            let id = m.name.strip_prefix("models/").unwrap_or(&m.name);

            let mut capabilities = Vec::new();
            for method in &m.supported_generation_methods {
                match method.as_str() {
                    "generateContent" => {
                        if !capabilities.contains(&"completion".to_string()) {
                            capabilities.push("completion".into());
                        }
                        // Flash and Pro models support tool use
                        if (id.contains("flash") || id.contains("pro"))
                            && !capabilities.contains(&"tool_use".to_string())
                        {
                            capabilities.push("tool_use".into());
                        }
                    }
                    "embedContent" => {
                        capabilities.push("embeddings".into());
                    }
                    _ => {}
                }
            }

            all_models.push(AiModel {
                id: id.to_string(),
                provider: AiProvider::Google,
                display_name: m.display_name,
                description: m.description,
                context_length: m.input_token_limit,
                input_price: None, // Google API doesn't return pricing
                output_price: None,
                capabilities,
            });
        }

        match resp.next_page_token {
            Some(token) if !token.is_empty() => page_token = Some(token),
            _ => break,
        }
    }

    debug!(count = all_models.len(), "fetched Google Gemini models");
    Ok(all_models)
}

// ---------------------------------------------------------------------------
// OpenRouter
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct OpenRouterListResponse {
    data: Vec<OpenRouterModel>,
}

#[derive(serde::Deserialize)]
struct OpenRouterModel {
    /// e.g. "anthropic/claude-sonnet-4"
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    context_length: Option<i32>,
    #[serde(default)]
    pricing: Option<OpenRouterPricing>,
}

#[derive(serde::Deserialize)]
struct OpenRouterPricing {
    /// USD per token (string in the API)
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
}

async fn fetch_openrouter_models(
    http: &reqwest::Client,
    api_key: &str,
) -> Result<Vec<AiModel>, CatalogueError> {
    let resp: OpenRouterListResponse = http
        .get("https://openrouter.ai/api/v1/models")
        .bearer_auth(api_key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let models: Vec<AiModel> = resp
        .data
        .into_iter()
        .map(|m| {
            // OpenRouter pricing is per-token (string); convert to per-million-tokens
            let (input_price, output_price) = m.pricing.as_ref().map_or((None, None), |p| {
                let inp = p
                    .prompt
                    .as_deref()
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|per_token| per_token * 1_000_000.0);
                let out = p
                    .completion
                    .as_deref()
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|per_token| per_token * 1_000_000.0);
                (inp, out)
            });

            // Most OpenRouter chat models support completion + tool_use
            let capabilities = if m.id.contains("embed") {
                vec!["embeddings".into()]
            } else {
                vec!["completion".into(), "tool_use".into()]
            };

            AiModel {
                id: m.id,
                provider: AiProvider::OpenRouter,
                display_name: m.name,
                description: m.description,
                context_length: m.context_length,
                input_price,
                output_price,
                capabilities,
            }
        })
        .collect();

    debug!(count = models.len(), "fetched OpenRouter models");
    Ok(models)
}
