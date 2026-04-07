use ps_core::models::{AiProvider, TaskType};
use serde::{Deserialize, Serialize};

/// Configuration for a single AI task (which provider + model to use).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTaskConfig {
    pub provider: AiProvider,
    pub model: String,
}

/// All AI task routing configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiConfig {
    pub tasks: AiTaskRouting,
    /// Default model for image generation via the `generate_image` MCP tool.
    #[serde(default)]
    pub image_generation: Option<AiTaskConfig>,
}

/// Per-task provider/model routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTaskRouting {
    pub enrichment: AiTaskConfig,
    pub agentic: AiTaskConfig,
    pub embeddings: AiTaskConfig,
}

impl AiTaskRouting {
    /// Get the config for a specific task type.
    pub fn get(&self, task: TaskType) -> &AiTaskConfig {
        match task {
            TaskType::Enrichment => &self.enrichment,
            TaskType::Agentic | TaskType::ImageGeneration => &self.agentic,
            TaskType::Embeddings => &self.embeddings,
        }
    }
}

impl Default for AiTaskRouting {
    fn default() -> Self {
        Self {
            enrichment: AiTaskConfig {
                provider: AiProvider::Google,
                model: "gemini-2.5-flash".into(),
            },
            agentic: AiTaskConfig {
                provider: AiProvider::Google,
                model: "gemini-2.5-flash".into(),
            },
            embeddings: AiTaskConfig {
                provider: AiProvider::Google,
                model: "text-embedding-004".into(),
            },
        }
    }
}
