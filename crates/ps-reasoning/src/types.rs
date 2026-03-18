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
