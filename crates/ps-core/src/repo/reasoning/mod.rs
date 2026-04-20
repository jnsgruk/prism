mod api_usage;
mod conversations;
mod embeddings;
mod enrichments;

pub use api_usage::{ApiUsageRecord, ModelUsage, TaskUsage};
pub use conversations::{
    Conversation, ConversationEvent, ConversationMessage, ConversationSummary,
    CreateConversationParams, CreateMessageParams,
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

use crate::Error;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

/// Repository for the `reasoning` schema: API usage tracking and AI enrichments.
#[derive(Clone)]
pub struct ReasoningRepo {
    pool: PgPool,
}

impl ReasoningRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Delete all reasoning data in reverse-FK order.
    ///
    /// Called at the start of a full overwrite restore to clear the slate.
    pub async fn delete_all_for_restore(&self) -> Result<(), Error> {
        let mut tx: sqlx::Transaction<'_, sqlx::Postgres> =
            self.pool.begin().await.map_err(Error::from)?;

        // Child tables first
        sqlx::query!("DELETE FROM reasoning.insight_snapshot_sources")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!("DELETE FROM reasoning.insight_snapshots")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!("DELETE FROM reasoning.conversation_messages")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!("DELETE FROM reasoning.conversation_events")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!("DELETE FROM reasoning.conversations")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!("DELETE FROM reasoning.embedding_queue")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!("DELETE FROM reasoning.embeddings")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!("DELETE FROM reasoning.enrichment_queue")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!("DELETE FROM reasoning.enrichments")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        tx.commit().await.map_err(Error::from)
    }
}

/// Compute a SHA-256 content hash for change detection.
pub fn content_hash(content: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(content).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    format!("{digest:x}")
}
