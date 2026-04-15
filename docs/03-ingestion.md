# Data Ingestion Pipeline

All data ingestion runs as Restate handlers — never as synchronous gRPC RPCs. This ensures durability, cancellation, progress tracking, and journal visibility.

## Source Trait

Platform-specific ingestion logic is abstracted behind the `Source` trait (`ps-core/src/ingestion.rs`):

```rust
pub trait Source: Send + Sync {
    fn name(&self) -> &'static str;
    async fn plan(&self, ctx: &IngestionContext) -> Result<IngestionPlan, Error>;
    async fn fetch_batch(&self, ctx: &IngestionContext, cursor: &str) -> Result<FetchResult, Error>;
    async fn store_batch(&self, ctx: &IngestionContext, items: &[ContributionInput]) -> Result<usize, Error>;
    async fn advance_watermark(&self, ctx: &IngestionContext, new_watermark: &str, items: i32) -> Result<(), Error>;
    fn initial_cursor(&self, ctx: &IngestionContext, plan: &IngestionPlan) -> String;
    fn watermark_field(&self) -> &'static str;
}
```

Sources are registered in `registry.rs` and instantiated by `create_source(platform)`.

## Handler Architecture

### Handler Types

| Handler | Restate Type | Key | Purpose |
| --- | --- | --- | --- |
| `GithubIngestionHandler` | Object | source name | GitHub PR/review ingestion |
| `JiraIngestionHandler` | Object | source name | Jira issue ingestion |
| `DiscourseIngestionHandler` | Object | source name | Discourse topic ingestion |
| `IngestionChunkService` | Service | — | Processes a single chunk (batch-limited fetch-store loop) |
| `GithubTeamSyncHandler` | Object | source name | GitHub team/member/repo sync |
| `MetricsComputeHandler` | Service | — | Metric snapshot computation |
| `EnrichmentHandler` | Service | — | AI enrichment pipeline |
| `IdentityResolutionHandler` | Service | — | Discourse identity resolution |
| `ModelCatalogueHandler` | Service | — | AI model catalogue refresh |
| `AgenticQueryHandler` | Object | conversation_id | Agent pod lifecycle |
| `QueryWatchdogHandler` | Object | `singleton` | Reset stuck conversations |

**Objects** are keyed (per-source or per-conversation). **Services** are singletons.

### SharedState

All handlers receive `SharedState` (constructed once in `main.rs`, cloned into each handler):

```rust
pub struct SharedState {
    pub repos: Repos,
    pub secret_key: Zeroizing<[u8; 32]>,
    pub http_client: reqwest::Client,
}
```

Handlers never touch `PgPool` directly — always go through `state.repos`.

## Chunked Ingestion Flow

Ingestion uses a two-level architecture to keep Restate journals small while supporting long-running ingestion runs. All handlers call `execute_ingestion_chunked()` from `features/ingestion/lib/`.

### Terminology

- **Batch** — a single `fetch_batch()` / `store_batch()` cycle (one API page of data)
- **Chunk** — up to N batches (currently 50) processed in a single Restate service invocation

### Coordinator (`execute_ingestion_chunked` in `orchestration.rs`)

The coordinator runs in the per-source Object handler. Its journal stays minimal (~1 entry per chunk):

1. **Create source adapter** — `registry::create_source(source_type)`
2. **Create run record** (journaled) — `Uuid::now_v7()` inside `ctx.run()` for idempotent retries
3. **Decrypt secrets** (outside `ctx.run()`) — plaintext must never be journaled
4. **Build IngestionContext** — combine state + config + decrypted secrets
5. **Plan** (not journaled) — determine repos/projects/categories to fetch, load watermark
6. **Override watermark** if backfilling
7. **Dispatch chunks** — sequential loop sending `ChunkRequest`s to `IngestionChunkService`, accumulating `items_offset` and cursor across chunks until `ChunkResult.is_complete == true`
8. **Finalise run** — three outcomes based on failed items
9. **Trigger downstream** — fire-and-forget to MetricsComputeHandler, etc.

### Chunk Service (`IngestionChunkService` in `chunk.rs`)

Each chunk runs as a separate Restate service invocation with its own isolated journal:

