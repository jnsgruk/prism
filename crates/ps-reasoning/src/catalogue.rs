//! Fetch available models from the Google Gemini API.
//!
//! Returns a normalized `Vec<AiModel>` for storage in the model catalogue.

use ps_core::models::{AiModel, AiProvider};
use tracing::debug;

/// Errors that can occur when fetching the model catalogue.
#[derive(Debug, thiserror::Error)]
pub enum CatalogueError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("unexpected response: {0}")]
    Parse(String),
}

/// Fetch the model catalogue for the Google provider.
pub async fn fetch_models(
    http: &reqwest::Client,
    _provider: AiProvider,
    api_key: &str,
) -> Result<Vec<AiModel>, CatalogueError> {
    fetch_google_models(http, api_key).await
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

/// Returns `true` for Google model IDs that should be excluded from the
/// catalogue. The listing API has no lifecycle field, so we approximate by
/// keeping only stable (versioned) and preview models.
fn is_stale_google_model(id: &str) -> bool {
    // "-latest" aliases hot-swap on new releases — unusable for pinned config.
    if id.ends_with("-latest") {
        return true;
    }
    // "-exp" / "-exp-" experimental models are short-lived.
    if id.contains("-exp-") || id.ends_with("-exp") {
        return true;
    }
    // Bare legacy aliases (no version number).
    if id == "gemini-pro" || id == "gemini-pro-vision" {
        return true;
    }
    // Deprecated generations.
    if id.starts_with("gemini-1.0-")
        || id.starts_with("gemini-1.5-")
        || id.starts_with("gemini-2.0-")
    {
        return true;
    }
    false
}

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

            if is_stale_google_model(id) {
                continue;
            }

            let has_generate = m
                .supported_generation_methods
                .iter()
                .any(|s| s == "generateContent");
            let has_embed = m
                .supported_generation_methods
                .iter()
                .any(|s| s == "embedContent");
            // Imagen models use "predict" as their generation method
            let has_predict = m
                .supported_generation_methods
                .iter()
                .any(|s| s == "predict");
            // Imagen models: use predict + have "imagen" or "generate" in name
            let is_imagen = has_predict && (id.contains("imagen") || id.contains("-generate-"));
            // Gemini native image gen models have "-image" in their ID
            // (e.g. "gemini-2.0-flash-exp-image-generation",
            // "gemini-3.1-flash-image-001")
            let is_gemini_image_gen = has_generate && id.split('-').any(|seg| seg == "image");

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
            if is_imagen || is_gemini_image_gen {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to compute capabilities for a Google model given its ID and
    /// supported generation methods (mirrors the logic in `fetch_google_models`).
    fn google_caps(id: &str, methods: &[&str]) -> Vec<String> {
        let has_generate = methods.iter().any(|s| *s == "generateContent");
        let has_embed = methods.iter().any(|s| *s == "embedContent");
        let has_predict = methods.iter().any(|s| *s == "predict");
        let is_imagen = has_predict && (id.contains("imagen") || id.contains("-generate-"));
        let is_gemini_image_gen = has_generate && id.split('-').any(|seg| seg == "image");

        let mut caps = Vec::new();
        if has_generate {
            caps.push("completion".into());
            if id.contains("flash") || id.contains("pro") {
                caps.push("tool_use".into());
            }
        }
        if has_embed {
            caps.push("embeddings".into());
        }
        if is_imagen || is_gemini_image_gen {
            caps.push("image_generation".into());
        }
        caps
    }

    #[test]
    fn google_imagen_model() {
        // Imagen 3 reports "predict" as its generation method
        let caps = google_caps("imagen-3.0-generate-002", &["predict"]);
        assert!(!caps.contains(&"completion".to_string()));
        assert!(caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn google_gemini_native_image_gen() {
        // Gemini native image generation uses generateContent but has
        // "image-generation" in the model ID
        let caps = google_caps(
            "gemini-2.0-flash-exp-image-generation",
            &["generateContent", "countTokens"],
        );
        assert!(caps.contains(&"completion".to_string()));
        assert!(caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn google_gemini_flash_image_model() {
        // Nano Banana 2 / Gemini 3.1 Flash Image
        let caps = google_caps(
            "gemini-3.1-flash-image-preview",
            &["generateContent", "countTokens"],
        );
        assert!(caps.contains(&"completion".to_string()));
        assert!(caps.contains(&"tool_use".to_string()));
        assert!(caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn google_gemini_pro_image_model() {
        // Nano Banana Pro / Gemini 3 Pro Image
        let caps = google_caps(
            "gemini-3-pro-image-preview",
            &["generateContent", "countTokens"],
        );
        assert!(caps.contains(&"completion".to_string()));
        assert!(caps.contains(&"tool_use".to_string()));
        assert!(caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn google_text_only_model() {
        let caps = google_caps("gemini-2.5-flash", &["generateContent"]);
        assert!(caps.contains(&"completion".to_string()));
        assert!(!caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn google_predict_non_imagen_model() {
        // A model with "predict" but not an imagen/generate model ID
        let caps = google_caps("some-other-model", &["predict"]);
        assert!(!caps.contains(&"image_generation".to_string()));
    }

    #[test]
    fn stale_google_latest_alias() {
        assert!(is_stale_google_model("gemini-pro-latest"));
        assert!(is_stale_google_model("gemini-2.5-flash-latest"));
    }

    #[test]
    fn stale_google_experimental() {
        assert!(is_stale_google_model("gemini-2.0-flash-exp"));
        assert!(is_stale_google_model(
            "gemini-2.0-flash-exp-image-generation"
        ));
    }

    #[test]
    fn stale_google_legacy_aliases() {
        assert!(is_stale_google_model("gemini-pro"));
        assert!(is_stale_google_model("gemini-pro-vision"));
    }

    #[test]
    fn stale_google_old_generations() {
        assert!(is_stale_google_model("gemini-1.0-pro-001"));
        assert!(is_stale_google_model("gemini-1.5-flash-001"));
        assert!(is_stale_google_model("gemini-1.5-pro-latest"));
    }

    #[test]
    fn stable_google_models_kept() {
        assert!(!is_stale_google_model("gemini-2.5-flash"));
        assert!(!is_stale_google_model("gemini-2.5-pro"));
        assert!(!is_stale_google_model("gemini-3.1-flash-image-preview"));
        assert!(!is_stale_google_model("gemini-3-pro-image-preview"));
        assert!(!is_stale_google_model("imagen-3.0-generate-002"));
    }
}
