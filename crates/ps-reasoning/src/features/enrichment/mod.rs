//! Enrichment pipeline: AI-generated metadata on contributions.
//!
//! Uses Rig extractors for structured data extraction. Each enrichment type
//! has a typed output struct, a prompt preamble, and a function that builds
//! the input text from a contribution's fields.

mod extract;
pub mod prompts;
pub mod types;

use std::fmt::Write as _;

use futures::stream::{self, StreamExt};
use ps_core::models::{AiProvider, TaskType};
use ps_core::repo::ReasoningRepo;
use ps_core::repo::reasoning::{EnrichmentResult, QueuedContribution, UpsertEnrichmentParams};
use rig::completion::Usage;
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::cost::CostTracker;
use crate::routing::TaskRouter;

use self::extract::extract_enrichment;
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

/// Max errors before flagging a systemic issue.
const MAX_CONSECUTIVE_ERRORS: usize = 3;

/// Max length for error messages stored in `BatchResult.first_error`.
///
/// Provider errors can contain sensitive data (partial API keys in auth
/// failures, internal URLs). Truncating limits exposure surface.
const MAX_ERROR_LENGTH: usize = 200;

/// Truncate an error message to a safe length for storage and logging.
fn sanitize_error(msg: &str) -> String {
    if msg.len() <= MAX_ERROR_LENGTH {
        msg.to_string()
    } else {
        format!(
            "{}… (truncated)",
            &msg[..msg.floor_char_boundary(MAX_ERROR_LENGTH)]
        )
    }
}

/// Build the input text for an enrichment from the queue's structured JSONB content.
///
/// The queue content has richer data than the old contribution fields (PR diffs,
/// review inline comments, topic body, Jira description).
fn build_input_from_queue(
    enrichment_type: EnrichmentType,
    q: &QueuedContribution,
) -> Option<String> {
    let c = &q.content;
    match enrichment_type {
        EnrichmentType::ReviewDepth | EnrichmentType::Sentiment => {
            let pr_title = c
                .get("pr_title")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled PR)");
            let body = c.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let mut text = format!(
                "Review on: {pr_title}\nState: {}\n\n",
                c.get("state").and_then(|v| v.as_str()).unwrap_or("")
            );

            if !body.is_empty() {
                text.push_str(body);
                text.push('\n');
            }

            if let Some(comments) = c.get("inline_comments").and_then(|v| v.as_array()) {
                for comment in comments {
                    let path = comment.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    let comment_body = comment.get("body").and_then(|v| v.as_str()).unwrap_or("");
                    if !comment_body.is_empty() {
                        let _ = write!(text, "\n[{path}]: {comment_body}");
                    }
                }
            }

            if text.trim().len() <= pr_title.len() + 20 {
                // Only has the header — no actual review content.
                return None;
            }
            Some(text)
        }
        EnrichmentType::Significance => {
            let title = c
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            let description = c
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("(no description)");
            let additions = c
                .get("additions")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let deletions = c
                .get("deletions")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let changed_files = c
                .get("changed_files")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let draft = c
                .get("draft")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);

            let mut text = format!(
                "PR Title: {title}\n\
                 Description: {description}\n\
                 Lines added: {additions}, Lines removed: {deletions}, Files changed: {changed_files}\n\
                 Draft: {draft}"
            );

            if let Some(diff) = c.get("diff").and_then(|v| v.as_str())
                && !diff.is_empty()
            {
                text.push_str("\n\n--- Diff ---\n");
                text.push_str(diff);
            }

            Some(text)
        }
        EnrichmentType::Topic => {
            let title = c
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            let body = c.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let category = c.get("category").and_then(|v| v.as_str()).unwrap_or("");

            if title.trim().is_empty() && body.trim().is_empty() {
                return None;
            }

            let mut text = format!("Topic: {title}");
            if !category.is_empty() {
                let _ = write!(text, "\nCategory: {category}");
            }
            if !body.is_empty() {
                let _ = write!(text, "\n\n{body}");
            }
            Some(text)
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
        // Find the last char boundary at or before max_chars to avoid
        // slicing through multi-byte characters.
        let end = text
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= max_chars)
            .last()
            .unwrap_or(0);
        format!("{}…", &text[..end])
    }
}

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
                        enrichment_type: type_str.to_string(),
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

    // Phase 4: Bulk upsert all successful enrichments in a single query.
    let processed = successes.len();
    if !successes.is_empty()
        && let Err(e) = repo.bulk_upsert_enrichments(&successes, model_name).await
    {
        warn!(
            enrichment = type_str,
            error = %e,
            count = processed,
            "failed to bulk upsert enrichments, falling back to individual upserts"
        );
        // Fallback: try individual upserts so we don't lose all results.
        let mut fallback_ok = 0usize;
        for r in &successes {
            if repo
                .upsert_enrichment(&UpsertEnrichmentParams {
                    contribution_id: r.contribution_id,
                    enrichment_type: &r.enrichment_type,
                    value: &r.value,
                    model_name,
                    confidence: Some(r.confidence),
                    input_hash: Some(&r.input_hash),
                    input_preview: Some(&r.input_preview),
                })
                .await
                .is_ok()
            {
                fallback_ok += 1;
            }
        }
        info!(
            enrichment = type_str,
            fallback_ok,
            total = processed,
            "individual upsert fallback complete"
        );
    }

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
    }
}

