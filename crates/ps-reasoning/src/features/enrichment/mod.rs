//! Enrichment pipeline: AI-generated metadata on contributions.
//!
//! Uses Rig extractors for structured data extraction. Each enrichment type
//! has a typed output struct, a prompt preamble, and a function that builds
//! the input text from a contribution's fields.

mod extract;
mod input;
pub mod prompts;
pub mod types;

use futures::stream::{self, StreamExt};
use ps_core::models::TaskType;
use ps_core::repo::ReasoningRepo;
use ps_core::repo::reasoning::{EnrichmentResult, QueuedContribution};
use rig::completion::Usage;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::routing::TaskRouter;

use self::extract::extract_enrichment;
use self::input::{build_input_from_queue, hash_input, input_preview, sanitize_error};
use self::types::EnrichmentType;

/// Result of processing a single enrichment batch.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BatchResult {
    pub enrichment_type: EnrichmentType,
    pub processed: usize,
    pub errors: usize,
    pub total_usage: Usage,
    /// The first error message encountered, if any. Useful for surfacing
    /// systemic issues (e.g. model not found, auth failure) to the UI.
    pub first_error: Option<String>,
    /// Contribution IDs that produced a successful enrichment in this batch.
    /// Used by the handler to count distinct contributions processed across
    /// types, rather than summing per-type row counts (a `pr_review` produces
    /// two enrichment rows — `review_depth` + `sentiment` — but is one unit
    /// of work from the user's perspective).
    pub successful_contribution_ids: Vec<uuid::Uuid>,
}

/// Max errors before flagging a systemic issue.
const MAX_CONSECUTIVE_ERRORS: usize = 3;

/// Max concurrent AI API calls per batch.
const ENRICHMENT_CONCURRENCY: usize = 10;

/// A prepared item ready for concurrent AI extraction.
struct PreparedItem {
    contribution_id: Uuid,
    input_text: String,
    input_hash: String,
    input_preview: String,
}

/// Outcome of a single AI extraction call.
enum ItemOutcome {
    Success {
        result: EnrichmentResult,
        usage: Usage,
    },
    Error(String),
}

/// Process a batch of queued contributions for a single enrichment type.
///
/// Items are processed concurrently (up to `ENRICHMENT_CONCURRENCY`) for
/// throughput, then successful results are bulk-upserted in a single query.
pub async fn process_queued_enrichment_batch(
    router: &TaskRouter,
    repo: &ReasoningRepo,
    enrichment_type: EnrichmentType,
    contributions: &[QueuedContribution],
) -> BatchResult {
    let task_config = router.task_config(TaskType::Enrichment);
    let model_name = &task_config.model;

    if contributions.is_empty() {
        return BatchResult {
            enrichment_type,
            processed: 0,
            errors: 0,
            total_usage: Usage::new(),
            first_error: None,
            successful_contribution_ids: Vec::new(),
        };
    }

    // Phase 1: Build all inputs synchronously (fast, no async needed).
    let prepared: Vec<PreparedItem> = contributions
        .iter()
        .filter_map(|contribution| {
            let input_text = build_input_from_queue(enrichment_type, contribution)?;
            let input_hash = hash_input(&input_text);
            let input_preview = input_preview(&input_text, 500);
            Some(PreparedItem {
                contribution_id: contribution.contribution_id,
                input_text,
                input_hash,
                input_preview,
            })
        })
        .collect();

    let skipped = contributions.len() - prepared.len();
    if skipped > 0 {
        debug!(
            enrichment = enrichment_type.as_str(),
            skipped, "skipped contributions with empty input"
        );
    }

    // Phase 2: Run all AI extractions concurrently.
    let type_str = enrichment_type.as_str();
    let outcomes: Vec<ItemOutcome> = stream::iter(prepared)
        .map(|item| async move {
            match extract_enrichment(router, enrichment_type, &item.input_text).await {
                Ok((value, confidence, usage)) => ItemOutcome::Success {
                    result: EnrichmentResult {
                        contribution_id: item.contribution_id,
                        enrichment_type,
                        value,
                        confidence,
                        input_hash: item.input_hash,
                        input_preview: item.input_preview,
                    },
                    usage,
                },
                Err(e) => {
                    let err_msg = sanitize_error(&e.to_string());
                    warn!(
                        contribution_id = %item.contribution_id,
                        enrichment = type_str,
                        error = %err_msg,
                        "enrichment extraction failed"
                    );
                    ItemOutcome::Error(err_msg)
                }
            }
        })
        .buffer_unordered(ENRICHMENT_CONCURRENCY)
        .collect()
        .await;

    // Phase 3: Aggregate results.
    let mut successes = Vec::new();
    let mut errors = 0usize;
    let mut first_error: Option<String> = None;
    let mut total_usage = Usage::new();

    for outcome in outcomes {
        match outcome {
            ItemOutcome::Success { result, usage } => {
                total_usage += usage;
                successes.push(result);
            }
            ItemOutcome::Error(msg) => {
                if first_error.is_none() {
                    first_error = Some(msg);
                }
                errors += 1;
            }
        }
    }

    // Detect systemic failures: if all items failed, flag it.
    if errors >= MAX_CONSECUTIVE_ERRORS && successes.is_empty() {
        warn!(
            enrichment = type_str,
            errors, "all items failed — likely systemic issue (wrong model, auth failure, etc.)"
        );
    }

    // Phase 4: Accept only results for the still-current queue generation.
    let successful_contribution_ids =
        persist_results(repo, &successes, model_name, contributions).await;
    let processed = successful_contribution_ids.len();

    info!(
        enrichment = type_str,
        processed,
        errors,
        input_tokens = total_usage.input_tokens,
        output_tokens = total_usage.output_tokens,
        "queued enrichment batch complete"
    );

    BatchResult {
        enrichment_type,
        processed,
        errors,
        total_usage,
        first_error,
        successful_contribution_ids,
    }
}

async fn persist_results(
    repo: &ReasoningRepo,
    successes: &[EnrichmentResult],
    model_name: &str,
    captured: &[QueuedContribution],
) -> Vec<Uuid> {
    match repo
        .bulk_upsert_queued_enrichments(successes, model_name, captured)
        .await
    {
        Ok(ids) => ids,
        Err(error) => {
            warn!(%error, count = successes.len(), "bulk enrichment write failed; retrying guarded individual writes");
            let mut written = Vec::new();
            for result in successes {
                match repo
                    .bulk_upsert_queued_enrichments(
                        std::slice::from_ref(result),
                        model_name,
                        captured,
                    )
                    .await
                {
                    Ok(ids) => written.extend(ids),
                    Err(error) => {
                        warn!(%error, contribution_id = %result.contribution_id, "guarded enrichment write failed");
                    }
                }
            }
            written
        }
    }
}