1. Load source config (journaled)
2. Decrypt secrets (outside `ctx.run()`)
3. Build `IngestionContext`
4. Run `chunk_fetch_store_loop()` — up to `max_batches` iterations of fetch→store→advance
5. Return `ChunkResult { items_stored, cursor, is_complete }`

The `ChunkRequest` carries `source_type`, `cursor`, `run_id`, `max_batches`, and `items_offset` (global item count from previous chunks for progress display).

## Journaling Rules

| What | Inside `ctx.run()`? | Why |
| --- | --- | --- |
| DB writes (store, watermark, run lifecycle) | Yes | Must be idempotent on replay |
| External API calls (GitHub, Jira, AI) | No | Responses are large; re-executing is safe (upserts) |
| Secret decryption | No | Journal persists results — plaintext must never be inside |
| Progress updates | No | Best-effort, doesn't affect replay correctness |

All `ctx.run()` closures must have `.name("step_name")` labels for journal debugging.

Use `journaled!` / `journaled_value!` macros from `infra/run_lifecycle.rs` for ad-hoc journaled calls. They handle the double-clone dance required by Restate's `Fn` closures. Use `terminal_err("context")` for error mapping.

## Cursor and Watermark Design

Each source defines its own cursor struct (serialised to JSON). Cursors are opaque to the orchestration layer.

- **GitHub**: Multi-phase (TeamRepos -> MemberSearch), tracks repo_index, graphql_cursor, max_updated_at, failed_items
- **Jira**: Iterates projects, tracks project_index, next_page_token, max_updated_at, failed_items
- **Discourse**: Iterates categories, tracks category_index, page, max_bumped_at

**Incremental watermark advancement:** after each successful `store_batch()`, the watermark advances immediately. On retry, only the last incomplete batch needs re-fetching.

### Finalisation Outcomes

| Outcome | Watermark | Run status |
| --- | --- | --- |
| No failures, items > 0 | Advanced (final) | `completed` |
| All items failed | Not advanced | `failed` |
| Partial failure | Not advanced | `completed_with_warnings` |

## Scheduling

Recurring ingestion uses Restate's durable delayed self-invocation (`ctx.object_client().method().send_with_delay()`), not external cron. Cron expressions stored per-source, evaluated in UTC.

Frontend dispatch uses `TriggerHandler` RPC (fire-and-forget to Restate). `trigger_handler()` guards against duplicate runs by checking for active runs before dispatching.

## Transient Error Retry

All external API calls inside `fetch_batch()` are wrapped with `retry_transient()` from `ps-workers/src/retry.rs`. Retries up to 3 times with exponential backoff (1s, 2s, 4s) for transient errors (5xx, timeouts, connection resets).

- HTTP clients must use `Error::HttpStatus { status, message }` so `is_transient()` can inspect the status code
- Rate limits (429) are handled separately via `Error::RateLimit` and durable sleep, not retry
- All retry sites are inside `fetch_batch()` which runs outside `ctx.run()` — never introduce `ctx.run()` inside a retry loop

## GitHub Two-Phase Ingestion

1. **Team repos phase** — fetch PRs/reviews for repos discovered via team sync data (GraphQL for inline reviews)
2. **Member search phase** — discover cross-repo contributions by team members via GraphQL search API

GraphQL over REST for N+1-prone queries. REST for infrequent operations like team sync.

## Adding a New Source

1. **Source module** — `crates/ps-workers/src/features/ingestion/<platform>/`. Implement `Source` trait. Define cursor struct.
2. **Registry** — add `Platform::NewPlatform => Some(Box::new(NewPlatformSource))` in `registry.rs`
3. **Handler** — define `IngestionSpec`, implement `ProgressTracker`, create `#[restate_sdk::object]` with `run_ingestion()` and `backfill()`. Call `execute_ingestion_chunked()`.
4. **Export** — add `pub mod` in handlers `mod.rs`
5. **Wire up** — instantiate in `main.rs`, bind to Restate endpoint
6. **Platform enum** — add variant to `Platform` in `ps-core/src/models/enums.rs`

## Journal Compatibility

Changing the sequence of `ctx.run()` calls in a handler breaks in-flight invocations (Restate replays positionally). After refactoring handler code:

1. Cancel all in-flight invocations for affected handlers
2. If needed, wipe Restate's journal storage and restart
3. Re-register the deployment: `restate deployments register http://ps-workers:9081/ --force --yes`
