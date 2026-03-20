# Plan 52: Embeddings & Similarity (Phase 3, Workstream 2)

## Context

This plan details the implementation of W2 from [Phase 3](./14-phase3-intelligence.md). W0 (Provider Foundation) is complete: Rig is adopted ([plan 40](./40-adopt-rig-framework.md)), `TaskRouter` routes to Rig clients, cost tracking logs to `reasoning.api_usage`, and the AI Settings admin tab exists. W1 (Enrichment Pipeline) is also complete: the enrichment queue, `EnrichmentHandler`, and `ReasoningRepo` enrichment methods are all in place.

W2 adds vector embeddings for contributions, enabling similarity search across platforms — the foundation for "find similar PRs", "what Jira ticket relates to this PR?", and the RAG context source that W3's agentic interface will use.

**References:**
- [14 Phase 3 Intelligence](./14-phase3-intelligence.md) — W2 section, deliverables, similarity queries
- [40 Adopt Rig Framework](./40-adopt-rig-framework.md) — Rig `EmbeddingModel` + `VectorStoreIndex` trait
- [04 Database Design](./04-database-design.md) — `reasoning.embeddings` table definition
- [06 AI Reasoning](./06-ai-reasoning.md) — embedding strategy, what to embed / not embed
- [42 Enrichment Queue](./42-enrichment-queue.md) — queue pattern this plan mirrors

---

## Deliverables

1. **pgvector migration** — enable the `vector` extension, create `reasoning.embeddings` table and IVFFlat index
2. **Embedding queue** — `reasoning.embedding_queue` table for tracking which contributions need embedding
3. **ReasoningRepo embedding methods** — store, query, similarity search, queue management
4. **Embedding generation pipeline** — `ps-reasoning/src/features/embeddings/` module using Rig's `EmbeddingModel`
5. **EmbeddingHandler** — Restate service handler running periodic embedding cycles
6. **PgVectorIndex** — Rig `VectorStoreIndex` impl wrapping pgvector queries (for W3 RAG)
7. **Similarity gRPC API** — `FindSimilar` and `SearchByText` RPCs on `ReasoningService`
8. **Similarity UI** — "Related items" panel on contribution detail views, cross-platform link badges
9. **Embedding pipeline status** — added to the AI Pipeline Status section on the Ingestion Status page
10. **Proto definitions** — request/response messages for similarity endpoints
11. **Integration tests** — pgvector queries, similarity search, pipeline end-to-end

---

## Architecture Overview

```
Ingestion handlers (GitHub/Jira/Discourse)
    │
    │  store_batch() → enrichment queue
    ▼
EnrichmentHandler (Restate service)
    │
    │  after enrichment cycle completes,
    │  enqueue enriched contribution IDs + fire-and-forget
    ▼
reasoning.embedding_queue          ← "what needs embedding"
    │
    │  EmbeddingHandler (Restate service, triggered by enrichment)
    ▼
Rig EmbeddingModel (Gemini Embedding 2)
    │
    │  batch embed (raw content + enrichment rationale)
    ▼
reasoning.embeddings (pgvector)    ← IVFFlat cosine index
    │
    │  similarity queries
    ▼
ReasoningService gRPC              ← FindSimilar / SearchByText
    │
    ▼
Frontend: related items panel, cross-platform badges
```

### Why embed after enrichment

Embedding input is **raw contribution text + enrichment rationale**. This produces richer vectors than raw text alone:

1. **Coverage** — PRs with empty or terse descriptions are no longer skipped. The enrichment significance rationale (e.g. *"Notable: introduces new authentication middleware with 340 lines changed across 8 files"*) provides semantic content even when the author wrote nothing.
2. **Better cross-platform matching** — enrichment rationale describes *what the contribution does* in normalised natural language, improving matches between a PR and a related Jira ticket regardless of how each author phrased things.
3. **Review quality signal** — a review body of "LGTM, one nit" alone is near-useless for similarity. With depth rationale (*"Score 2: superficial approval with a minor style comment"*), the embedding captures review quality patterns.
4. **Negligible cost** — embedding the enrichment adds a few hundred tokens per item at $0.20/1M tokens.

The coupling cost is low: enrichment already triggers downstream work (metrics, insights). Embedding becomes another downstream step in the same chain. The extra latency (~one enrichment cycle) is invisible to users — nobody needs similarity results within seconds of ingestion.

---

## Database

### Prerequisite: Switch PostgreSQL image to pgvector

The current K8s deployment uses `postgres:17` (vanilla), which does not ship the pgvector shared library. The `CREATE EXTENSION vector` migration will fail without it.

**Change:** In `k8s/base/postgres.yaml`, replace the image:

```yaml
# before
image: postgres:17
# after
image: pgvector/pgvector:pg17
```

**This is a safe, zero-downtime swap:**

- `pgvector/pgvector:pg17` is built `FROM postgres:17` — identical PostgreSQL major version, same entrypoint, same data directory layout (`/var/lib/postgresql/data`).
- The PVC (`pgdata`) is a `ReadWriteOnce` persistent volume that survives pod restarts. PostgreSQL will start up against the existing data directory exactly as before — no `pg_upgrade`, no dump/restore, no data conversion.
- The only difference is that the `vector.so` shared library is present in the image's extension directory, making `CREATE EXTENSION vector` available.
- Rolling back is equally safe: switching back to `postgres:17` leaves the data intact (the extension rows in `pg_catalog` become orphaned but don't prevent startup; you'd just need to `DROP EXTENSION vector CASCADE` first if reverting fully).

**Deployment steps:**

1. Update `k8s/base/postgres.yaml` image to `pgvector/pgvector:pg17`
2. Apply the manifest — K8s will terminate the old pod and start a new one with the new image, reattaching the same PVC
3. Verify: `kubectl exec -it postgres-0 -- psql -U prism -c "SELECT * FROM pg_available_extensions WHERE name = 'vector';"`  — should show `vector` as available
4. Run migrations normally (ps-migrate init container will execute the `CREATE EXTENSION` migration on next deploy)

### Migration 1: Enable pgvector

```sql
-- XXXX_enable_pgvector.sql
CREATE EXTENSION IF NOT EXISTS vector;
```

This must run as a separate migration before the embeddings table migration, because `vector(N)` column types require the extension to exist.

### Migration 2: Embeddings table

Already defined in [04-database-design.md](./04-database-design.md):

