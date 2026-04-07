use ps_core::models::{AiProvider, TaskType};
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

/// Estimate cost in USD from Rig's token usage, provider, and model name.
#[allow(clippy::cast_precision_loss)] // Token counts won't exceed f64 mantissa range in practice
pub fn estimate_cost(provider: AiProvider, model: &str, usage: &Usage) -> f64 {
    let pricing = model_pricing(provider, model);
    let input_cost = (usage.input_tokens as f64) * pricing.input_per_million / 1_000_000.0;
    let output_cost = (usage.output_tokens as f64) * pricing.output_per_million / 1_000_000.0;
    input_cost + output_cost
}

fn model_pricing(provider: AiProvider, model: &str) -> ModelPricing {
    match provider {
        AiProvider::Google => google_pricing(model),
        AiProvider::OpenRouter => openrouter_pricing(model),
    }
}

fn google_pricing(model: &str) -> ModelPricing {
    match model {
        m if m.contains("flash-lite") => ModelPricing {
            input_per_million: 0.075,
            output_per_million: 0.30,
        },
        m if m.contains("flash") => ModelPricing {
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
        _ => ModelPricing {
            input_per_million: 0.50,
            output_per_million: 2.0,
        },
    }
}

fn openrouter_pricing(model: &str) -> ModelPricing {
    match model {
        m if m.contains("claude") => ModelPricing {
            input_per_million: 3.0,
            output_per_million: 15.0,
        },
        m if m.contains("gpt-4") => ModelPricing {
            input_per_million: 2.50,
            output_per_million: 10.0,
        },
        _ => ModelPricing {
            input_per_million: 1.0,
            output_per_million: 5.0,
        },
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
    pub async fn log_usage(
        &self,
        provider: AiProvider,
        model: &str,
        task_type: TaskType,
        usage: &Usage,
    ) {
        let cost = estimate_cost(provider, model, usage);

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
                provider.as_str(),
                model,
                task_type.as_str(),
                usage.input_tokens as i32,
                usage.output_tokens as i32,
                cost as f32,
            )
            .await
        {
            warn!(
                error = %e,
                provider = %provider,
                model = %model,
                task = %task_type,
                metric = "api_usage_log_failure",
                "failed to log API usage — cost tracking may be incomplete"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            cached_input_tokens: 0,
        }
    }

    #[test]
    fn google_flash_lite_pricing() {
        let cost = estimate_cost(
            AiProvider::Google,
            "gemini-2.5-flash-lite",
            &usage(1_000_000, 1_000_000),
        );
        assert!((cost - 0.375).abs() < 0.001);
    }

    #[test]
    fn google_flash_pricing() {
        let cost = estimate_cost(
            AiProvider::Google,
            "gemini-2.0-flash",
            &usage(1_000_000, 1_000_000),
        );
        assert!((cost - 0.75).abs() < 0.001);
    }

    #[test]
    fn google_pro_pricing() {
        let cost = estimate_cost(
            AiProvider::Google,
            "gemini-2-pro",
            &usage(1_000_000, 1_000_000),
        );
        assert!((cost - 11.25).abs() < 0.001);
    }

    #[test]
    fn google_embedding_pricing() {
        let cost = estimate_cost(
            AiProvider::Google,
            "text-embedding-004",
            &usage(1_000_000, 0),
        );
        assert!((cost - 0.20).abs() < 0.001);
    }

    #[test]
    fn openrouter_claude_pricing() {
        let cost = estimate_cost(
            AiProvider::OpenRouter,
            "anthropic/claude-3-sonnet",
            &usage(1_000_000, 1_000_000),
        );
        assert!((cost - 18.0).abs() < 0.001);
    }

    #[test]
    fn openrouter_gpt4_pricing() {
        let cost = estimate_cost(
            AiProvider::OpenRouter,
            "openai/gpt-4-turbo",
            &usage(1_000_000, 1_000_000),
        );
        assert!((cost - 12.5).abs() < 0.001);
    }

    #[test]
    fn zero_tokens_zero_cost() {
        let cost = estimate_cost(AiProvider::Google, "gemini-2.0-flash", &usage(0, 0));
        assert!((cost).abs() < f64::EPSILON);
    }

    #[test]
    fn unknown_model_fallback() {
        // Google unknown model uses default (0.50 / 2.0)
        let cost = estimate_cost(
            AiProvider::Google,
            "unknown-xyz",
            &usage(1_000_000, 1_000_000),
        );
        assert!((cost - 2.5).abs() < 0.001);
    }
}
