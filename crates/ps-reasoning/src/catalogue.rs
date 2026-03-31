//! Fetch available models from AI provider APIs.
//!
//! Each provider has a `fetch_models` function that calls the provider's
//! model-listing endpoint and returns a normalized `Vec<AiModel>`.

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

/// Maximum number of pages to fetch from the Gemini model listing API.
const MAX_GOOGLE_PAGES: usize = 50;

async fn fetch_google_models(
    http: &reqwest::Client,
    api_key: &str,
) -> Result<Vec<AiModel>, CatalogueError> {
    let mut all_models = Vec::new();
    let mut page_token: Option<String> = None;

    for _ in 0..MAX_GOOGLE_PAGES {
        let mut request = http
            .get("https://generativelanguage.googleapis.com/v1beta/models")
            .query(&[("key", api_key), ("pageSize", "100")]);
        if let Some(ref token) = page_token {
            request = request.query(&[("pageToken", token.as_str())]);
        }

        let resp: GeminiListResponse = request.send().await?.error_for_status()?.json().await?;

        for m in resp.models {
            // Strip "models/" prefix to get the usable model ID
            let id = m.name.strip_prefix("models/").unwrap_or(&m.name);

            let has_generate = m
                .supported_generation_methods
                .iter()
                .any(|s| s == "generateContent");
            let has_embed = m
                .supported_generation_methods
                .iter()
                .any(|s| s == "embedContent");
            let has_generate_images = m
                .supported_generation_methods
                .iter()
                .any(|s| s == "generateImages");

            let mut capabilities = Vec::new();
            if has_generate {
                capabilities.push("completion".into());
                // Flash and Pro models support tool use
                if id.contains("flash") || id.contains("pro") {
                    capabilities.push("tool_use".into());
                }
            }
            if has_embed {
                capabilities.push("embeddings".into());
            }
            if has_generate_images {
                capabilities.push("image_generation".into());
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
    #[serde(default)]
    architecture: Option<OpenRouterArchitecture>,
}

#[derive(serde::Deserialize)]
struct OpenRouterArchitecture {
    #[serde(default)]
    modality: Option<String>,
    #[serde(default)]
    output_modalities: Vec<String>,
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

            let capabilities = if m.id.contains("embed") {
                vec!["embeddings".into()]
            } else {
                let (has_text_output, has_image_output) = match &m.architecture {
                    Some(arch) => {
                        // Check output_modalities array first, then fall back to
                        // the "input->output" modality string (output is after "->").
                        let output_str =
                            arch.modality.as_deref().and_then(|m| m.split("->").nth(1));
                        let img = arch.output_modalities.iter().any(|o| o == "image")
                            || output_str.is_some_and(|o| o.contains("image"));
                        let txt = arch.output_modalities.iter().any(|o| o == "text")
                            || output_str
                                .map_or(arch.output_modalities.is_empty(), |o| o.contains("text"));
                        (txt, img)
                    }
                    None => (true, false),
                };

                let mut caps = Vec::new();
                if has_text_output {
                    caps.push("completion".into());
                    caps.push("tool_use".into());
                }
                if has_image_output {
                    caps.push("image_generation".into());
                }
                caps
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to parse an OpenRouter model JSON and extract capabilities.
    fn openrouter_caps(json: serde_json::Value) -> Vec<String> {
        let resp: OpenRouterListResponse =
            serde_json::from_value(serde_json::json!({"data": [json]})).unwrap();
        let m = &resp.data[0];

        if m.id.contains("embed") {
            return vec!["embeddings".into()];
        }

        let (has_text_output, has_image_output) = match &m.architecture {
            Some(arch) => {
                let output_str = arch.modality.as_deref().and_then(|m| m.split("->").nth(1));
                let img = arch.output_modalities.iter().any(|o| o == "image")
                    || output_str.is_some_and(|o| o.contains("image"));
                let txt = arch.output_modalities.iter().any(|o| o == "text")
                    || output_str.map_or(arch.output_modalities.is_empty(), |o| o.contains("text"));
                (txt, img)
            }
            None => (true, false),
        };

        let mut caps = Vec::new();
        if has_text_output {
            caps.push("completion".into());
            caps.push("tool_use".into());
        }
        if has_image_output {
            caps.push("image_generation".into());
        }
        caps
    }

    #[test]
    fn openrouter_image_only_model() {
        let caps = openrouter_caps(serde_json::json!({
            "id": "stabilityai/sdxl", "name": "SDXL",
            "architecture": {"modality": "text->image", "output_modalities": ["image"]}
        }));
        assert!(!caps.contains(&"completion".to_string()));
        assert!(caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn openrouter_multimodal_model() {
        let caps = openrouter_caps(serde_json::json!({
            "id": "openai/gpt-4o", "name": "GPT-4o",
            "architecture": {"output_modalities": ["text", "image"]}
        }));
        assert!(caps.contains(&"completion".to_string()));
        assert!(caps.contains(&"tool_use".to_string()));
        assert!(caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn openrouter_text_only_model() {
        let caps = openrouter_caps(serde_json::json!({
            "id": "anthropic/claude-4", "name": "Claude 4",
            "architecture": {"modality": "text->text", "output_modalities": ["text"]}
        }));
        assert!(caps.contains(&"completion".to_string()));
        assert!(!caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn openrouter_missing_architecture_defaults_to_text() {
        let caps = openrouter_caps(serde_json::json!({
            "id": "mistralai/mistral-large", "name": "Mistral Large"
        }));
        assert!(caps.contains(&"completion".to_string()));
        assert!(!caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn openrouter_modality_text_image_output() {
        // Model with "text+image->text+image" modality format
        let caps = openrouter_caps(serde_json::json!({
            "id": "openai/gpt-image", "name": "GPT Image",
            "architecture": {"modality": "text+image->text+image", "output_modalities": []}
        }));
        assert!(caps.contains(&"completion".to_string()));
        assert!(caps.contains(&"image_generation".to_string()));
    }

    /// Helper to compute capabilities for a Google model.
    fn google_caps(methods: &[&str]) -> Vec<String> {
        let has_generate = methods.iter().any(|s| *s == "generateContent");
        let has_embed = methods.iter().any(|s| *s == "embedContent");
        let has_images = methods.iter().any(|s| *s == "generateImages");

        let mut caps = Vec::new();
        if has_generate {
            caps.push("completion".into());
        }
        if has_embed {
            caps.push("embeddings".into());
        }
        if has_images {
            caps.push("image_generation".into());
        }
        caps
    }

    #[test]
    fn google_generate_images_capability() {
        let caps = google_caps(&["generateContent", "generateImages"]);
        assert!(caps.contains(&"completion".to_string()));
        assert!(caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn google_image_only_model() {
        let caps = google_caps(&["generateImages"]);
        assert!(!caps.contains(&"completion".to_string()));
        assert!(caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn google_text_only_model() {
        let caps = google_caps(&["generateContent"]);
        assert!(caps.contains(&"completion".to_string()));
        assert!(!caps.contains(&"image_generation".to_string()));
    }
}