```sql
-- XXXX_create_embeddings.sql
CREATE TABLE reasoning.embeddings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id UUID NOT NULL REFERENCES activity.contributions(id) ON DELETE CASCADE,
    embedding vector(1024),
    model_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (contribution_id, model_name)
);

-- IVFFlat requires data to exist before index creation for accurate clustering.
-- Create the index after initial data load, or accept suboptimal clusters initially.
CREATE INDEX idx_embeddings_vector ON reasoning.embeddings
    USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);
```

**Dimension choice:** 1024 via MRL truncation from Gemini Embedding 2's native 3072. This is a good balance of quality and pgvector performance. The dimension is a column type constraint — changing it requires a migration + re-embedding all content.

### Migration 3: Embedding queue

Mirrors the enrichment queue pattern from [plan 42](./42-enrichment-queue.md):

```sql
-- XXXX_create_embedding_queue.sql
CREATE TABLE reasoning.embedding_queue (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id UUID NOT NULL REFERENCES activity.contributions(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (contribution_id)
);

CREATE INDEX idx_embedding_queue_created ON reasoning.embedding_queue(created_at);
```

The queue holds contributions that need embedding. Entries are enqueued after enrichment completes (or directly during Jira ingestion), and dequeued by the `EmbeddingHandler` after successful embedding. The `content_hash` (SHA-256 of the text that will be embedded) allows skipping re-embedding when content hasn't changed.

### Why a queue instead of a LEFT JOIN scan

The enrichment pipeline taught us this lesson: scanning `activity.contributions LEFT JOIN reasoning.embeddings WHERE embedding IS NULL` is expensive at scale and doesn't give control over ordering or priority. A dedicated queue table:

- Is cheap to scan (small table, indexed by `created_at`)
- Naturally orders by arrival time
- Allows deduplication via `UNIQUE (contribution_id)` with `ON CONFLICT DO NOTHING`
- Allows cleanup of fully-processed entries
- Decouples enrichment from embedding — enrichment just enqueues and moves on

---

## What Gets Embedded

Every contribution type that receives enrichment is eligible for embedding. Since embedding runs *after* enrichment, we always have enrichment rationale available — this eliminates the need for most content filters (empty descriptions, short review bodies) that would otherwise gate eligibility.

| Content Type | Embedding Input | Filter | Rationale |
|---|---|---|---|
| Pull requests | `title + "\n\n" + description + "\n\n" + significance rationale` | Has at least one enrichment | Significance rationale fills the gap when description is empty |
| Code reviews | `body + "\n\n" + depth rationale + "\n\n" + sentiment label` | Has at least one enrichment | Depth rationale captures review quality even for terse "LGTM" reviews |
| Discourse topics | `title + "\n\n" + first_post_body + "\n\n" + topic classification` | Has at least one enrichment | Topic tags improve clustering |
| Jira tickets | `summary + "\n\n" + description` | Non-empty summary or description | No enrichment yet (per W1 plan), so raw text only for now |

**Not embedded directly:** code diffs (too noisy/large as raw text — but the *substance* of the diff is captured via the significance enrichment rationale, which summarises what changed and why it matters), mailing list messages (low signal), Google Drive content (not integrated), review comments (individual comments are too short — the review body is sufficient).

**Jira note:** Jira tickets don't receive enrichment in W1, so they're embedded from raw text only and enqueued directly during ingestion (not after enrichment). When Jira enrichment is added later, Jira embeddings will move to the enrichment-first pipeline and benefit from the same rationale-enriched input.

### Text preparation

Before embedding, text is assembled and normalised:

1. **Assemble input** — concatenate raw contribution text with enrichment rationale (see `build_embedding_text()` below). Enrichment sections are prefixed with labels (e.g. `Significance: Notable — rationale...`) to give the embedding model structural context.
2. **Truncate to 8,192 tokens** — Gemini Embedding 2's input limit. In practice, most contribution text + enrichment is well under this. Truncate by characters (rough estimate: 4 chars ≈ 1 token, so truncate at 32,000 chars) rather than tokenising.
3. **Strip HTML** — Jira and Discourse content may contain HTML markup. Strip tags, keep text content.
4. **Collapse whitespace** — normalise runs of whitespace to single spaces. Remove leading/trailing whitespace.
5. **No lowercasing** — embedding models handle casing; lowercasing destroys signal (e.g. acronyms, proper nouns).

This logic lives in `ps-reasoning/src/features/embeddings/text.rs`.

---

## Embedding Queue Population

Contributions are enqueued for embedding **after enrichment completes**, not during ingestion. This ensures the embedding input includes enrichment rationale for richer vectors.

### Enqueue point: end of `EnrichmentHandler::run_cycle()`

After the enrichment handler finishes processing a batch, it enqueues the newly-enriched contribution IDs into `reasoning.embedding_queue`, then fires-and-forgets to `EmbeddingHandler::run_cycle()`.

```rust
// In EnrichmentHandler::run_cycle(), after enrichment batch completes:

// 1. Enqueue enriched contributions for embedding (journaled)
ctx.run(move || {
    let entries: Vec<EmbeddingQueueEntry> = enriched_contribution_ids
        .iter()
        .map(|&id| EmbeddingQueueEntry {
            contribution_id: id,
            content_hash: String::new(), // computed at embed time from full text + enrichments
        })
        .collect();
    repos.reasoning.bulk_enqueue_embeddings(&entries).await
}).name("enqueue_embeddings").await?;

// 2. Trigger embedding handler (fire-and-forget)
ctx.service_client::<EmbeddingHandlerClient>()
    .run_cycle()
    .send();
```

This gives us the chain: **ingestion → enrichment → embedding**, with each link as a Restate fire-and-forget.

### Jira exception

Jira tickets have no enrichment in W1, so they are enqueued for embedding directly during ingestion (in `store_batch()`), following the original pattern. When Jira enrichment is added later, this moves to the enrichment-first path.

### Backfill

For existing contributions that already have enrichments but no embeddings, a one-time backfill enqueues them:

```sql
INSERT INTO reasoning.embedding_queue (contribution_id, content_hash)
SELECT DISTINCT e.contribution_id, ''
FROM reasoning.enrichments e
LEFT JOIN reasoning.embeddings emb ON emb.contribution_id = e.contribution_id
WHERE emb.id IS NULL
ON CONFLICT (contribution_id) DO NOTHING;
```

For Jira tickets (no enrichments):

```sql
INSERT INTO reasoning.embedding_queue (contribution_id, content_hash)
SELECT c.id, ''
FROM activity.contributions c
LEFT JOIN reasoning.embeddings e ON e.contribution_id = c.id
WHERE e.id IS NULL
  AND c.contribution_type = 'jira_ticket'
  AND (c.title IS NOT NULL OR c.body IS NOT NULL)
ON CONFLICT (contribution_id) DO NOTHING;
```

