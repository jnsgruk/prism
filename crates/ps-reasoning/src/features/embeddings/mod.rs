pub mod text;

use ps_core::repo::Repos;
use ps_core::repo::reasoning::QueuedEmbedding;
use rig::embeddings::EmbeddingError;
use tracing::{info, warn};

use self::text::build_embedding_text;

/// Number of dimensions for stored embeddings.
///
/// Gemini Embedding 2 natively produces 768 dimensions.
/// If a model returns more, we truncate via MRL; if fewer, we use as-is.
pub const EMBEDDING_DIMS: usize = 768;

/// Truncate a Rig embedding (f64) to `EMBEDDING_DIMS` dimensions and convert to f32.
pub fn truncate_embedding(embedding: &rig::embeddings::Embedding) -> Vec<f32> {
    embedding
        .vec
        .iter()
        .take(EMBEDDING_DIMS)
        .map(|&v| v as f32)
        .collect()
}

/// Maximum texts per embedding API call.
const SUB_BATCH_SIZE: usize = 100;

/// Result of processing a batch of embedding queue items.
pub struct BatchResult {
    pub embedded: usize,
    pub skipped: usize,
    pub errors: usize,
    pub total_tokens: u64,
    pub failures: Vec<BatchFailure>,
}

pub struct BatchFailure {
    pub queue_ids: Vec<uuid::Uuid>,
    pub message: String,
    pub permanent: bool,
    pub retry_after_secs: u64,
}

impl BatchResult {
    pub fn empty() -> Self {
        Self {
            embedded: 0,
            skipped: 0,
            errors: 0,
            total_tokens: 0,
            failures: Vec::new(),
        }
    }
}

/// Process a batch of queued contributions: build text, embed via Rig, store vectors.
///
/// Uses the deprecated `EmbeddingModelDyn` trait for dynamic dispatch, since
/// `EmbeddingModel` is not object-safe (has associated types and consts).
#[allow(deprecated)]
pub async fn process_embedding_batch(
    items: &[QueuedEmbedding],
    model: &dyn rig::embeddings::EmbeddingModelDyn,
    repos: &Repos,
    model_name: &str,
) -> Result<BatchResult, EmbeddingError> {
    // Build texts, filtering out items with no embeddable content
    let texts: Vec<(uuid::Uuid, uuid::Uuid, String)> = items
        .iter()
        .filter_map(|item| {
            let text = build_embedding_text(item)?;
            Some((item.id, item.contribution_id, text))
        })
        .collect();

    let skipped = items.len() - texts.len();
    let skipped_ids: Vec<uuid::Uuid> = items
        .iter()
        .filter(|item| !texts.iter().any(|(id, _, _)| *id == item.id))
        .map(|item| item.id)
        .collect();
    let mut failures = if skipped_ids.is_empty() {
        Vec::new()
    } else {
        vec![BatchFailure {
            queue_ids: skipped_ids,
            message: "contribution has no embeddable text".into(),
            permanent: true,
            retry_after_secs: 0,
        }]
    };

    if texts.is_empty() {
        return Ok(BatchResult {
            embedded: 0,
            skipped,
            errors: 0,
            total_tokens: 0,
            failures,
        });
    }

    let mut total_embedded = 0usize;
    let mut total_errors = 0usize;
    // Rough token estimate: ~4 chars per token
    let mut total_tokens = 0u64;

    for chunk in texts.chunks(SUB_BATCH_SIZE) {
        let text_strs: Vec<String> = chunk.iter().map(|(_, _, text)| text.clone()).collect();
        let queue_ids: Vec<uuid::Uuid> = chunk.iter().map(|(id, _, _)| *id).collect();
        let ids: Vec<uuid::Uuid> = chunk.iter().map(|(_, id, _)| *id).collect();

        // Estimate tokens for cost tracking
        let chunk_tokens: u64 = text_strs.iter().map(|t| t.len() as u64 / 4).sum();
        total_tokens += chunk_tokens;

        // Rig embedding call — returns Vec<Embedding> with f64 vectors
        let embeddings = match model.embed_texts(text_strs).await {
            Ok(embs) => embs,
            Err(e) => {
                let message = truncate_error(&e.to_string());
                let (permanent, retry_after_secs) = classify_error(&message);
                warn!(error = %message, count = chunk.len(), permanent, retry_after_secs, "embedding API call failed");
                total_errors += chunk.len();
                failures.push(BatchFailure {
                    queue_ids,
                    message,
                    permanent,
                    retry_after_secs,
                });
                continue;
            }
        };

        // Truncate to EMBEDDING_DIMS and convert f64 → f32
        let truncated: Vec<Vec<f32>> = embeddings.iter().map(truncate_embedding).collect();

        match repos
            .reasoning
            .bulk_upsert_embeddings(&ids, &truncated, model_name)
            .await
        {
            Ok(count) => {
                total_embedded += count as usize;
            }
            Err(e) => {
                let message = truncate_error(&e.to_string());
                warn!(error = %message, count = truncated.len(), "failed to store embeddings");
                total_errors += truncated.len();
                failures.push(BatchFailure {
                    queue_ids,
                    message,
                    permanent: false,
                    retry_after_secs: 30,
                });
            }
        }
    }

    info!(
        embedded = total_embedded,
        skipped,
        errors = total_errors,
        "embedding batch complete"
    );

    Ok(BatchResult {
        embedded: total_embedded,
        skipped,
        errors: total_errors,
        total_tokens,
        failures,
    })
}

fn classify_error(message: &str) -> (bool, u64) {
    let message = message.to_ascii_lowercase();
    if message.contains("429")
        || message.contains("resource_exhausted")
        || message.contains("too many requests")
    {
        (false, 60)
    } else if message.contains("400") || message.contains("invalid_argument") {
        (true, 0)
    } else {
        (false, 30)
    }
}

fn truncate_error(message: &str) -> String {
    message.chars().take(1_000).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_rate_limits_for_delayed_retry() {
        assert_eq!(
            classify_error("429 Too Many Requests RESOURCE_EXHAUSTED"),
            (false, 60)
        );
    }

    #[test]
    fn classifies_invalid_requests_as_permanent() {
        assert_eq!(classify_error("400 INVALID_ARGUMENT"), (true, 0));
    }

    #[test]
    fn bounds_persisted_provider_errors() {
        assert_eq!(truncate_error(&"x".repeat(2_000)).len(), 1_000);
    }
}
