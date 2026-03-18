//! Enrichment pipeline: AI-generated metadata on contributions.
//!
//! Uses Rig extractors for structured data extraction. Each enrichment type
//! has a typed output struct, a prompt preamble, and a function that builds
//! the input text from a contribution's fields.

pub mod prompts;
pub mod types;

use ps_core::models::{AiProvider, TaskType};
use ps_core::repo::ReasoningRepo;
use ps_core::repo::reasoning::{UnenrichedContribution, UpsertEnrichmentParams};
use rig::client::CompletionClient;
use rig::completion::Usage;
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use crate::cost::CostTracker;
use crate::routing::TaskRouter;

use self::prompts::*;
use self::types::*;

/// Result of processing a single enrichment batch.
pub struct BatchResult {
    pub enrichment_type: EnrichmentType,
    pub processed: usize,
    pub errors: usize,
    pub total_usage: Usage,
    /// The first error message encountered, if any. Useful for surfacing
    /// systemic issues (e.g. model not found, auth failure) to the UI.
    pub first_error: Option<String>,
}

/// Max consecutive errors before aborting a batch (likely a systemic issue).
const MAX_CONSECUTIVE_ERRORS: usize = 3;

/// Build the input text for a contribution based on the enrichment type.
fn build_input_text(enrichment_type: EnrichmentType, c: &UnenrichedContribution) -> Option<String> {
    match enrichment_type {
        EnrichmentType::ReviewDepth | EnrichmentType::Sentiment => {
            // For reviews, the content is the review body text.
            let content = c.content.as_deref().unwrap_or("");
            if content.trim().is_empty() {
                return None;
            }
            Some(format!(
                "Review on: {}\n\n{}",
                c.title.as_deref().unwrap_or("(untitled PR)"),
                content,
            ))
        }
        EnrichmentType::Significance => {
            // For PRs, combine title + description + size metrics.
            let title = c.title.as_deref().unwrap_or("(untitled)");
            let description = c.content.as_deref().unwrap_or("(no description)");
            let additions = c
                .metrics
                .get("additions")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let deletions = c
                .metrics
                .get("deletions")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let changed_files = c
                .metrics
                .get("changed_files")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);

            Some(format!(
                "PR Title: {title}\n\
                 Description: {description}\n\
                 Lines added: {additions}, Lines removed: {deletions}, Files changed: {changed_files}"
            ))
        }
        EnrichmentType::Topic => {
            // For Discourse topics, combine title + opening post content.
            let title = c.title.as_deref().unwrap_or("(untitled)");
            let content = c.content.as_deref().unwrap_or("");
            if title.trim().is_empty() && content.trim().is_empty() {
                return None;
            }
            Some(format!("Topic: {title}\n\n{content}"))
        }
    }
}

/// Compute SHA-256 hash of input text.
fn hash_input(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Truncate text to approximately `max_chars` for the input preview.
fn input_preview(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        format!("{}…", &text[..max_chars])
    }
}

/// Process a batch of contributions for a single enrichment type.
///
/// Returns the number processed and any errors. Caller is responsible for
/// budget checks before calling this.
pub async fn process_enrichment_batch(
    router: &TaskRouter,
    repo: &ReasoningRepo,
    enrichment_type: EnrichmentType,
    contributions: &[UnenrichedContribution],
) -> BatchResult {
    let task_config = router.task_config(TaskType::Enrichment);
    let model_name = &task_config.model;
    let _provider_name = task_config.provider.as_str();

    let mut processed = 0usize;
    let mut errors = 0usize;
    let mut consecutive_errors = 0usize;
    let mut first_error: Option<String> = None;
    let mut total_usage = Usage::new();

    for contribution in contributions {
        // Fail fast: if we hit MAX_CONSECUTIVE_ERRORS in a row, it's a systemic
        // issue (wrong model, auth failure, etc.) — stop wasting API calls.
        if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
            warn!(
                enrichment = enrichment_type.as_str(),
                consecutive_errors,
                remaining = contributions.len() - processed - errors,
                "aborting batch after consecutive errors (likely systemic issue)"
            );
            errors += contributions.len() - processed - errors;
            break;
        }

        let Some(input_text) = build_input_text(enrichment_type, contribution) else {
            debug!(
                contribution_id = %contribution.id,
                enrichment = enrichment_type.as_str(),
                "skipping contribution with empty input"
            );
            continue;
        };

        let input_hash = hash_input(&input_text);
        let preview = input_preview(&input_text, 500);

        let result = extract_enrichment(router, enrichment_type, &input_text).await;

        match result {
            Ok((value, confidence, usage)) => {
                total_usage += usage;
                consecutive_errors = 0;

                if let Err(e) = repo
                    .upsert_enrichment(&UpsertEnrichmentParams {
                        contribution_id: contribution.id,
                        enrichment_type: enrichment_type.as_str(),
                        value: &value,
                        model_name,
                        confidence: Some(confidence),
                        input_hash: Some(&input_hash),
                        input_preview: Some(&preview),
                    })
                    .await
                {
                    warn!(
                        contribution_id = %contribution.id,
                        enrichment = enrichment_type.as_str(),
                        error = %e,
                        "failed to store enrichment"
                    );
                    errors += 1;
                    continue;
                }
                processed += 1;
            }
            Err(e) => {
                let err_msg = e.to_string();
                if first_error.is_none() {
                    first_error = Some(err_msg.clone());
                }
                warn!(
                    contribution_id = %contribution.id,
                    enrichment = enrichment_type.as_str(),
                    error = %err_msg,
                    "enrichment extraction failed"
                );
                errors += 1;
                consecutive_errors += 1;
            }
        }
    }

    info!(
        enrichment = enrichment_type.as_str(),
        processed,
        errors,
        input_tokens = total_usage.input_tokens,
        output_tokens = total_usage.output_tokens,
        "enrichment batch complete"
    );

    BatchResult {
        enrichment_type,
        processed,
        errors,
        total_usage,
        first_error,
    }
}