/// Log the API cost of an enrichment batch.
///
/// This is designed to be called inside `ctx.run()` by the Restate handler,
/// keeping DB writes journaled and idempotent on replay.
pub async fn log_enrichment_cost(
    cost_tracker: &CostTracker,
    provider: AiProvider,
    model_name: &str,
    batch: &BatchResult,
) {
    if batch.total_usage.input_tokens > 0 || batch.total_usage.output_tokens > 0 {
        cost_tracker
            .log_usage(
                provider,
                model_name,
                TaskType::Enrichment,
                &batch.total_usage,
            )
            .await;
    }
}

#[cfg(test)]
mod tests {
    use ps_core::repo::reasoning::QueuedContribution;
    use uuid::Uuid;

    use super::*;

    fn queued(content: serde_json::Value) -> QueuedContribution {
        QueuedContribution {
            id: Uuid::nil(),
            contribution_id: Uuid::nil(),
            contribution_type: "pr_review".into(),
            content,
        }
    }

    // -- build_input_from_queue --

    #[test]
    fn review_depth_with_body_and_comments() {
        let q = queued(serde_json::json!({
            "pr_title": "Add feature X",
            "state": "APPROVED",
            "body": "Looks good overall.",
            "inline_comments": [
                {"path": "src/main.rs", "body": "Consider using a match here."}
            ]
        }));
        let text = build_input_from_queue(EnrichmentType::ReviewDepth, &q).unwrap();
        assert!(text.contains("Review on: Add feature X"));
        assert!(text.contains("Looks good overall."));
        assert!(text.contains("[src/main.rs]: Consider using a match here."));
    }

    #[test]
    fn review_depth_no_content_has_header_only() {
        // Without a body or inline comments, the result only contains the
        // header (title + state). The function still returns Some because the
        // threshold check compares against pr_title length, but the content
        // is effectively just metadata.
        let q = queued(serde_json::json!({
            "pr_title": "Fix typo",
            "state": "APPROVED"
        }));
        let result = build_input_from_queue(EnrichmentType::ReviewDepth, &q);
        // Header-only result: "Review on: Fix typo\nState: APPROVED\n\n"
        // This exceeds title.len() + 20, so it returns Some
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("Review on: Fix typo"));
        assert!(!text.contains('[')); // no inline comments
    }

    #[test]
    fn significance_with_diff() {
        let q = queued(serde_json::json!({
            "title": "Refactor auth",
            "description": "Major rewrite of auth module",
            "additions": 200,
            "deletions": 150,
            "changed_files": 10,
            "draft": false,
            "diff": "+fn new_auth() {}\n-fn old_auth() {}"
        }));
        let text = build_input_from_queue(EnrichmentType::Significance, &q).unwrap();
        assert!(text.contains("PR Title: Refactor auth"));
        assert!(text.contains("Lines added: 200"));
        assert!(text.contains("--- Diff ---"));
        assert!(text.contains("+fn new_auth()"));
    }

    #[test]
    fn significance_without_diff() {
        let q = queued(serde_json::json!({
            "title": "Small fix",
            "description": "Typo fix",
            "additions": 1,
            "deletions": 1,
            "changed_files": 1,
            "draft": false
        }));
        let text = build_input_from_queue(EnrichmentType::Significance, &q).unwrap();
        assert!(text.contains("PR Title: Small fix"));
        assert!(!text.contains("--- Diff ---"));
    }

    #[test]
    fn topic_with_category() {
        let q = queued(serde_json::json!({
            "title": "How to install?",
            "category": "Support",
            "body": "I'm trying to install the package."
        }));
        let text = build_input_from_queue(EnrichmentType::Topic, &q).unwrap();
        assert!(text.contains("Topic: How to install?"));
        assert!(text.contains("Category: Support"));
        assert!(text.contains("I'm trying to install the package."));
    }

    #[test]
    fn topic_empty_returns_none() {
        let q = queued(serde_json::json!({
            "title": "",
            "body": ""
        }));
        assert!(build_input_from_queue(EnrichmentType::Topic, &q).is_none());
    }

    // -- hash_input --

    #[test]
    fn hash_input_deterministic() {
        let h1 = hash_input("hello world");
        let h2 = hash_input("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_input_differs_on_change() {
        let h1 = hash_input("hello");
        let h2 = hash_input("world");
        assert_ne!(h1, h2);
    }

    // -- input_preview --

    #[test]
    fn input_preview_short_text_unchanged() {
        assert_eq!(input_preview("short", 100), "short");
    }

    #[test]
    fn input_preview_truncates_long_text() {
        let text = "a".repeat(200);
        let preview = input_preview(&text, 50);
        assert!(preview.len() < 60);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn input_preview_respects_char_boundaries() {
        // Multi-byte character: 'é' is 2 bytes
        let text = "café".repeat(100);
        let preview = input_preview(&text, 10);
        assert!(preview.ends_with('…'));
        // Should not panic from slicing mid-character
    }

    // -- sanitize_error --

    #[test]
    fn sanitize_error_short_unchanged() {
        assert_eq!(sanitize_error("short error"), "short error");
    }

    #[test]
    fn sanitize_error_long_truncated() {
        let long = "x".repeat(300);
        let sanitized = sanitize_error(&long);
        assert!(sanitized.len() < 300);
        assert!(sanitized.contains("(truncated)"));
    }
}
