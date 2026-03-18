use crate::provider::{ModelProvider, ProviderError};
use crate::types::{
    CompletionMessage, CompletionRequest, CompletionResponse, FinishReason, Role, TokenUsage,
    ToolCall,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Google Gemini API provider.
///
/// Uses the REST API directly via reqwest. Supports both completion
/// (generateContent) and embedding (batchEmbedContents) endpoints.
pub struct GoogleProvider {
    api_key: String,
    client: reqwest::Client,
}

impl GoogleProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Make a minimal API call to validate the API key.
    pub async fn test_connection(&self) -> Result<(), ProviderError> {
        let url = format!("{GEMINI_BASE_URL}/models?key={}", urlencoded(&self.api_key));
        let resp = self.client.get(&url).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(ProviderError::Api {
                status,
                message: body,
            })
        }
    }
}

fn urlencoded(s: &str) -> String {
    s.replace('%', "%25")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
}

// ---------------------------------------------------------------------------
// Gemini API request/response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<GeminiTool>,
}

#[derive(Serialize, Deserialize)]
struct GeminiContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_call: Option<GeminiFunctionCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_response: Option<GeminiFunctionResponse>,
}

#[derive(Serialize, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct GeminiFunctionResponse {
    name: String,
    response: serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

#[derive(Serialize)]
struct GeminiTool {
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiCandidate {
    content: Option<GeminiContent>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    prompt_token_count: Option<u32>,
    candidates_token_count: Option<u32>,
}

// ---------------------------------------------------------------------------
// Embedding types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiEmbedRequest {
    requests: Vec<GeminiEmbedContentRequest>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiEmbedContentRequest {
    model: String,
    content: GeminiContent,
}

#[derive(Deserialize)]
struct GeminiEmbedResponse {
    embeddings: Option<Vec<GeminiEmbedding>>,
}

#[derive(Deserialize)]
struct GeminiEmbedding {
    values: Vec<f32>,
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

fn to_gemini_role(role: Role) -> &'static str {
    match role {
        Role::Assistant => "model",
        Role::Tool => "function",
        // System is handled separately via system_instruction; if it reaches
        // here, treat as user.
        Role::User | Role::System => "user",
    }
}

fn messages_to_gemini(
    messages: &[CompletionMessage],
) -> (Option<GeminiContent>, Vec<GeminiContent>) {
    let mut system_instruction = None;
    let mut contents = Vec::new();

    for msg in messages {
        if msg.role == Role::System {
            system_instruction = Some(GeminiContent {
                role: None,
                parts: vec![GeminiPart {
                    text: Some(msg.content.clone()),
                    function_call: None,
                    function_response: None,
                }],
            });
        } else {
            contents.push(GeminiContent {
                role: Some(to_gemini_role(msg.role).into()),
                parts: vec![GeminiPart {
                    text: Some(msg.content.clone()),
                    function_call: None,
                    function_response: None,
                }],
            });
        }
    }

    (system_instruction, contents)
}

fn parse_finish_reason(s: &str) -> FinishReason {
    match s {
        "STOP" => FinishReason::Stop,
        "MAX_TOKENS" => FinishReason::MaxTokens,
        "SAFETY" => FinishReason::ContentFilter,
        _ => FinishReason::Unknown,
    }
}

// ---------------------------------------------------------------------------
// ModelProvider impl
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl ModelProvider for GoogleProvider {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let (system_instruction, contents) = messages_to_gemini(&req.messages);

        let tools: Vec<GeminiTool> = if req.tools.is_empty() {
            vec![]
        } else {
            vec![GeminiTool {
                function_declarations: req
                    .tools
                    .iter()
                    .map(|t| GeminiFunctionDeclaration {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.parameters.clone(),
                    })
                    .collect(),
            }]
        };

        let gemini_req = GeminiRequest {
            contents,
            system_instruction,
            generation_config: Some(GeminiGenerationConfig {
                temperature: req.temperature,
                max_output_tokens: req.max_tokens,
            }),
            tools,
        };

        let url = format!(
            "{GEMINI_BASE_URL}/models/{}:generateContent?key={}",
            req.model,
            urlencoded(&self.api_key)
        );

        debug!(model = %req.model, "sending Gemini completion request");

        let resp = self.client.post(&url).json(&gemini_req).send().await?;

        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            return Err(ProviderError::RateLimited {
                retry_after_secs: retry_after,
            });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let gemini_resp: GeminiResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        let usage = gemini_resp
            .usage_metadata
            .map(|u| TokenUsage {
                prompt_tokens: u.prompt_token_count.unwrap_or(0),
                completion_tokens: u.candidates_token_count.unwrap_or(0),
            })
            .unwrap_or_default();

        let candidate = gemini_resp.candidates.and_then(|mut c| {
            if c.is_empty() {
                None
            } else {
                Some(c.remove(0))
            }
        });

        let finish_reason = candidate
            .as_ref()
            .and_then(|c| c.finish_reason.as_deref())
            .map_or(FinishReason::Unknown, parse_finish_reason);

        let parts = candidate
            .and_then(|c| c.content)
            .map(|c| c.parts)
            .unwrap_or_default();

        let mut content = None;
        let mut tool_calls = Vec::new();

        for part in parts {
            if let Some(text) = part.text {
                content = Some(text);
            }
            if let Some(fc) = part.function_call {
                tool_calls.push(ToolCall {
                    id: format!("call_{}", uuid::Uuid::now_v7()),
                    name: fc.name,
                    arguments: fc.args,
                });
            }
        }

        let finish_reason = if tool_calls.is_empty() {
            finish_reason
        } else {
            FinishReason::ToolUse
        };

        Ok(CompletionResponse {
            content,
            tool_calls,
            usage,
            model: req.model,
            finish_reason,
        })
    }

    async fn embed(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let model_path = format!("models/{model}");
        let requests: Vec<GeminiEmbedContentRequest> = texts
            .iter()
            .map(|text| GeminiEmbedContentRequest {
                model: model_path.clone(),
                content: GeminiContent {
                    role: None,
                    parts: vec![GeminiPart {
                        text: Some(text.clone()),
                        function_call: None,
                        function_response: None,
                    }],
                },
            })
            .collect();

        let url = format!(
            "{GEMINI_BASE_URL}/models/{model}:batchEmbedContents?key={}",
            urlencoded(&self.api_key)
        );

        debug!(model = %model, count = texts.len(), "sending Gemini embed request");

        let resp = self
            .client
            .post(&url)
            .json(&GeminiEmbedRequest { requests })
            .send()
            .await?;

        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            return Err(ProviderError::RateLimited {
                retry_after_secs: retry_after,
            });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let embed_resp: GeminiEmbedResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        let embeddings = embed_resp.embeddings.unwrap_or_default();
        if embeddings.len() != texts.len() {
            warn!(
                expected = texts.len(),
                got = embeddings.len(),
                "embedding count mismatch"
            );
        }

        Ok(embeddings.into_iter().map(|e| e.values).collect())
    }

    fn name(&self) -> &'static str {
        "google"
    }
}