Both run as `psctl embed --backfill` or an admin RPC, not automatically.

---

## ReasoningRepo: Embedding Methods

Added to `crates/ps-core/src/repo/reasoning.rs`, following the existing pattern:

### Types

```rust
/// A stored embedding record (without the vector, for metadata queries).
pub struct EmbeddingRecord {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub model_name: String,
    pub created_at: OffsetDateTime,
}

/// A contribution queued for embedding, with its content and enrichments.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct QueuedEmbedding {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub content_hash: String,
    // Contribution fields:
    pub title: Option<String>,
    pub body: Option<String>,
    pub contribution_type: String,
    pub platform: String,
    // Enrichment rationale (populated by JOINing reasoning.enrichments):
    pub enrichments: Vec<QueuedEnrichment>,
}

/// A single enrichment attached to a queued embedding item.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct QueuedEnrichment {
    pub enrichment_type: String,
    pub value: serde_json::Value, // contains label/score + rationale
}

/// Input for bulk enqueue.
pub struct EmbeddingQueueEntry {
    pub contribution_id: Uuid,
    pub content_hash: String,
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
```

### Methods

```rust
impl ReasoningRepo {
    // -- Queue management --

    /// Enqueue contributions for embedding. ON CONFLICT DO NOTHING (idempotent).
    pub async fn bulk_enqueue_embeddings(&self, entries: &[EmbeddingQueueEntry]) -> Result<u64, Error>;

    /// Fetch a batch of queued contributions with their content and enrichments.
    /// JOINs activity.contributions for title/body and reasoning.enrichments for
    /// rationale text. LEFT JOINs reasoning.embeddings to skip contributions
    /// that already have an embedding with the same content_hash.
    /// Returns items with their enrichments pre-loaded for text assembly.
    pub async fn find_queued_for_embedding(&self, limit: i64) -> Result<Vec<QueuedEmbedding>, Error>;

    /// Delete queue entries for contributions that now have embeddings.
    pub async fn delete_embedded_queue_entries(&self) -> Result<u64, Error>;

    /// Get queue and pipeline stats.
    pub async fn get_embedding_status(&self) -> Result<EmbeddingStatus, Error>;

    // -- Embedding storage --

    /// Bulk upsert embeddings using UNNEST. ON CONFLICT (contribution_id, model_name)
    /// updates the vector and timestamp.
    pub async fn bulk_upsert_embeddings(
        &self,
        contribution_ids: &[Uuid],
        embeddings: &[Vec<f32>],
        model_name: &str,
    ) -> Result<u64, Error>;

    // -- Similarity queries --

    /// Find contributions with embeddings most similar to the given vector.
    /// Filters by optional platform. Returns top N results sorted by cosine distance.
    pub async fn find_similar(
        &self,
        embedding: &[f32],
        limit: i64,
        platform_filter: Option<&str>,
        exclude_contribution_id: Option<Uuid>,
    ) -> Result<Vec<SimilarContribution>, Error>;

    /// Find similar contributions to a given contribution (by ID).
    /// Looks up the contribution's embedding, then queries for similar vectors.
    pub async fn find_similar_to_contribution(
        &self,
        contribution_id: Uuid,
        limit: i64,
        platform_filter: Option<&str>,
    ) -> Result<Vec<SimilarContribution>, Error>;

    /// Check if a contribution has an embedding.
    pub async fn has_embedding(&self, contribution_id: Uuid) -> Result<bool, Error>;
}
```

### Key SQL patterns

**Similarity search:**

```sql
SELECT c.id AS contribution_id, c.title, c.platform, c.contribution_type,
       c.state, c.external_url, c.created_at,
       p.display_name AS author_name,
       e.embedding <=> $1::vector AS distance
FROM reasoning.embeddings e
JOIN activity.contributions c ON c.id = e.contribution_id
LEFT JOIN org.people p ON p.id = c.person_id
WHERE ($2::text IS NULL OR c.platform = $2)
  AND ($3::uuid IS NULL OR c.id != $3)
  AND e.embedding <=> $1::vector < 0.5  -- distance threshold
ORDER BY e.embedding <=> $1::vector
LIMIT $4;
```

The `<=>` operator is pgvector's cosine distance (0 = identical, 2 = opposite). A threshold of 0.5 filters out low-relevance results. The IVFFlat index accelerates this query.

**Bulk upsert with UNNEST:**

```sql
INSERT INTO reasoning.embeddings (contribution_id, embedding, model_name)
SELECT * FROM UNNEST($1::uuid[], $2::vector[], $3::text[])
ON CONFLICT (contribution_id, model_name)
DO UPDATE SET embedding = EXCLUDED.embedding, created_at = now();
```

Note: sqlx's `query!` macro doesn't directly support `vector[]` array types. We'll need to use `query_scalar!` with a raw cast or the `pgvector` crate's `Vector` type. The `pgvector` Rust crate provides `pgvector::Vector` which implements `sqlx::Type` and `sqlx::Encode`. Individual inserts may be needed if UNNEST doesn't work cleanly with vector arrays — benchmark both approaches.

---

## ps-reasoning: Embeddings Feature Module

### File structure

```
crates/ps-reasoning/src/features/embeddings/
├── mod.rs       # process_embedding_batch(), log_embedding_cost()
├── text.rs      # build_embedding_text(), normalise_text(), strip_html()
└── pgvector.rs  # PgVectorIndex impl for Rig's VectorStoreIndex trait
```

### mod.rs — Batch processing

