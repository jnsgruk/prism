//! AI extraction logic for enrichment types.
//!
//! Dispatches to the correct Rig extractor based on provider and enrichment
//! type, using a macro to avoid per-variant boilerplate.

use ps_core::models::TaskType;
use rig::client::CompletionClient;
use rig::completion::Usage;

use crate::routing::TaskRouter;

use super::prompts::*;
use super::types::*;

/// Build, run, and unwrap a Rig extractor for a single output type.
///
/// Every enrichment output struct has a `confidence: f32` field and derives
/// `Serialize + JsonSchema`, so the body is identical across types — only the
/// type parameter and preamble differ.
macro_rules! extract_typed {
    ($client:expr, $model:expr, $preamble:expr, $input_text:expr, $output_type:ty) => {{
        let extractor = $client
            .extractor::<$output_type>($model)
            .preamble($preamble)
            .retries(1)
            .build();
        let resp = extractor
            .extract_with_usage($input_text)
            .await
            .map_err(|e| {
                crate::routing::ProviderError::Completion(
                    rig::completion::CompletionError::ProviderError(e.to_string()),
                )
            })?;
        let confidence = resp.data.confidence;
        let value = serde_json::to_value(&resp.data).unwrap_or_default();
        Ok((value, confidence, resp.usage))
    }};
}

/// Extract a single enrichment using the Gemini Rig extractor.
///
/// Returns (value as JSON, confidence, token usage).
pub async fn extract_enrichment(
    router: &TaskRouter,
    enrichment_type: EnrichmentType,
    input_text: &str,
) -> Result<(serde_json::Value, f32, Usage), crate::routing::ProviderError> {
    let task_config = router.task_config(TaskType::Enrichment);
    let client = router.google_client()?;
    extract_with_client(client, &task_config.model, enrichment_type, input_text).await
}

/// Generic extraction using any Rig completion client.
async fn extract_with_client<C>(
    client: &C,
    model: &str,
    enrichment_type: EnrichmentType,
    input_text: &str,
) -> Result<(serde_json::Value, f32, Usage), crate::routing::ProviderError>
where
    C: CompletionClient,
    C::CompletionModel: Send + Sync,
{
    match enrichment_type {
        EnrichmentType::ReviewDepth => {
            extract_typed!(
                client,
                model,
                REVIEW_DEPTH_PREAMBLE,
                input_text,
                ReviewDepthScore
            )
        }
        EnrichmentType::Sentiment => {
            extract_typed!(
                client,
                model,
                SENTIMENT_PREAMBLE,
                input_text,
                SentimentLabel
            )
        }
        EnrichmentType::Significance => {
            extract_typed!(
                client,
                model,
                SIGNIFICANCE_PREAMBLE,
                input_text,
                SignificanceLabel
            )
        }
        EnrichmentType::Topic => {
            extract_typed!(
                client,
                model,
                TOPIC_PREAMBLE,
                input_text,
                TopicClassification
            )
        }
    }
}
