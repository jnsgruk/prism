---
paths:
  - "crates/ps-workers/src/features/ingestion/**"
  - "crates/ps-core/src/ingestion.rs"
---

# Ingestion Pipeline Rules

The three ingestion handlers (GitHub, Jira, Discourse) share unified orchestration via `execute_ingestion_chunked()` in `features/ingestion/lib/`. Platform-specific logic is abstracted behind the `Source` trait.

## Source Trait (`ps-core/src/ingestion.rs`)

Sources implement: `plan()`, `fetch_batch()`, `store_batch()`, `advance_watermark()`, `initial_cursor()`, `watermark_field()`. Registered in `registry.rs`, instantiated by `create_source(platform)`.

## Chunked Ingestion Flow

Ingestion uses a two-level architecture: a **coordinator** dispatches sequential chunks to the **chunk service**, each processed as a separate Restate invocation with its own small journal. This keeps the coordinator's journal minimal (~1 entry per chunk). A **batch** is one `fetch_batch()`/`store_batch()` cycle; a **chunk** is up to N batches (currently 50 for all sources).

### Coordinator (`execute_ingestion_chunked`)

1. Create source adapter via registry
2. Create run record (journaled — `Uuid::now_v7()` inside `ctx.run()` for idempotent retries)
3. Decrypt secrets (**outside** `ctx.run()` — plaintext must never be journaled)
4. Build `IngestionContext` with pre-decrypted secrets
5. Plan (not journaled)
6. Override watermark if backfilling
7. Dispatch chunks — sequential loop sending `ChunkRequest`s to `IngestionChunkService`, accumulating `items_offset` and cursor until `ChunkResult.is_complete == true`
8. Finalise run
9. Trigger downstream (fire-and-forget to MetricsComputeHandler, etc.)

### Chunk Service (`IngestionChunkService`)

Each chunk: loads config, decrypts secrets, builds `IngestionContext`, runs `chunk_fetch_store_loop()` for up to `max_batches` iterations, returns `ChunkResult { items_stored, cursor, is_complete }`.

## IngestionSpec

Each handler defines a static spec:

```rust
const GITHUB_SPEC: IngestionSpec = IngestionSpec {
    handler_name: "GithubIngestionHandler",
    token_key: Some("api_token"),
    token_required: true,
    email_key: None,
    api_username_key: None,
    item_noun: "repo",
};
```

## chunk_fetch_store_loop() Rules

1. `fetch_batch()` — **NOT journaled** (external API, idempotent on replay). Wrapped in `catch_unwind()`.
2. `store_batch()` — **journaled** inside `ctx.run()`
3. `advance_watermark()` — **journaled** inside `ctx.run()` (incremental, after each batch)
4. Progress updates — **NOT journaled** (best-effort)

**Incremental watermark advancement**: watermark advances after each successful batch, not at the end. On retry, only the last incomplete batch needs re-fetching.

## Transient Error Retry

All external API calls in `fetch_batch()` must be wrapped with `retry_transient()` from `retry.rs`:
- Retries 3 times with exponential backoff (1s, 2s, 4s) for 5xx, timeouts, connection resets
- HTTP clients must use `Error::HttpStatus { status, message }` (not `Error::Internal`) so `is_transient()` can inspect status
- Rate limits (429) are **not transient** — handled via `Error::RateLimit` and durable sleep
- **Never** introduce `ctx.run()` inside a retry loop

## Cursor Design

Each source has its own cursor struct (serialised to JSON), opaque to the orchestration layer. Use `#[serde(default)]` on cursor fields for forward compatibility.

## Finalisation

| Outcome | Watermark | Run status |
| --- | --- | --- |
| No failures, items > 0 | Advanced | `completed` |
| All items failed | Not advanced | `failed` |
| Partial failure | Not advanced | `completed_with_warnings` |

## GitHub Two-Phase Ingestion

1. **Team repos phase** — PRs/reviews for repos from team sync (GraphQL for inline reviews)
2. **Member search phase** — cross-repo contributions via GraphQL search API

GraphQL over REST for N+1-prone queries. REST for infrequent operations (team sync).

## Scheduling

Recurring ingestion uses Restate's durable delayed self-invocation (`ctx.object_client().method().send_with_delay()`). Cron expressions stored per-source, evaluated in UTC.
