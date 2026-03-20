pub mod text;

use ps_core::repo::Repos;
use ps_core::repo::reasoning::QueuedEmbedding;
use rig::embeddings::EmbeddingError;
use tracing::{info, warn};

use self::text::build_embedding_text;

/// Number of dimensions to keep after MRL truncation.
pub const EMBEDDING_DIMS: usize = 1024;

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
}

impl BatchResult {
    pub fn empty() -> Self {
        Self {
            embedded: 0,
            skipped: 0,
            errors: 0,
            total_tokens: 0,
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
    let texts: Vec<(uuid::Uuid, String)> = items
        .iter()
        .filter_map(|item| {
            let text = build_embedding_text(item)?;
            Some((item.contribution_id, text))
        })
        .collect();

    let skipped = items.len() - texts.len();

    if texts.is_empty() {
        return Ok(BatchResult {
            embedded: 0,
            skipped,
            errors: 0,
            total_tokens: 0,
        });
    }

    let mut total_embedded = 0usize;
    let mut total_errors = 0usize;
    // Rough token estimate: ~4 chars per token
    let mut total_tokens = 0u64;

    for chunk in texts.chunks(SUB_BATCH_SIZE) {
        let text_strs: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
        let ids: Vec<uuid::Uuid> = chunk.iter().map(|(id, _)| *id).collect();

        // Estimate tokens for cost tracking
        let chunk_tokens: u64 = text_strs.iter().map(|t| t.len() as u64 / 4).sum();
        total_tokens += chunk_tokens;

        // Rig embedding call — returns Vec<Embedding> with f64 vectors
        let embeddings = match model.embed_texts(text_strs).await {
            Ok(embs) => embs,
            Err(e) => {
                warn!(error = %e, count = chunk.len(), "embedding API call failed");
                total_errors += chunk.len();
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
                warn!(error = %e, count = truncated.len(), "failed to store embeddings");
                total_errors += truncated.len();
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
    })
}
