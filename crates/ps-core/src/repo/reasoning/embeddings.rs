use crate::Error;

use time::OffsetDateTime;
use uuid::Uuid;

use super::ReasoningRepo;

/// Embedding queue query row.
type EmbeddingQueueRow = (
    Uuid,
    Uuid,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
);

/// Similarity search query row.
type SimilarRow = (
    Uuid,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    OffsetDateTime,
    Option<String>,
    f64,
);

// ---------------------------------------------------------------------------
// Embedding types
// ---------------------------------------------------------------------------

/// An entry to insert into the embedding queue.
#[derive(Clone)]
pub struct EmbeddingQueueEntry {
    pub contribution_id: Uuid,
    pub content_hash: String,
}

/// A contribution queued for embedding, with its content and enrichments pre-loaded.
///
/// Derives Serialize/Deserialize so it can be passed through Restate's
/// `ctx.run()` journal.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct QueuedEmbedding {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub content_hash: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub contribution_type: String,
    pub platform: String,
    pub enrichments: Vec<QueuedEnrichmentData>,
}

/// A single enrichment attached to a queued embedding item.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct QueuedEnrichmentData {
    pub enrichment_type: String,
    pub value: serde_json::Value,
}

/// A similar contribution result from a vector search.
pub struct SimilarContribution {
    pub contribution_id: Uuid,
    pub title: Option<String>,
    pub platform: String,
    pub contribution_type: String,
    pub state: Option<String>,
    pub author_name: Option<String>,
    pub external_url: Option<String>,
    pub distance: f64,
    pub created_at: OffsetDateTime,
}

/// Embedding pipeline status.
pub struct EmbeddingStatus {
    pub queued_count: i64,
    pub embedded_count: i64,
    pub total_eligible: i64,
    pub last_embedded_at: Option<OffsetDateTime>,
}

impl ReasoningRepo {
    // -----------------------------------------------------------------------
    // Embedding queue
    // -----------------------------------------------------------------------