/// Extract a single enrichment using the appropriate Rig extractor.
///
/// Returns (value as JSON, confidence, token usage).
async fn extract_enrichment(
    router: &TaskRouter,
    enrichment_type: EnrichmentType,
    input_text: &str,
) -> Result<(serde_json::Value, f32, Usage), crate::routing::ProviderError> {
    let task_config = router.task_config(TaskType::Enrichment);

    match task_config.provider {
        AiProvider::Google => {
            let client = router.google_client()?;
            extract_with_client(client, &task_config.model, enrichment_type, input_text).await
        }
        AiProvider::OpenRouter => {
            let client = router.openrouter_client()?;
            extract_with_client(client, &task_config.model, enrichment_type, input_text).await
        }
    }
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
            let extractor = client
                .extractor::<ReviewDepthScore>(model)
                .preamble(REVIEW_DEPTH_PREAMBLE)
                .retries(1)
                .build();
            let resp = extractor
                .extract_with_usage(input_text)
                .await
                .map_err(|e| {
                    crate::routing::ProviderError::Completion(
                        rig::completion::CompletionError::ProviderError(e.to_string()),
                    )
                })?;
            let confidence = resp.data.confidence;
            let value = serde_json::to_value(&resp.data).unwrap_or_default();
            Ok((value, confidence, resp.usage))
        }
        EnrichmentType::Sentiment => {
            let extractor = client
                .extractor::<SentimentLabel>(model)
                .preamble(SENTIMENT_PREAMBLE)
                .retries(1)
                .build();
            let resp = extractor
                .extract_with_usage(input_text)
                .await
                .map_err(|e| {
                    crate::routing::ProviderError::Completion(
                        rig::completion::CompletionError::ProviderError(e.to_string()),
                    )
                })?;
            let confidence = resp.data.confidence;
            let value = serde_json::to_value(&resp.data).unwrap_or_default();
            Ok((value, confidence, resp.usage))
        }
        EnrichmentType::Significance => {
            let extractor = client
                .extractor::<SignificanceLabel>(model)
                .preamble(SIGNIFICANCE_PREAMBLE)
                .retries(1)
                .build();
            let resp = extractor
                .extract_with_usage(input_text)
                .await
                .map_err(|e| {
                    crate::routing::ProviderError::Completion(
                        rig::completion::CompletionError::ProviderError(e.to_string()),
                    )
                })?;
            let confidence = resp.data.confidence;
            let value = serde_json::to_value(&resp.data).unwrap_or_default();
            Ok((value, confidence, resp.usage))
        }
        EnrichmentType::Topic => {
            let extractor = client
                .extractor::<TopicClassification>(model)
                .preamble(TOPIC_PREAMBLE)
                .retries(1)
                .build();
            let resp = extractor
                .extract_with_usage(input_text)
                .await
                .map_err(|e| {
                    crate::routing::ProviderError::Completion(
                        rig::completion::CompletionError::ProviderError(e.to_string()),
                    )
                })?;
            let confidence = resp.data.confidence;
            let value = serde_json::to_value(&resp.data).unwrap_or_default();
            Ok((value, confidence, resp.usage))
        }
    }
}

/// Store the results of an enrichment batch: upsert enrichments and log API cost.
///
/// This is designed to be called inside `ctx.run()` by the Restate handler,
/// keeping DB writes journaled and idempotent on replay.
pub async fn log_enrichment_cost(
    cost_tracker: &CostTracker,
    provider_name: &str,
    model_name: &str,
    batch: &BatchResult,
) {
    // Log cost for the batch
    if batch.total_usage.input_tokens > 0 || batch.total_usage.output_tokens > 0 {
        cost_tracker
            .log_usage(
                provider_name,
                model_name,
                TaskType::Enrichment,
                &batch.total_usage,
            )
            .await;
    }
}
