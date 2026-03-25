mod api_usage;
mod conversations;
mod embeddings;
mod enrichments;

pub use api_usage::{ApiUsageRecord, DailySpend, ModelSpend, TaskSpend};
pub use conversations::{
    Conversation, ConversationArtifact, ConversationEvent, ConversationMessage,
    ConversationSummary, CreateArtifactParams, CreateConversationParams, CreateMessageParams,
};
pub use embeddings::{
    EmbeddingQueueEntry, EmbeddingStatus, QueuedEmbedding, QueuedEnrichmentData,
    SimilarContribution,
};
pub use enrichments::{
    EnrichmentPipelineStatus, EnrichmentQueueEntry, EnrichmentRecord, EnrichmentResult,
    EnrichmentStatus, QueueContributionTypeCount, QueueStats, QueuedContribution,
    UnenrichedContribution, UpsertEnrichmentParams,
};

use sha2::{Digest, Sha256};
use sqlx::PgPool;

/// Repository for the `reasoning` schema: API usage tracking, cost management,
/// and AI enrichments.
#[derive(Clone)]
pub struct ReasoningRepo {
    pool: PgPool,
}

impl ReasoningRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Compute a SHA-256 content hash for change detection.
pub fn content_hash(content: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(content).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    format!("{digest:x}")
}
