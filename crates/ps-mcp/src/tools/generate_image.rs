//! Image generation via Google Gemini API.
//!
//! Called by the `generate_image` MCP tool. Decodes the base64 response and
//! saves the image to `/workspace`.

use base64::Engine as _;
use bytes::Bytes;
use serde::Deserialize;
use tracing::{debug, info};

/// Successful image generation result.
pub struct GeneratedImage {
    pub data: Bytes,
    pub content_type: String,
}

// ---------------------------------------------------------------------------
// Model resolution
// ---------------------------------------------------------------------------

/// Resolve model from explicit args + env fallbacks.
///
/// Priority: explicit model arg → `DEFAULT_IMAGE_MODEL` env var.
pub fn resolve_model(model: Option<&str>) -> Result<String, String> {
    model
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| std::env::var("DEFAULT_IMAGE_MODEL").ok())
        .ok_or_else(|| {
            "No image model specified and DEFAULT_IMAGE_MODEL is not configured. \
             Ask an admin to set a default image model in AI settings."
                .to_string()
        })
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

#[cfg(test)]
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

/// Generate an image and save it to `/workspace`.
pub async fn generate_and_save(
    http: &reqwest::Client,
    prompt: &str,
    model: Option<&str>,
    _provider: Option<&str>,
    aspect_ratio: Option<&str>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("prompt must not be empty".to_string());
    }

    let model_id = resolve_model(model)?;
    let _aspect = aspect_ratio.unwrap_or("1:1");

    info!(model = %model_id, "generating image");

    let image = generate_google(http, &model_id, prompt).await?;

    let ext = extension_for(&image.content_type);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let filename = format!("generated-image-{timestamp}.{ext}");
    let size = image.data.len();

    let file_path = format!("/workspace/{filename}");
    tokio::fs::write(&file_path, &image.data)
        .await
        .map_err(|e| format!("failed to write image to workspace: {e}"))?;

    let display_name: String = prompt.chars().take(80).collect();

    let result = serde_json::json!({
        "status": "generated",
        "file_path": file_path,
        "display_name": display_name,
        "content_type": image.content_type,
        "size_bytes": size,
    });

    info!(file_path = %file_path, size_bytes = size, "image generated and saved to workspace");

    serde_json::to_string_pretty(&result).map_err(|e| format!("serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_model_from_arg() {
        let model = resolve_model(Some("google/imagen-3")).unwrap();
        assert_eq!(model, "google/imagen-3");
    }

    #[test]
    fn resolve_missing_model_and_no_env_errors() {
        // Ensure DEFAULT_IMAGE_MODEL is not set for this test
        unsafe { std::env::remove_var("DEFAULT_IMAGE_MODEL") };
        let result = resolve_model(None);
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
