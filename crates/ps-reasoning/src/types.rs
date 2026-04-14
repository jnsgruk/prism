use ps_core::models::{AiProvider, TaskType};
use serde::{Deserialize, Serialize};

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
    /// Default model for image generation via the `generate_image` MCP tool.
    #[serde(default = "default_image_generation")]
    pub image_generation: Option<AiTaskConfig>,
}

fn default_image_generation() -> Option<AiTaskConfig> {
    Some(AiTaskConfig {
        provider: AiProvider::Google,
        model: "gemini-3-pro-image-preview".into(),
    })
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            tasks: AiTaskRouting::default(),
            image_generation: default_image_generation(),
        }
    }
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
                model: "gemini-3-flash-preview".into(),
            },
            agentic: AiTaskConfig {
                provider: AiProvider::Google,
                model: "gemini-3.1-pro-preview".into(),
            },
            embeddings: AiTaskConfig {
                provider: AiProvider::Google,
                model: "gemini-embedding-2-preview".into(),
            },
        }
    }
}
