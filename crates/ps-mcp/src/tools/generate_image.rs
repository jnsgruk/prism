//! Image generation via provider APIs (`OpenRouter` / Google Gemini).
//!
//! Called by the `generate_image` MCP tool.  Decodes the base64 response,
//! uploads to S3 via `ArtifactStore`, and returns the standard artifact JSON.

use base64::Engine as _;
use bytes::Bytes;
use serde::Deserialize;
use tracing::{debug, info};

use crate::artifact_store::ArtifactStore;

/// Resolved provider for image generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProvider {
    OpenRouter,
    Google,
}

/// Successful image generation result.
pub struct GeneratedImage {
    pub data: Bytes,
    pub content_type: String,
}

// ---------------------------------------------------------------------------
// Provider resolution
// ---------------------------------------------------------------------------

/// Resolve provider and model from explicit args + env fallbacks.
///
/// Priority: explicit model arg → `DEFAULT_IMAGE_MODEL` env var.
/// Provider is inferred from model prefix (`google/` → Google, else `OpenRouter`).
pub fn resolve_model_and_provider(
    model: Option<&str>,
    provider: Option<&str>,
) -> Result<(String, ImageProvider), String> {
    let model_id = model
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| std::env::var("DEFAULT_IMAGE_MODEL").ok())
        .ok_or_else(|| {
            "No image model specified and DEFAULT_IMAGE_MODEL is not configured. \
             Ask an admin to set a default image model in AI settings."
                .to_string()
        })?;

    let prov = if let Some(p) = provider {
        match p {
            "google" => ImageProvider::Google,
            "openrouter" => ImageProvider::OpenRouter,
            _ => return Err(format!("Unknown provider: {p}")),
        }
    } else if model_id.starts_with("google/") {
        ImageProvider::Google
    } else {
        ImageProvider::OpenRouter
    };

    Ok((model_id, prov))
}

// ---------------------------------------------------------------------------
// OpenRouter
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OpenRouterResponse {
    choices: Vec<OpenRouterChoice>,
}

#[derive(Deserialize)]
struct OpenRouterChoice {
    message: OpenRouterMessage,
}

#[derive(Deserialize)]
struct OpenRouterMessage {
    content: serde_json::Value,
}

pub async fn generate_openrouter(
    http: &reqwest::Client,
    model: &str,
    prompt: &str,
) -> Result<GeneratedImage, String> {
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .map_err(|_| "OPENROUTER_API_KEY not set".to_string())?;

    debug!(model, "calling OpenRouter image generation");

    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
    });

    let resp = http
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("OpenRouter request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("OpenRouter returned {status}: {text}"));
    }

    let parsed: OpenRouterResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse OpenRouter response: {e}"))?;

    let content = parsed
        .choices
        .first()
        .map(|c| &c.message.content)
        .ok_or("Empty response from OpenRouter")?;

    extract_image_from_openrouter(content)
}