```rust
use rig::providers::gemini;

pub struct BatchResult {
    pub embedded: usize,
    pub skipped: usize,
    pub errors: usize,
    pub usage: EmbeddingUsage,
}

pub struct EmbeddingUsage {
    pub total_tokens: u64,
}

/// Process a batch of queued contributions:
/// 1. Build embedding text for each item
/// 2. Filter out items where content_hash matches an existing embedding (unchanged)
/// 3. Call Rig's EmbeddingModel::embed_texts() in sub-batches of 100
/// 4. Store vectors via ReasoningRepo::bulk_upsert_embeddings()
/// 5. Return batch result with counts and token usage
pub async fn process_embedding_batch(
    items: &[QueuedEmbedding],
    model: &dyn EmbeddingModel,
    repos: &Repos,
    model_name: &str,
) -> Result<BatchResult, Error> {
    // Build texts
    let texts: Vec<(Uuid, String)> = items
        .iter()
        .filter_map(|item| {
            let text = build_embedding_text(item)?;
            Some((item.contribution_id, text))
        })
        .collect();

    if texts.is_empty() {
        return Ok(BatchResult::empty());
    }

    // Sub-batch into groups of 100 (Gemini supports up to 2048 per call,
    // but 100 keeps memory and latency manageable)
    let mut total_embedded = 0;
    let mut total_usage = EmbeddingUsage { total_tokens: 0 };

    for chunk in texts.chunks(100) {
        let text_strs: Vec<&str> = chunk.iter().map(|(_, t)| t.as_str()).collect();
        let ids: Vec<Uuid> = chunk.iter().map(|(id, _)| *id).collect();

        // Rig's embedding call — returns Vec<Vec<f32>>
        let embeddings = model.embed_texts(&text_strs).await?;

        // Truncate to 1024 dimensions (MRL)
        let truncated: Vec<Vec<f32>> = embeddings
            .into_iter()
            .map(|v| v.into_iter().take(1024).collect())
            .collect();

        repos.reasoning
            .bulk_upsert_embeddings(&ids, &truncated, model_name)
            .await?;

        total_embedded += chunk.len();
    }

    Ok(BatchResult {
        embedded: total_embedded,
        skipped: items.len() - texts.len(),
        errors: 0,
        usage: total_usage,
    })
}
```

### text.rs — Text extraction and normalisation

```rust
/// Build the text to embed for a queued contribution.
/// Combines raw contribution text with enrichment rationale for richer embeddings.
/// Returns None only if there is no content AND no enrichments (nothing to embed).
pub fn build_embedding_text(item: &QueuedEmbedding) -> Option<String> {
    let mut sections: Vec<String> = Vec::new();

    // 1. Raw contribution text
    match item.contribution_type.as_str() {
        "pull_request" => {
            let title = item.title.as_deref().unwrap_or_default();
            let body = item.body.as_deref().unwrap_or_default();
            if !title.is_empty() || !body.is_empty() {
                sections.push(format!("{title}\n\n{body}"));
            }
        }
        "pr_review" => {
            let body = item.body.as_deref().unwrap_or_default();
            if !body.is_empty() {
                sections.push(body.to_string());
            }
        }
        "discourse_topic" => {
            let title = item.title.as_deref().unwrap_or_default();
            let body = item.body.as_deref().unwrap_or_default();
            if !title.is_empty() || !body.is_empty() {
                sections.push(format!("{title}\n\n{body}"));
            }
        }
        "jira_ticket" => {
            let summary = item.title.as_deref().unwrap_or_default();
            let description = item.body.as_deref().unwrap_or_default();
            if !summary.is_empty() || !description.is_empty() {
                sections.push(format!("{summary}\n\n{description}"));
            }
        }
        _ => return None,
    }

    // 2. Enrichment rationale — appended as labelled sections
    for enrichment in &item.enrichments {
        if let Some(text) = format_enrichment(enrichment) {
            sections.push(text);
        }
    }

    if sections.is_empty() {
        return None;
    }

    Some(normalise_text(&sections.join("\n\n")))
}

/// Format an enrichment's value into a labelled text section for embedding.
/// Extracts the rationale (the most semantically useful part) from the JSON value.
fn format_enrichment(enrichment: &QueuedEnrichment) -> Option<String> {
    let v = &enrichment.value;
    match enrichment.enrichment_type.as_str() {
        "significance" => {
            let label = v.get("label")?.as_str()?;
            let rationale = v.get("rationale")?.as_str()?;
            Some(format!("Significance: {label} — {rationale}"))
        }
        "review_depth" => {
            let score = v.get("score")?;
            let rationale = v.get("rationale")?.as_str()?;
            Some(format!("Review depth: {score}/5 — {rationale}"))
        }
        "sentiment" => {
            let label = v.get("label")?.as_str()?;
            Some(format!("Sentiment: {label}"))
        }
        "topic" => {
            let categories = v.get("categories")?;
            Some(format!("Topics: {categories}"))
        }
        _ => None,
    }
}

/// Strip HTML tags, collapse whitespace, truncate to ~32k chars.
pub fn normalise_text(input: &str) -> String { /* ... */ }
```

The key insight is that `format_enrichment()` extracts the **rationale** — the natural-language explanation — not just the label or score. The rationale is the most semantically rich part and is what makes cross-platform matching work well. A PR with an empty description but a significance rationale of *"Notable: introduces new authentication middleware with 340 lines changed across 8 files, restructuring the session handling layer"* gives the embedding model substantive content to work with.

### pgvector.rs — Rig VectorStoreIndex

This is the bridge between Rig's agent system and our pgvector storage. W3 will use it for RAG context and the `search_similar` / `search_by_text` agent tools.

```rust
use rig::vector_store::VectorStoreIndex;

pub struct PgVectorIndex {
    repo: ReasoningRepo,
    model: Arc<dyn EmbeddingModel>,
}

impl PgVectorIndex {
    pub fn new(repo: ReasoningRepo, model: Arc<dyn EmbeddingModel>) -> Self {
        Self { repo, model }
    }
}

#[async_trait]
impl VectorStoreIndex for PgVectorIndex {
    type SearchParams = PgVectorSearchParams;

    async fn top_n(&self, query: &str, n: usize) -> Result<Vec<(f64, Document)>> {
        // 1. Embed the query text on-the-fly
        let embedding = self.model.embed_text(query).await?;
        let truncated: Vec<f32> = embedding.into_iter().take(1024).collect();

        // 2. Query pgvector for similar contributions
        let results = self.repo
            .find_similar(&truncated, n as i64, None, None)
            .await?;

        // 3. Map to Rig's Document type
        Ok(results.into_iter().map(|r| {
            let doc = Document {
                id: r.contribution_id.to_string(),
                text: r.title.unwrap_or_default(),
                metadata: serde_json::json!({
                    "platform": r.platform,
                    "contribution_type": r.contribution_type,
                    "author": r.author_name,
                    "url": r.external_url,
                }),
            };
            (r.distance, doc)
        }).collect())
    }
}
```

This allows W3's agent builder to use `.dynamic_context(n, pgvector_index)` for RAG-augmented queries.

---

## EmbeddingHandler — Restate Service

### Pattern

Follows the `EnrichmentHandler` pattern exactly: a Restate **service** (singleton, not per-source) that runs periodic embedding cycles.

**File:** `crates/ps-workers/src/handlers/embedding.rs`

