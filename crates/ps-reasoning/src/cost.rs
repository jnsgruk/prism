use ps_core::models::TaskType;
use ps_core::repo::ReasoningRepo;
use tracing::{debug, warn};

use crate::types::TokenUsage;

/// Known per-token pricing (USD per 1M tokens).
///
/// Updated as models/pricing changes. This is a best-effort estimate —
/// actual costs come from provider billing, but this gives real-time visibility.
struct ModelPricing {
    input_per_million: f64,
    output_per_million: f64,
}

/// Estimate cost in USD from token usage and model name.
pub fn estimate_cost(model: &str, usage: &TokenUsage) -> f64 {
    let pricing = model_pricing(model);
    let input_cost = f64::from(usage.prompt_tokens) * pricing.input_per_million / 1_000_000.0;
    let output_cost = f64::from(usage.completion_tokens) * pricing.output_per_million / 1_000_000.0;
    input_cost + output_cost
}

fn model_pricing(model: &str) -> ModelPricing {
    // Match on known model names/prefixes
    match model {
        // Google Gemini models
        m if m.contains("flash-lite") => ModelPricing {
            input_per_million: 0.075,
            output_per_million: 0.30,
        },
        m if m.contains("gemini-3-flash") || m.contains("gemini-3.0-flash") => ModelPricing {
            input_per_million: 0.50,
            output_per_million: 3.0,
        },
        m if m.contains("gemini-3.1-pro") || m.contains("gemini-3-pro") => ModelPricing {
            input_per_million: 2.0,
            output_per_million: 12.0,
        },
        m if m.contains("embedding") => ModelPricing {
            input_per_million: 0.20,
            output_per_million: 0.0,
        },
        // OpenRouter / other models — conservative defaults
        m if m.contains("claude") => ModelPricing {
            input_per_million: 3.0,
            output_per_million: 15.0,
        },
        m if m.contains("gpt-4") => ModelPricing {
            input_per_million: 2.50,
            output_per_million: 10.0,
        },
        _ => {
            // Unknown model — use a conservative estimate
            ModelPricing {
                input_per_million: 1.0,
                output_per_million: 5.0,
            }
        }
    }
}

/// Tracks API costs by logging usage to the `reasoning.api_usage` table.
pub struct CostTracker {
    repo: ReasoningRepo,
}

impl CostTracker {
    pub fn new(repo: ReasoningRepo) -> Self {
        Self { repo }
    }

    /// Log a completed API call's usage and estimated cost.
    pub async fn log_usage(
        &self,
        provider: &str,
        model: &str,
        task_type: TaskType,
        usage: &TokenUsage,
    ) {
        let cost = estimate_cost(model, usage);

        debug!(
            provider = %provider,
            model = %model,
            task = %task_type,
            prompt_tokens = usage.prompt_tokens,
            completion_tokens = usage.completion_tokens,
            cost_usd = cost,
            "logging API usage"
        );

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        if let Err(e) = self
            .repo
            .log_api_usage(
                provider,
                model,
                task_type.as_str(),
                usage.prompt_tokens as i32,
                usage.completion_tokens as i32,
                cost as f32,
            )
            .await
        {
            warn!(error = %e, "failed to log API usage");
        }
    }

    /// Check if the daily budget has been exceeded.
    pub async fn check_budget(&self, cap_usd: f64) -> Result<bool, ps_core::Error> {
        let today = time::OffsetDateTime::now_utc().date();
        let spent = self.repo.get_daily_spend(today).await?;
        Ok(spent < cap_usd)
    }

    /// Get the current daily spend.
    pub async fn daily_spend(&self) -> Result<f64, ps_core::Error> {
        let today = time::OffsetDateTime::now_utc().date();
        self.repo.get_daily_spend(today).await
    }
}