/// Extract base64 image data from `OpenRouter` response content.
///
/// `OpenRouter` image models return content in various formats:
/// 1. `[{"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}]`
/// 2. Plain base64 string in the content field
/// 3. `{"type": "image", "data": "base64..."}` objects
fn extract_image_from_openrouter(content: &serde_json::Value) -> Result<GeneratedImage, String> {
    // Case 1: Array with image_url items
    if let Some(arr) = content.as_array() {
        for item in arr {
            if let Some(url) = item
                .get("image_url")
                .and_then(|u| u.get("url"))
                .and_then(|u| u.as_str())
            {
                return decode_data_uri(url);
            }
            // Case 3: image data objects
            if let Some(data) = item.get("data").and_then(|d| d.as_str()) {
                let ct = item
                    .get("mimeType")
                    .or_else(|| item.get("mime_type"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("image/png");
                return decode_base64_image(data, ct);
            }
        }
    }

    // Case 2: Plain string (could be data URI or raw base64)
    if let Some(s) = content.as_str() {
        if s.starts_with("data:image/") {
            return decode_data_uri(s);
        }
        if s.len() > 100 {
            // Likely raw base64
            return decode_base64_image(s, "image/png");
        }
    }

    Err("Could not extract image from OpenRouter response".to_string())
}

// ---------------------------------------------------------------------------
// Google Gemini
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
}

#[derive(Deserialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPart {
    inline_data: Option<GeminiInlineData>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiInlineData {
    mime_type: String,
    data: String,
}

pub async fn generate_google(
    http: &reqwest::Client,
    model: &str,
    prompt: &str,
) -> Result<GeneratedImage, String> {
    let api_key =
        std::env::var("GOOGLE_API_KEY").map_err(|_| "GOOGLE_API_KEY not set".to_string())?;

    // Strip "google/" prefix if present
    let model_id = model.strip_prefix("google/").unwrap_or(model);

    debug!(model = model_id, "calling Google Gemini image generation");

    let body = serde_json::json!({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {"responseModalities": ["IMAGE"]},
    });

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model_id}:generateContent?key={api_key}"
    );

    let resp = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Google API request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Google API returned {status}: {text}"));
    }

    let parsed: GeminiResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Google response: {e}"))?;

    let inline = parsed
        .candidates
        .first()
        .and_then(|c| c.content.parts.iter().find_map(|p| p.inline_data.as_ref()))
        .ok_or("No image data in Google response")?;

    decode_base64_image(&inline.data, &inline.mime_type)
}

// ---------------------------------------------------------------------------
// Image decoding helpers
// ---------------------------------------------------------------------------

fn decode_data_uri(uri: &str) -> Result<GeneratedImage, String> {
    // Format: data:image/png;base64,iVBOR...
    let after_data = uri
        .strip_prefix("data:")
        .ok_or("Invalid data URI: missing 'data:' prefix")?;
    let (meta, b64) = after_data
        .split_once(',')
        .ok_or("Invalid data URI: missing comma")?;
    let content_type = meta.split(';').next().unwrap_or("image/png");
    decode_base64_image(b64, content_type)
}

fn decode_base64_image(b64: &str, content_type: &str) -> Result<GeneratedImage, String> {
    let engine = base64::engine::general_purpose::STANDARD;
    let data = engine
        .decode(b64.trim())
        .map_err(|e| format!("Base64 decode failed: {e}"))?;
    Ok(GeneratedImage {
        data: Bytes::from(data),
        content_type: content_type.to_string(),
    })
}

/// File extension for a content type.
fn extension_for(content_type: &str) -> &str {
    match content_type {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "png",
    }
}

// ---------------------------------------------------------------------------
// Top-level generate + upload
// ---------------------------------------------------------------------------

