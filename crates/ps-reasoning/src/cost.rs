use ps_core::models::TaskType;
use ps_core::repo::ReasoningRepo;
use rig::completion::Usage;
use tracing::{debug, warn};

/// Known per-token pricing (USD per 1M tokens).
///
/// Updated as models/pricing changes. This is a best-effort estimate —
/// actual costs come from provider billing, but this gives real-time visibility.
struct ModelPricing {
    input_per_million: f64,
    output_per_million: f64,
}

/// Estimate cost in USD from Rig's token usage and model name.
#[allow(clippy::cast_precision_loss)] // Token counts won't exceed f64 mantissa range in practice
pub fn estimate_cost(model: &str, usage: &Usage) -> f64 {
    let pricing = model_pricing(model);
    let input_cost = (usage.input_tokens as f64) * pricing.input_per_million / 1_000_000.0;
    let output_cost = (usage.output_tokens as f64) * pricing.output_per_million / 1_000_000.0;
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
        m if m.contains("flash") && !m.contains("lite") => ModelPricing {
            input_per_million: 0.15,
            output_per_million: 0.60,
        },
        m if m.contains("pro") => ModelPricing {
            input_per_million: 1.25,
            output_per_million: 10.0,
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
    ///
    /// Accepts Rig's `Usage` type directly from completion responses.
    pub async fn log_usage(&self, provider: &str, model: &str, task_type: TaskType, usage: &Usage) {
        let cost = estimate_cost(model, usage);

        debug!(
            provider = %provider,
            model = %model,
            task = %task_type,
            input_tokens = usage.input_tokens,
            output_tokens = usage.output_tokens,
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
                usage.input_tokens as i32,
                usage.output_tokens as i32,
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