```rust
pub struct EmbeddingHandlerImpl {
    pub state: SharedState,
    pub router: Arc<RwLock<TaskRouter>>,
}

#[restate_sdk::service]
pub trait EmbeddingHandler {
    /// Run one embedding cycle: process queued contributions, embed, store.
    async fn run_cycle() -> Result<(), TerminalError>;
}
```

### Handler flow

```
run_cycle()
│
├── 1. Create run record (journaled)
│      create_run!(ctx, repos, "embedding", "EmbeddingHandler", "run_cycle")
│
├── 2. Check daily budget (NOT journaled — read-only, re-check on replay is correct)
│      If budget exceeded → complete run with "budget_paused" status
│
├── 3. Resolve embedding model from TaskRouter (NOT journaled)
│      let model = router.read().await.embedding_model()?;
│
├── 4. Fetch queued batch (journaled — DB read)
│      ctx.run(|| repos.reasoning.find_queued_for_embedding(500)).name("fetch_queue")
│
├── 5. Process batch (NOT journaled — API calls are idempotent on replay)
│      process_embedding_batch(&items, &model, &repos, model_name)
│
├── 6. Log cost (journaled)
│      ctx.run(|| repos.reasoning.log_api_usage(...)).name("log_cost")
│
├── 7. Clean up queue (journaled)
│      ctx.run(|| repos.reasoning.delete_embedded_queue_entries()).name("cleanup_queue")
│
├── 8. Update progress (NOT journaled — best effort)
│
├── 9. Complete/fail run (journaled)
│      complete_run! or fail_run!
│
└── 10. If items remain in queue → self-invoke with short delay
        ctx.service_client::<EmbeddingHandlerClient>()
            .run_cycle().send_with_delay(Duration::from_secs(5))
```

### Journaling rules (matching existing handler patterns)

| Step | Inside `ctx.run()`? | Why |
|---|---|---|
| Run creation | Yes | Must be idempotent on replay (UUID generated inside closure) |
| Budget check | No | Re-checking on replay is correct; read-only |
| Model resolution | No | Re-resolving is safe |
| Queue fetch | Yes | DB read, journal captures result to avoid re-reading stale data |
| Embedding API call | No | Responses are large vectors; re-executing is safe (upserts) |
| Cost logging | Yes | Must be idempotent |
| Queue cleanup | Yes | Must be idempotent |
| Progress update | No | Best-effort display data |
| Run completion | Yes | Must be idempotent |

### Batch sizing