/// Generate an image and upload it to S3.  Returns the JSON result string
/// in the same format as `upload_artifact`.
pub async fn generate_and_upload(
    http: &reqwest::Client,
    artifacts: &ArtifactStore,
    prompt: &str,
    model: Option<&str>,
    provider: Option<&str>,
    aspect_ratio: Option<&str>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("prompt must not be empty".to_string());
    }

    let (model_id, resolved_provider) = resolve_model_and_provider(model, provider)?;
    let _aspect = aspect_ratio.unwrap_or("1:1");

    info!(
        model = %model_id,
        provider = ?resolved_provider,
        "generating image"
    );

    let image = match resolved_provider {
        ImageProvider::OpenRouter => generate_openrouter(http, &model_id, prompt).await?,
        ImageProvider::Google => generate_google(http, &model_id, prompt).await?,
    };

    let ext = extension_for(&image.content_type);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let filename = format!("generated-image-{timestamp}.{ext}");
    let size = image.data.len();

    let key = artifacts
        .upload(&filename, Some(&image.content_type), image.data)
        .await
        .map_err(|e| format!("S3 upload failed: {e}"))?;

    // Truncate prompt to a reasonable display name
    let display_name: String = prompt.chars().take(80).collect();

    let result = serde_json::json!({
        "status": "generated",
        "artifact_key": key,
        "display_name": display_name,
        "content_type": image.content_type,
        "size_bytes": size,
    });

    info!(key = %key, size_bytes = size, "image generated and uploaded");

    serde_json::to_string_pretty(&result).map_err(|e| format!("serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_google_provider_from_model_prefix() {
        let (model, prov) = resolve_model_and_provider(Some("google/imagen-3"), None).unwrap();
        assert_eq!(model, "google/imagen-3");
        assert_eq!(prov, ImageProvider::Google);
    }

    #[test]
    fn resolve_openrouter_provider_from_model_prefix() {
        let (model, prov) = resolve_model_and_provider(Some("stabilityai/sdxl"), None).unwrap();
        assert_eq!(model, "stabilityai/sdxl");
        assert_eq!(prov, ImageProvider::OpenRouter);
    }

    #[test]
    fn resolve_explicit_provider_overrides_prefix() {
        let (_, prov) =
            resolve_model_and_provider(Some("google/imagen-3"), Some("openrouter")).unwrap();
        assert_eq!(prov, ImageProvider::OpenRouter);
    }

    #[test]
    fn resolve_missing_model_and_no_env_errors() {
        // Ensure DEFAULT_IMAGE_MODEL is not set for this test
        unsafe { std::env::remove_var("DEFAULT_IMAGE_MODEL") };
        let result = resolve_model_and_provider(None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No image model specified"));
    }

    #[test]
    fn decode_data_uri_png() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"fake-png-data");
        let uri = format!("data:image/png;base64,{b64}");
        let img = decode_data_uri(&uri).unwrap();
        assert_eq!(img.content_type, "image/png");
        assert_eq!(img.data.as_ref(), b"fake-png-data");
    }

    #[test]
    fn decode_data_uri_jpeg() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"fake-jpg");
        let uri = format!("data:image/jpeg;base64,{b64}");
        let img = decode_data_uri(&uri).unwrap();
        assert_eq!(img.content_type, "image/jpeg");
    }

    #[test]
    fn decode_base64_image_valid() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"test-image");
        let img = decode_base64_image(&b64, "image/webp").unwrap();
        assert_eq!(img.content_type, "image/webp");
        assert_eq!(img.data.as_ref(), b"test-image");
    }

    #[test]
    fn decode_base64_invalid() {
        let result = decode_base64_image("not-valid-base64!!!", "image/png");
        assert!(result.is_err());
    }

    #[test]
    fn extension_mapping() {
        assert_eq!(extension_for("image/png"), "png");
        assert_eq!(extension_for("image/jpeg"), "jpg");
        assert_eq!(extension_for("image/webp"), "webp");
        assert_eq!(extension_for("image/gif"), "gif");
        assert_eq!(extension_for("application/octet-stream"), "png");
    }

    #[test]
    fn extract_openrouter_image_url_format() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"img-data");
        let content = serde_json::json!([
            {
                "type": "image_url",
                "image_url": {"url": format!("data:image/png;base64,{b64}")}
            }
        ]);
        let img = extract_image_from_openrouter(&content).unwrap();
        assert_eq!(img.content_type, "image/png");
        assert_eq!(img.data.as_ref(), b"img-data");
    }

    #[test]
    fn extract_openrouter_plain_base64() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(vec![0u8; 200]);
        let content = serde_json::json!(b64);
        let img = extract_image_from_openrouter(&content).unwrap();
        assert_eq!(img.content_type, "image/png");
    }

    #[test]
    fn extract_openrouter_data_object() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"img");
        let content = serde_json::json!([{"type": "image", "data": b64, "mimeType": "image/webp"}]);
        let img = extract_image_from_openrouter(&content).unwrap();
        assert_eq!(img.content_type, "image/webp");
    }

    #[test]
    fn extract_openrouter_empty_content_errors() {
        let content = serde_json::json!("short");
        assert!(extract_image_from_openrouter(&content).is_err());
    }

    #[test]
    fn parse_gemini_response() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"gemini-img");
        let json = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{"inlineData": {"mimeType": "image/png", "data": b64}}]
                }
            }]
        });
        let resp: GeminiResponse = serde_json::from_value(json).unwrap();
        let inline = resp.candidates[0].content.parts[0]
            .inline_data
            .as_ref()
            .unwrap();
        let img = decode_base64_image(&inline.data, &inline.mime_type).unwrap();
        assert_eq!(img.data.as_ref(), b"gemini-img");
    }
}
