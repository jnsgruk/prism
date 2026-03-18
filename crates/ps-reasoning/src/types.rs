use ps_core::models::{AiProvider, TaskType};
use serde::{Deserialize, Serialize};

/// A message in a completion request conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionMessage {
    pub role: Role,
    pub content: String,
}

/// Role in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A tool definition for function-calling models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's parameters.
    pub parameters: serde_json::Value,
}

/// A tool call returned by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// A request to a completion model.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<CompletionMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub tools: Vec<ToolDefinition>,
}

/// Token usage from a model response.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl TokenUsage {
    pub fn total(&self) -> u32 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// Why the model stopped generating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    ToolUse,
    MaxTokens,
    ContentFilter,
    Unknown,
}

/// A response from a completion model.
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
    pub model: String,
    pub finish_reason: FinishReason,
}

/// Configuration for a single AI task (which provider + model to use).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTaskConfig {
    pub provider: AiProvider,
    pub model: String,
}

/// All AI task routing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub tasks: AiTaskRouting,
    pub budget_cap_usd: Option<f64>,
}

/// Per-task provider/model routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTaskRouting {
    pub enrichment: AiTaskConfig,
    pub insights: AiTaskConfig,
    pub agentic: AiTaskConfig,
    pub embeddings: AiTaskConfig,
}

impl AiTaskRouting {
    /// Get the config for a specific task type.
    pub fn get(&self, task: TaskType) -> &AiTaskConfig {
        match task {
            TaskType::Enrichment => &self.enrichment,
            TaskType::Insights => &self.insights,
            TaskType::Agentic => &self.agentic,
            TaskType::Embeddings => &self.embeddings,
        }
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            tasks: AiTaskRouting::default(),
            budget_cap_usd: Some(5.0),
        }
    }
}

impl Default for AiTaskRouting {
    fn default() -> Self {
        Self {
            enrichment: AiTaskConfig {
                provider: AiProvider::Google,
                model: "gemini-3.1-flash-lite".into(),
            },
            insights: AiTaskConfig {
                provider: AiProvider::Google,
                model: "gemini-3.1-pro".into(),
            },
            agentic: AiTaskConfig {
                provider: AiProvider::Google,
                model: "gemini-3-flash".into(),
            },
            embeddings: AiTaskConfig {
                provider: AiProvider::Google,
                model: "gemini-embedding-2".into(),
            },
        }
    }
}