- **Queue fetch:** 500 items per cycle (larger than enrichment's batches because embedding is cheaper and faster)
- **API sub-batch:** 100 texts per Gemini API call (well within the 2048 limit, keeps latency reasonable)
- **Concurrency:** Sequential sub-batches within a cycle (embedding calls are already batch-efficient; no need for `buffer_unordered`)

### Triggering

The `EmbeddingHandler` is triggered in two ways:

1. **After enrichment** — the `EnrichmentHandler` enqueues newly-enriched contribution IDs and fires-and-forgets to `EmbeddingHandler::run_cycle()` at the end of its cycle. This is the primary trigger and ensures embeddings include enrichment rationale.
2. **Self-continuation** — at the end of each `run_cycle()`, if items remain in the queue, the handler self-invokes with a 5-second delay. If the queue is empty, it stops. The next enrichment run will kick it off again.

**Jira exception:** Since Jira tickets have no enrichment in W1, ingestion handlers for Jira enqueue directly into `reasoning.embedding_queue` and fire-and-forget to `EmbeddingHandler::run_cycle()` — the same pattern ingestion uses for the enrichment queue today.

The full trigger chain is: **ingestion → enrichment → embedding** (for GitHub, Discourse) and **ingestion → embedding** (for Jira, until Jira enrichment is added).

### Wiring in main.rs

```rust
// In main.rs, alongside other handler construction:
let embedding = EmbeddingHandlerImpl {
    state: state.clone(),
    router: router.clone(), // Same Arc<RwLock<TaskRouter>> as enrichment
};

// In Restate endpoint builder:
.bind(embedding.serve())  // EmbeddingHandler
```

---

## Similarity gRPC API

### Proto definitions

Added to `proto/prism/v1/reasoning.proto`:

```protobuf
message FindSimilarRequest {
  string contribution_id = 1;
  int32 limit = 2;             // default 10, max 50
  optional string platform = 3; // filter results by platform
}

message SearchByTextRequest {
  string query_text = 1;
  int32 limit = 2;              // default 10, max 50
  optional string platform = 3;  // filter results by platform
}

message SimilarItem {
  string contribution_id = 1;
  string title = 2;
  string platform = 3;
  string contribution_type = 4;
  string state = 5;
  string author_name = 6;
  string external_url = 7;
  double distance = 8;          // cosine distance (0 = identical)
  google.protobuf.Timestamp created_at = 9;
}

message SimilarItemsResponse {
  repeated SimilarItem items = 1;
}

message EmbeddingStatusResponse {
  int64 queued_count = 1;
  int64 embedded_count = 2;
  int64 total_eligible = 3;
  optional google.protobuf.Timestamp last_embedded_at = 4;
  double coverage_percent = 5;  // embedded / total_eligible * 100
}
```

Added to the `ReasoningService`:

```protobuf
service ReasoningService {
  // ... existing RPCs ...
  rpc FindSimilar(FindSimilarRequest) returns (SimilarItemsResponse);
  rpc SearchByText(SearchByTextRequest) returns (SimilarItemsResponse);
  rpc GetEmbeddingStatus(google.protobuf.Empty) returns (EmbeddingStatusResponse);
}
```

### Service implementation

**`FindSimilar`** — looks up the contribution's embedding, then calls `find_similar()`:

```rust
async fn find_similar(&self, req: FindSimilarRequest) -> Result<SimilarItemsResponse> {
    let contribution_id = req.contribution_id.parse::<Uuid>()?;
    let limit = req.limit.min(50).max(1) as i64;
    let results = self.repos.reasoning
        .find_similar_to_contribution(contribution_id, limit, req.platform.as_deref())
        .await?;
    Ok(SimilarItemsResponse { items: results.into_iter().map(Into::into).collect() })
}
```

**`SearchByText`** — embeds the query text on-the-fly, then searches:

```rust
async fn search_by_text(&self, req: SearchByTextRequest) -> Result<SimilarItemsResponse> {
    let model = self.router.read().await.embedding_model()?;
    let embedding = model.embed_text(&req.query_text).await?;
    let truncated: Vec<f32> = embedding.into_iter().take(1024).collect();

    // Log the embedding cost
    self.cost_tracker.log_usage("google", &model_name, "embedding", usage).await?;

    let results = self.repos.reasoning
        .find_similar(&truncated, req.limit.min(50).max(1) as i64, req.platform.as_deref(), None)
        .await?;
    Ok(SimilarItemsResponse { items: results.into_iter().map(Into::into).collect() })
}
```

`SearchByText` is the more expensive RPC (requires an on-the-fly embedding call). It's used by the frontend for free-text similarity search and by W3's agent `search_by_text` tool.

---

## Frontend: Similarity UI

### New hook: `useEmbeddings`

**File:** `frontend/lib/hooks/use-embeddings.ts`

```typescript
export const useEmbeddingSimilar = (contributionId: string, options?: {
  limit?: number;
  platform?: string;
  enabled?: boolean;
}) => {
  return useQuery({
    queryKey: ["embeddings", "similar", contributionId, options?.platform],
    queryFn: () => reasoningClient.findSimilar({
      contributionId,
      limit: options?.limit ?? 5,
      platform: options?.platform,
    }),
    enabled: options?.enabled !== false,
  });
};

export const useEmbeddingSearch = () => {
  return useMutation({
    mutationFn: (params: { queryText: string; limit?: number; platform?: string }) =>
      reasoningClient.searchByText(params),
  });
};

export const useEmbeddingStatus = () => {
  return useQuery({
    queryKey: ["embeddings", "status"],
    queryFn: () => reasoningClient.getEmbeddingStatus({}),
    refetchInterval: 30_000,
  });
};
```

### Contribution detail page — new route and navigation

The contribution detail page is a **new page** introduced in this workstream. No standalone contribution view exists today — contribution rows in the person profile and team drilldown pages only link externally (to GitHub, Jira, Discourse via an `ExternalLink` icon). Phase 3 needs an internal page where enrichment badges, similarity results, and cross-platform links can live.

**Route:** `/contributions/:contributionId`

**File:** `frontend/views/contributions/pages/contribution-detail-page.tsx`

**Router addition** in `app.tsx`:
```typescript
<Route path="/contributions/:contributionId" element={<ContributionDetailPage />} />
```

#### How users get there

The contribution detail page is a **drill-down destination**, not a top-level nav item. Users arrive here from existing pages where contributions are already surfaced:

| Source page | Current behaviour | Change |
|---|---|---|
| **Person profile** (`/people/:personId`) — `ContributionTable` rows | Rows show an `ExternalLink` icon linking to GitHub/Jira/Discourse | **Make the row itself clickable** → navigates to `/contributions/:contributionId`. Keep the external link icon as a secondary action (opens source platform in new tab). |
| **Person profile** — `NotableContributionCard` | Card title links externally | **Add internal link** on card title → `/contributions/:contributionId`. Keep external link as secondary icon. |
| **Team detail** (`/teams/:teamId`) — contribution drilldowns | Same external-only pattern | **Same change** — row click navigates internally, external icon preserved. |
| **Similarity panel** — Related Items on another contribution | N/A (new) | Each similar item row links to `/contributions/:contributionId`. |
| **W3 agent answers** (future) | N/A (future) | Citations in agent responses link to `/contributions/:contributionId`. |

The pattern is consistent: **click the row/title → internal detail page; click the external link icon → source platform**. This mirrors how most data platforms handle the internal-vs-external distinction.

#### Why no top-level nav item

Contributions don't get a sidebar entry. The page is a detail view reached by drilling down from People or Teams — the same pattern as `/people/:personId` (reachable from the People list, but People is the nav item, not individual profiles). Adding a top-level "Contributions" list/search page is a future consideration if users want to browse or free-text search contributions directly, but for this workstream the drill-down entry points provide sufficient access.

#### Page content

The page shows full contribution metadata, enrichment badges with provenance, and the Related Items similarity panel:

```
┌─────────────────────────────────────────────────┐
│ Contribution Detail                             │
│                                                 │
│ ┌─ Metadata ──────────────────────────────────┐ │
│ │ Title: Fix race condition in auth middleware │ │
│ │ Platform: GitHub  Type: Pull Request        │ │
│ │ Author: alice  State: Merged                │ │
│ │ Created: Mar 12 14:30                       │ │
│ └─────────────────────────────────────────────┘ │
│                                                 │
│ ┌─ Enrichments ───────────────────────────────┐ │
│ │ [Significant] [Depth: 4/5] [Constructive]   │ │
│ │ (clickable badges with provenance popovers) │ │
│ └─────────────────────────────────────────────┘ │
│                                                 │
│ ┌─ Cross-Platform Links ──────────────────────┐ │
│ │ 🔗 Likely related: PROJ-142 "Auth token     │ │
│ │    expiry handling" (Jira, distance: 0.12)   │ │
│ └─────────────────────────────────────────────┘ │
│                                                 │
│ ┌─ Similar Contributions ─────────────────────┐ │
│ │ 1. Fix session timeout in auth layer        │ │
│ │    GitHub · Pull Request · merged · 0.18    │ │
│ │ 2. Auth middleware refactor proposal         │ │
│ │    Discourse · Topic · 0.23                 │ │
│ │ 3. Update auth token validation             │ │
│ │    GitHub · Pull Request · merged · 0.31    │ │
│ │ 4. PROJ-98 "Implement OAuth2 flow"          │ │
│ │    Jira · Ticket · done · 0.35             │ │
│ └─────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────┘
```

### Component: `RelatedItems`

**File:** `frontend/views/contributions/components/related-items.tsx`

```typescript
const RelatedItems = ({ contributionId }: { contributionId: string }) => {
  const { data, isLoading } = useEmbeddingSimilar(contributionId, { limit: 10 });

  if (isLoading) return <Skeleton className="h-40 w-full" />;
  if (!data?.items.length) return null; // Don't show panel if no similar items

  // Split into cross-platform links (distance < 0.2, different platform)
  // and general similar items
  const crossPlatform = data.items.filter(
    item => item.distance < 0.2 && item.platform !== currentPlatform
  );
  const similar = data.items.filter(
    item => !crossPlatform.includes(item)
  );

  return (
    <div className="space-y-4">
      {crossPlatform.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-sm">
              <Link2 className="size-4" /> Cross-Platform Links
            </CardTitle>
          </CardHeader>
          <CardContent>
            {crossPlatform.map(item => <CrossPlatformLink key={item.contributionId} item={item} />)}
          </CardContent>
        </Card>
      )}

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-sm">
            <Layers className="size-4" /> Similar Contributions
            <Badge variant="secondary">{similar.length}</Badge>
          </CardTitle>
        </CardHeader>
        <CardContent>
          {similar.map(item => <SimilarItemRow key={item.contributionId} item={item} />)}
        </CardContent>
      </Card>
    </div>
  );
};
```

### Cross-platform link prominence

When similarity search finds a high-confidence match (distance < 0.2) on a different platform, it's surfaced as a **cross-platform link** — more prominently than general similar items. This is the "find the Jira ticket for this PR" use case.

The cross-platform link appears as:
- A `Link2` icon with "Likely related:" prefix
- The item's external ID (e.g. "PROJ-142") and title
- Platform badge and distance score
- Click navigates to that contribution's detail view

### Distance display

Raw cosine distances are not meaningful to users. Map to a qualitative label:

| Distance | Label | Colour |
|---|---|---|
| < 0.15 | Very similar | `text-green-600` |
| 0.15–0.30 | Similar | `text-foreground` |
| 0.30–0.45 | Somewhat related | `text-muted-foreground` |
| > 0.45 | (filtered out by SQL threshold) | — |

Display as a subtle pill: `<Badge variant="outline" className="text-xs">Similar · 0.18</Badge>`

### Existing page enhancements

| Page | Enhancement | Component |
|---|---|---|
| `/people/:personId` | "Related work across platforms" section — aggregate cross-platform links for this person's contributions | `PersonCrossPlatformLinks` |
| `/teams/:teamId` | No embedding-specific additions (team-level similarity is too noisy) | — |
| Ingestion Status page | Embedding pipeline stats added to the existing AI Pipeline Status card | See below |
| Admin → Handlers tab | `EmbeddingHandler` appears as an ingestion-adjacent handler with run/cancel controls | See below |

### Ingestion page: AI Pipeline Status card

The existing `AiPipelineStatus` component (`frontend/views/ingestion/components/ai-pipeline-status.tsx`) currently shows enrichment stats only. It needs extending with an embedding section below the enrichment breakdown.

The component already fetches enrichment status and provides Run/Cancel controls for `EnrichmentHandler`. Add a parallel section for embedding:

```
┌─ AI Pipeline ─────────────────────────── [Run Enrichment] ─┐
│                                                             │
│  Enrichment                                                 │
│  Pending: 42       Total: 8,291       Last run: 12m ago     │
│  [Review Depth 2,847] [Sentiment 2,844] [Significance 2,600]│
│                                                             │
│  ──────────────────────────────────────────────────────────  │
│                                                             │
│  Embeddings                              [Run Embedding]    │
│  Queued: 142       Embedded: 3,847       Last run: 5m ago   │
│  Coverage: 94.2%                                            │
│  ████████████████████░░                                     │
└─────────────────────────────────────────────────────────────┘
```

Implementation:
- Add `useEmbeddingStatus()` call to `AiPipelineStatus`
- Add a second Run/Cancel button pair for `EmbeddingHandler` (same pattern as the existing enrichment controls)
- Show queued count, embedded count, coverage percentage with progress bar, last run time
- Separated from enrichment by a `<Separator />`

### Admin → Handlers tab: EmbeddingHandler registration

The existing `HandlersTab` (`frontend/views/admin/components/handlers-tab.tsx`) groups handlers into "Ingestion Handlers" and "System Handlers". Currently, `EnrichmentHandler` is explicitly added to the ingestion group via a hardcoded set:

```typescript
// Current code (handlers-tab.tsx line 52)
const ingestionNames = new Set(["EnrichmentHandler"]);
```

**Change:** Add `EmbeddingHandler` to this set:

```typescript
const ingestionNames = new Set(["EnrichmentHandler", "EmbeddingHandler"]);
```

This ensures `EmbeddingHandler` appears in the "Ingestion Handlers" section alongside the ingestion sources and enrichment handler, where it logically belongs (it's part of the ingestion→enrichment→embedding pipeline). The handler automatically inherits the existing Run/Cancel/run history UI from `HandlerSection`.

The `EmbeddingHandler` also needs to be registered in the backend's handler list RPC so it appears in the admin UI. This follows the same pattern as `EnrichmentHandler` — registered in `ps-workers/src/main.rs` and listed via the `ListHandlers` RPC.

---

## psctl Extensions

```
psctl embed [--backfill]           # Trigger embedding cycle, or enqueue all eligible contributions
psctl embed status                 # Show embedding pipeline stats
psctl similar <contribution-id>    # Find similar contributions
psctl search "query text"          # Free-text similarity search
```

`psctl similar` and `psctl search` output a table of results with contribution ID, title, platform, type, and distance score. The `--json` flag outputs structured JSON.

---

## Cost Expectations

Gemini Embedding 2 pricing: **$0.20 per 1M tokens** for text input.

| Scenario | Volume | Est. Tokens | Daily Cost |
|---|---|---|---|
| Steady-state (new contributions/day) | ~500–2,000 | ~500K | ~$0.10 |
| Initial backfill (50K contributions) | 50,000 | ~50M | ~$10 one-time |
| SearchByText queries (on-the-fly) | ~20/day | ~10K | ~$0.002 |

At this pricing, embedding cost is negligible in steady-state. The backfill is a one-time cost that can be spread over multiple cycles.

Budget checking is included in the handler (shared daily budget cap with enrichment), but in practice embeddings will rarely approach the cap.

---

## Re-embedding Strategy

When the embedding model changes (e.g. moving from `gemini-embedding-2` to a future model):

1. **All embeddings must be regenerated** — you cannot mix vectors from different models in the same index (dimensions and semantic spaces differ)
2. Add a migration changing `vector(1024)` to the new dimension if needed
3. Truncate `reasoning.embeddings` (not drop — preserve the table structure)
4. Run `psctl embed --backfill` to re-enqueue all contributions
5. The `EmbeddingHandler` processes the backfill normally

The `model_name` column on each embedding row tracks which model produced it, so mixed-model states are detectable. The handler checks that all embeddings use the currently configured model; a mismatch triggers a warning in the admin UI.

This is infrequent (model changes happen at most yearly) and the cost is low (~$10 for a full re-embed).

---

## Testing Strategy

### Integration tests (Rust)

**pgvector:**
- Verify `vector` extension is enabled after migration
- Insert a vector, read it back, confirm values match
- Test IVFFlat index is used (via `EXPLAIN` in a non-CI test)

**Queue:**
- `bulk_enqueue_embeddings` — insert 10 entries, verify count
- `bulk_enqueue_embeddings` — re-enqueue same contribution_id, verify no duplicates (ON CONFLICT)
- `find_queued_for_embedding` — verify joins contribution data correctly
- `delete_embedded_queue_entries` — verify only removes entries with completed embeddings

**Similarity search:**
- Seed 20 embeddings with known vectors (manually constructed to have known cosine distances)
- `find_similar` — verify results sorted by distance, limit respected
- `find_similar` with `platform_filter` — verify filtering works
- `find_similar_to_contribution` — verify looks up embedding then searches
- `find_similar` with distance threshold — verify far-away vectors excluded

**Cross-platform linkage:**
- Seed a PR embedding and a Jira ticket embedding with identical vectors (distance 0)
- `find_similar` with `platform_filter = "jira"` — verify the Jira ticket is returned

**Pipeline:**
- Mock `EmbeddingModel` returning fixed vectors
- Call `process_embedding_batch()` — verify embeddings stored with correct IDs and model_name
- Verify skipped count for ineligible contributions (empty body, too short)

### Integration tests (gRPC)

Using `define_api_test!` macro:

- `FindSimilar` with a contribution that has an embedding — verify results
- `FindSimilar` with a contribution that has no embedding — verify empty response (not error)
- `SearchByText` — verify on-the-fly embedding + search returns results
- `GetEmbeddingStatus` — verify counts match actual table state

### Frontend tests (Vitest)

- `RelatedItems` — render with mocked similar items, verify list rendering
- `RelatedItems` — render with cross-platform match (distance < 0.2), verify prominent display
- `RelatedItems` — render with no similar items, verify panel hidden
- `EmbeddingPipelineStatus` — render with mock status, verify coverage bar
- `useEmbeddingSimilar` — verify query key structure and enabled flag

### Manual testing checklist

1. Run ingestion for a GitHub source with PRs → verify contributions enqueued in `reasoning.embedding_queue`
2. Wait for `EmbeddingHandler` cycle (or trigger via `psctl embed`) → verify embeddings created
3. Check Ingestion Status page → verify embedding pipeline stats appear
4. Navigate to a PR contribution detail → verify "Similar Contributions" panel shows results
5. Check if a cross-platform link appears for a PR that has a related Jira ticket
6. Click a similar item → verify navigation to that contribution's detail page
7. Run `psctl similar <id>` → verify CLI output
8. Run `psctl search "auth middleware"` → verify search results
9. Check AI Cost Dashboard → verify embedding cost logged (should be near $0)
10. Set daily budget cap to $0.01 → trigger embedding cycle → verify handler respects cap

---

## Implementation Order

### Step 1: Database migrations
- Enable pgvector extension
- Create `reasoning.embeddings` table with IVFFlat index
- Create `reasoning.embedding_queue` table
- Run `cargo sqlx prepare --workspace` (separate commit)

### Step 2: ReasoningRepo methods
- Add types (`EmbeddingRecord`, `QueuedEmbedding`, `SimilarContribution`, etc.)
- Add queue methods (`bulk_enqueue_embeddings`, `find_queued_for_embedding`, `delete_embedded_queue_entries`)
- Add embedding storage (`bulk_upsert_embeddings`)
- Add similarity queries (`find_similar`, `find_similar_to_contribution`)
- Add status query (`get_embedding_status`)
- Integration tests for all repo methods

### Step 3: ps-reasoning embeddings module
- `text.rs` — text extraction and normalisation
- `mod.rs` — `process_embedding_batch()`
- `pgvector.rs` — `PgVectorIndex` (Rig `VectorStoreIndex` impl)
- Unit tests with mocked `EmbeddingModel`

### Step 4: EmbeddingHandler (Restate)
- Handler implementation following enrichment handler pattern
- Wire up in `main.rs`
- Add downstream trigger from ingestion handlers

### Step 5: Queue population from enrichment + Jira ingestion
- Add `bulk_enqueue_embeddings()` call and `EmbeddingHandler` fire-and-forget trigger at end of `EnrichmentHandler::run_cycle()`
- Add Jira-only direct enqueue in Jira ingestion `store_batch()` (no enrichment dependency)
- Backfill command: `psctl embed --backfill`

### Step 6: Proto + gRPC API
- Add `FindSimilar`, `SearchByText`, `GetEmbeddingStatus` to proto
- `buf lint && buf generate`
- Implement in `ReasoningService`
- API integration tests

### Step 7: Frontend — similarity UI
- `use-embeddings.ts` hook
- `RelatedItems` component
- Contribution detail page integration
- `EmbeddingPipelineStatus` component on Ingestion Status page
- Component tests

### Step 8: psctl commands
- `psctl embed`, `psctl similar`, `psctl search`
- End-to-end manual testing

---

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| pgvector performance at scale (>500K vectors) | Low | Slow similarity queries | IVFFlat with `lists=100` handles up to ~1M vectors well. Increase lists or switch to HNSW if needed. Monitor query latency. |
| Gemini Embedding 2 API instability | Low | Embedding backlog | Retry with backoff (Rig handles this). Queue ensures no data loss — handler retries on next cycle. |
| MRL truncation quality loss at 1024 dims | Low | Lower similarity accuracy | Gemini Embedding 2 is designed for MRL. 1024 retains >95% of retrieval quality. Increase to 1536 if needed (requires migration + re-embed). |
| IVFFlat index needs data to cluster well | Medium | Poor recall on small datasets | Accept suboptimal recall initially. Index quality improves as data grows. Can `REINDEX` after initial backfill. |
| Noisy similarity results (false positives) | Medium | Users lose trust in "related items" | Distance threshold (0.5) filters weak matches. Cross-platform links require distance < 0.2. Tune thresholds based on real data. |
| sqlx + pgvector type compatibility | Low | Build issues | Use `pgvector` crate which provides `sqlx::Type` impl. Fall back to raw SQL casts if needed. |

---

## Key Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Dimensions | 1024 (MRL truncation from 3072) | Balance of quality and pgvector performance per Phase 3 plan |
| Index type | IVFFlat (lists=100) | Simpler than HNSW, sufficient for <1M vectors |
| Distance metric | Cosine (`<=>`) | Standard for text similarity |
| Distance threshold | 0.5 (general), 0.2 (cross-platform links) | Filter noise; cross-platform links need high confidence |
| Queue vs scan | Dedicated queue table | Follows enrichment queue pattern; cheap, ordered, deduped |
| Batch size | 500 items per cycle, 100 per API call | Large enough for throughput, small enough for memory |
| Trigger model | Demand-driven (post-ingestion + self-invoke) | Processes promptly, no idle polling |
| Text truncation | 32K chars (~8K tokens) | Gemini Embedding 2 input limit |
| Re-embedding | Truncate + re-enqueue | Infrequent, low cost, clean approach |