    /// Bulk enqueue contributions for embedding. `ON CONFLICT DO NOTHING` (idempotent).
    pub async fn bulk_enqueue_embeddings(
        &self,
        entries: &[EmbeddingQueueEntry],
    ) -> Result<u64, Error> {
        if entries.is_empty() {
            return Ok(0);
        }

        let contribution_ids: Vec<Uuid> = entries.iter().map(|e| e.contribution_id).collect();
        let hashes: Vec<&str> = entries.iter().map(|e| e.content_hash.as_str()).collect();

        // Runtime query — the embedding_queue table is new and not yet in the
        // sqlx offline cache. Will be migrated to query!() after sqlx prepare.
        let result = sqlx::query(
            r"
            INSERT INTO reasoning.embedding_queue (contribution_id, content_hash)
            SELECT unnest($1::uuid[]), unnest($2::text[])
            ON CONFLICT (contribution_id) DO NOTHING
            ",
        )
        .bind(&contribution_ids)
        .bind(&hashes as &[&str])
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected())
    }

    /// Fetch a batch of queued contributions with their content and enrichments.
    ///
    /// JOINs `activity.contributions` for title/body and LEFT JOINs
    /// `reasoning.enrichments` for rationale text.
    pub async fn find_queued_for_embedding(
        &self,
        limit: i64,
    ) -> Result<Vec<QueuedEmbedding>, Error> {
        // Step 1: Fetch queue entries joined with contribution data.
        let rows: Vec<EmbeddingQueueRow> = sqlx::query_as(
            r"
                SELECT
                    eq.id,
                    eq.contribution_id,
                    eq.content_hash,
                    c.title,
                    c.content,
                    c.contribution_type,
                    c.platform
                FROM reasoning.embedding_queue eq
                JOIN activity.contributions c ON c.id = eq.contribution_id
                ORDER BY eq.created_at
                LIMIT $1
                ",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        if rows.is_empty() {
            return Ok(vec![]);
        }

        // Step 2: Batch-fetch enrichments for all these contributions.
        let contribution_ids: Vec<Uuid> = rows.iter().map(|r| r.1).collect();
        let enrichment_rows: Vec<(Uuid, String, serde_json::Value)> = sqlx::query_as(
            r"
            SELECT contribution_id, enrichment_type, value
            FROM reasoning.enrichments
            WHERE contribution_id = ANY($1)
            ORDER BY contribution_id, enrichment_type
            ",
        )
        .bind(&contribution_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        // Group enrichments by contribution_id.
        let mut enrichment_map: std::collections::HashMap<Uuid, Vec<QueuedEnrichmentData>> =
            std::collections::HashMap::new();
        for (cid, etype, value) in enrichment_rows {
            enrichment_map
                .entry(cid)
                .or_default()
                .push(QueuedEnrichmentData {
                    enrichment_type: etype,
                    value,
                });
        }

        Ok(rows
            .into_iter()
            .map(
                |(id, contribution_id, content_hash, title, body, contribution_type, platform)| {
                    QueuedEmbedding {
                        id,
                        contribution_id,
                        content_hash,
                        title,
                        body,
                        contribution_type,
                        platform,
                        enrichments: enrichment_map.remove(&contribution_id).unwrap_or_default(),
                    }
                },
            )
            .collect())
    }

    /// Delete queue entries for contributions that now have embeddings.
    pub async fn delete_embedded_queue_entries(&self) -> Result<u64, Error> {
        let result = sqlx::query(
            r"
            DELETE FROM reasoning.embedding_queue eq
            WHERE EXISTS (
                SELECT 1 FROM reasoning.embeddings e
                WHERE e.contribution_id = eq.contribution_id
            )
            ",
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected())
    }

    // -----------------------------------------------------------------------
    // Embedding storage
    // -----------------------------------------------------------------------

    /// Store a single embedding. Upserts on (`contribution_id`, `model_name`).
    pub async fn upsert_embedding(
        &self,
        contribution_id: Uuid,
        embedding: &pgvector::Vector,
        model_name: &str,
    ) -> Result<(), Error> {
        sqlx::query(
            r"
            INSERT INTO reasoning.embeddings (contribution_id, embedding, model_name)
            VALUES ($1, $2, $3)
            ON CONFLICT (contribution_id, model_name)
            DO UPDATE SET embedding = EXCLUDED.embedding, created_at = now()
            ",
        )
        .bind(contribution_id)
        .bind(embedding)
        .bind(model_name)
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Bulk upsert embeddings. Iterates per-row since sqlx doesn't support
    /// `vector[]` UNNEST natively.
    pub async fn bulk_upsert_embeddings(
        &self,
        contribution_ids: &[Uuid],
        embeddings: &[Vec<f32>],
        model_name: &str,
    ) -> Result<u64, Error> {
        if contribution_ids.is_empty() {
            return Ok(0);
        }

        let mut count = 0u64;
        for (cid, emb) in contribution_ids.iter().zip(embeddings.iter()) {
            let vector = pgvector::Vector::from(emb.clone());
            self.upsert_embedding(*cid, &vector, model_name).await?;
            count += 1;
        }

        Ok(count)
    }

    // -----------------------------------------------------------------------
    // Similarity queries
    // -----------------------------------------------------------------------

    /// Find contributions with embeddings most similar to the given vector.
    /// Filters by optional platform. Returns top N sorted by cosine distance.
    pub async fn find_similar(
        &self,
        embedding: &[f32],
        limit: i64,
        platform_filter: Option<&str>,
        exclude_contribution_id: Option<Uuid>,
    ) -> Result<Vec<SimilarContribution>, Error> {
        let vector = pgvector::Vector::from(embedding.to_vec());

        let rows: Vec<SimilarRow> = sqlx::query_as(
            r"
            SELECT
                c.id,
                c.title,
                c.platform,
                c.contribution_type,
                c.state,
                c.url AS external_url,
                c.created_at,
                p.name AS display_name,
                (e.embedding <=> $1::vector)::float8 as distance
            FROM reasoning.embeddings e
            JOIN activity.contributions c ON c.id = e.contribution_id
            LEFT JOIN org.people p ON p.id = c.person_id
            WHERE ($2::text IS NULL OR c.platform ILIKE $2 OR c.platform ILIKE $2 || '-%')
              AND ($3::uuid IS NULL OR c.id != $3)
              AND e.embedding <=> $1::vector < 0.5
            ORDER BY e.embedding <=> $1::vector
            LIMIT $4
            ",
        )
        .bind(&vector)
        .bind(platform_filter)
        .bind(exclude_contribution_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    contribution_id,
                    title,
                    platform,
                    contribution_type,
                    state,
                    external_url,
                    created_at,
                    author_name,
                    distance,
                )| {
                    SimilarContribution {
                        contribution_id,
                        title,
                        platform,
                        contribution_type,
                        state,
                        author_name,
                        external_url,
                        distance,
                        created_at,
                    }
                },
            )
            .collect())
    }

    /// Find similar contributions to a given contribution (by ID).
    pub async fn find_similar_to_contribution(
        &self,
        contribution_id: Uuid,
        limit: i64,
        platform_filter: Option<&str>,
    ) -> Result<Vec<SimilarContribution>, Error> {
        // Look up the contribution's embedding vector.
        let vector: Option<(pgvector::Vector,)> = sqlx::query_as(
            r"
            SELECT embedding
            FROM reasoning.embeddings
            WHERE contribution_id = $1
            LIMIT 1
            ",
        )
        .bind(contribution_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        let Some((vector,)) = vector else {
            return Ok(vec![]);
        };

        self.find_similar(
            vector.as_slice(),
            limit,
            platform_filter,
            Some(contribution_id),
        )
        .await
    }

    /// Check if a contribution has an embedding.
    pub async fn has_embedding(&self, contribution_id: Uuid) -> Result<bool, Error> {
        let (exists,): (bool,) = sqlx::query_as(
            r"
            SELECT EXISTS(
                SELECT 1 FROM reasoning.embeddings WHERE contribution_id = $1
            )
            ",
        )
        .bind(contribution_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(exists)
    }

    /// Get embedding pipeline status. Runs three independent count queries
    /// in parallel for lower latency.
    pub async fn get_embedding_status(&self) -> Result<EmbeddingStatus, Error> {
        let (queued, embedded, eligible) = tokio::try_join!(
            async {
                let (count,): (i64,) =
                    sqlx::query_as(r"SELECT COUNT(*)::bigint FROM reasoning.embedding_queue")
                        .fetch_one(&self.pool)
                        .await
                        .map_err(Error::from)?;
                Ok::<_, Error>(count)
            },
            async {
                let row: (i64, Option<OffsetDateTime>) = sqlx::query_as(
                    r"SELECT COUNT(*)::bigint, MAX(created_at) FROM reasoning.embeddings",
                )
                .fetch_one(&self.pool)
                .await
                .map_err(Error::from)?;
                Ok::<_, Error>(row)
            },
            async {
                let (count,): (i64,) = sqlx::query_as(
                    r"
                    SELECT COUNT(DISTINCT contribution_id)::bigint
                    FROM (
                        SELECT contribution_id FROM reasoning.enrichments
                        UNION
                        SELECT id FROM activity.contributions
                        WHERE contribution_type = 'jira_ticket'
                          AND (title IS NOT NULL OR content IS NOT NULL)
                    ) eligible
                    ",
                )
                .fetch_one(&self.pool)
                .await
                .map_err(Error::from)?;
                Ok::<_, Error>(count)
            },
        )?;

        Ok(EmbeddingStatus {
            queued_count: queued,
            embedded_count: embedded.0,
            total_eligible: eligible,
            last_embedded_at: embedded.1,
        })
    }
}
