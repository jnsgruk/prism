use crate::provider::{ModelProvider, ProviderError};
use crate::types::{
    CompletionMessage, CompletionRequest, CompletionResponse, FinishReason, Role, TokenUsage,
    ToolCall,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// `OpenRouter` provider — OpenAI-compatible API gateway to multiple models.
pub struct OpenRouterProvider {
    api_key: String,
    client: reqwest::Client,
}

impl OpenRouterProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Make a minimal API call to validate the API key.
    pub async fn test_connection(&self) -> Result<(), ProviderError> {
        let url = format!("{OPENROUTER_BASE_URL}/models");
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;
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

// ---------------------------------------------------------------------------
// OpenAI-compatible request/response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ChatTool>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatTool {
    r#type: String,
    function: ChatFunction,
}

#[derive(Serialize)]
struct ChatFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Option<Vec<ChatChoice>>,
    usage: Option<ChatUsage>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: Option<ChatResponseMessage>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ChatToolCall>>,
}

#[derive(Deserialize)]
struct ChatToolCall {
    id: Option<String>,
    function: Option<ChatToolCallFunction>,
}

#[derive(Deserialize)]
struct ChatToolCallFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct ChatUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct ChatErrorResponse {
    error: Option<ChatError>,
}

#[derive(Deserialize)]
struct ChatError {
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

fn to_openai_role(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn parse_finish_reason(s: &str) -> FinishReason {
    match s {
        "stop" => FinishReason::Stop,
        "tool_calls" => FinishReason::ToolUse,
        "length" => FinishReason::MaxTokens,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Unknown,
    }
}

fn messages_to_openai(messages: &[CompletionMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .map(|m| ChatMessage {
            role: to_openai_role(m.role).into(),
            content: m.content.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// ModelProvider impl
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl ModelProvider for OpenRouterProvider {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let tools: Vec<ChatTool> = req
            .tools
            .iter()
            .map(|t| ChatTool {
                r#type: "function".into(),
                function: ChatFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect();

        let chat_req = ChatRequest {
            model: req.model.clone(),
            messages: messages_to_openai(&req.messages),
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            tools,
        };

        let url = format!("{OPENROUTER_BASE_URL}/chat/completions");

        debug!(model = %req.model, "sending OpenRouter completion request");

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://prism.canonical.com")
            .header("X-Title", "Prism")
            .json(&chat_req)
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
            // Try to extract a readable error message
            let message = serde_json::from_str::<ChatErrorResponse>(&body)
                .ok()
                .and_then(|e| e.error)
                .and_then(|e| e.message)
                .unwrap_or(body);
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message,
            });
        }

        let chat_resp: ChatResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        let usage = chat_resp
            .usage
            .map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens.unwrap_or(0),
                completion_tokens: u.completion_tokens.unwrap_or(0),
            })
            .unwrap_or_default();

        let choice = chat_resp.choices.and_then(|mut c| {
            if c.is_empty() {
                None
            } else {
                Some(c.remove(0))
            }
        });

        let finish_reason = choice
            .as_ref()
            .and_then(|c| c.finish_reason.as_deref())
            .map_or(FinishReason::Unknown, parse_finish_reason);

        let message = choice.and_then(|c| c.message);

        let content = message.as_ref().and_then(|m| m.content.clone());

        let tool_calls = message
            .and_then(|m| m.tool_calls)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tc| {
                let id = tc
                    .id
                    .unwrap_or_else(|| format!("call_{}", uuid::Uuid::now_v7()));
                let func = tc.function?;
                let name = func.name?;
                let arguments = func
                    .arguments
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                Some(ToolCall {
                    id,
                    name,
                    arguments,
                })
            })
            .collect();

        let model = chat_resp.model.unwrap_or(req.model);

        Ok(CompletionResponse {
            content,
            tool_calls,
            usage,
            model,
            finish_reason,
        })
    }

    async fn embed(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // OpenRouter supports the OpenAI embeddings endpoint
        let url = format!("{OPENROUTER_BASE_URL}/embeddings");

        debug!(model = %model, count = texts.len(), "sending OpenRouter embed request");

        let body = serde_json::json!({
            "model": model,
            "input": texts,
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let embed_resp: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        let data = embed_resp
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| ProviderError::Parse("missing data array".into()))?;

        let mut embeddings = Vec::with_capacity(data.len());
        for item in data {
            let values = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| ProviderError::Parse("missing embedding array".into()))?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            embeddings.push(values);
        }

        if embeddings.len() != texts.len() {
            warn!(
                expected = texts.len(),
                got = embeddings.len(),
                "embedding count mismatch"
            );
        }

        Ok(embeddings)
    }

    fn name(&self) -> &'static str {
        "openrouter"
    }
}
