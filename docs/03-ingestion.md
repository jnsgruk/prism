# Data Ingestion Pipeline

All data ingestion runs as Restate handlers — never as synchronous gRPC RPCs. This ensures durability, cancellation, progress tracking, and journal visibility.

## Source Trait

Platform-specific ingestion logic is abstracted behind the `Source` trait (`ps-core/src/ingestion.rs`):

```rust
pub trait Source: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports_person_backfill(&self) -> bool;
    async fn plan(&self, ctx: &IngestionContext) -> Result<IngestionPlan, Error>;
    async fn fetch_batch(&self, ctx: &IngestionContext, cursor: &str) -> Result<FetchResult, Error>;
    async fn store_batch(&self, ctx: &IngestionContext, items: &[ContributionInput]) -> Result<usize, Error>;
    async fn advance_watermark(&self, ctx: &IngestionContext, new_watermark: &str, items: i32) -> Result<(), Error>;
    fn initial_cursor(&self, ctx: &IngestionContext, plan: &IngestionPlan) -> String;
    fn watermark_field(&self) -> WatermarkField;
}
```

Sources are registered in `registry.rs` and instantiated by `create_source(platform)`.

## Durable Scope Contracts

`ps-core::ingestion` owns the serializable domain contracts, without protobuf or
Restate dependencies. `PipelineScope` defaults to `All`; `Person` contains a
typed `PersonId`. A `PipelineRequest` freezes selected source IDs and labels,
the optional backfill date, the run boundary, and `ProcessingScope` at admission.
The API accepts saved person/source UUIDs, never replacement usernames. Its
optional `PersonBackfillScope` requires a person, at least one selected source,
and the existing `since_date` field in `YYYY-MM-DD` format. An absent scope
retains All behavior. A caller-generated `submission_id` supports retrying an
ambiguous admission without creating another workflow.

Each `SelectedSource` has a typed `SourceId`, saved name, exact `Platform`, and
an optional `IdentitySnapshot`. Source ID identifies configuration rather than
platform; two sources on one platform remain distinct. Person runs require a
snapshot of the saved identity UUID, person UUID, canonical username, platform,
and platform account ID where necessary. Discourse platforms include their
instance suffix, so an Ubuntu identity cannot match a Snapcraft source. Jira
Cloud requires its opaque account ID; Server/Data Center is unsupported.

`SourceRunContext` carries the pipeline ID and frozen source, scope, date,
boundary, and processing values through handler, chunk, and `IngestionContext`
boundaries. Legacy adapter contexts have no request. Legacy workflow `run`
arguments remain `Option<String>` and legacy object entrypoints remain in place;
new requests use `ScopedIngestionPipelineWorkflow.run` and scoped object
methods. Optional chunk context defaults preserve existing serialized All
payloads and journals. Settings and encrypted credentials are looked up by exact
source UUID; the UUID/platform binding must still match the frozen selection.
Credentials are decrypted outside journalled side effects.

Discovery timestamps and eligibility timestamps serve different purposes.
Discovery must find parents updated since the lower bound, including old PRs
with newer qualifying reviews. Contribution eligibility uses the inclusive UTC
interval from midnight on `since_date` through frozen `run_started_at`: GitHub
reviews use `submittedAt`; Jira uses updated-since discovery with current
assignee/account-ID ownership; Discourse topics, posts, and likes use their own
action times. A parent creation date cannot exclude an otherwise eligible event.

GitHub, Jira Cloud, and instance-qualified Discourse adapters support person
backfills through scoped coordinator/chunk entrypoints. The complete Person
pipeline admission remains disabled until historical processing and ongoing
tracking are delivered (#28–#30); adapter support alone cannot claim that product
release is complete. `ProcessingScope` must match ingestion scope and
person; a person request cannot silently trigger whole-organisation processing.
Pipeline status supports explicit pipeline/person filtering, and history carries
saved scope/source labels independent of later configuration changes.

### Admission, recovery, and cancellation

Admission stores the request and caller before dispatch and admits only one
pipeline at a time. The server retries leased dispatch intents using the same
workflow UUID, so a lost response or server restart preserves the original
request. Missing or malformed invocation acknowledgements remain uncertain and
are retried. Children register their own invocation IDs before work, including
chunks and detached processing continuations; parent registration also records
their exact ownership.

Recovery requires a definitively terminal root before repairing an unfinished
pipeline. For a saved cancellation of a legacy workflow, it first kills the
exact root invocation, including roots waiting on rate-limit sleeps. Legacy
worker finalization cannot release that cancelled admission; recovery confirms
the root is terminal before proceeding. It stops further registration and drains
exact registered descendants, following Restate's `invoked_by_id` ancestry to
find children missing from the registry, including legacy workflow children.
It holds admission while any
descendant is still active or its status is uncertain. Recovery never selects
work by source name, handler name, or start time. The pipeline and its running
records finish atomically under the admission lock: a saved user cancellation
becomes `cancelled`; an unexpectedly stopped root becomes `failed`. Partial
item counts and committed contributions remain available for a later rerun.
An internal stop flag does not become a user cancellation during retries.

### Runtime verification

On 2026-10-01, the local Tilt cluster exercised the scoped All workflow against
an isolated Wiremock Jira Cloud source with encrypted fake credentials. Fifty
pages completed the first chunk and a depleted-rate-limit response suspended the
second chunk in durable sleep. After replacing the workers pod, the admitted
snapshot and original journal prefix remained identical; the same chunk resumed
and entered a second sleep. Targeted pipeline cancellation terminated its owned
coordinator/chunks while an unrelated scheduled fixture invocation remained
scheduled. A second run verified that the cancelled ingestion record retained
its 50 collected items and reported `cancelled`. The temporary source,
contributions, run records and mock service
were removed after verification. This verifies admission, plumbing and ownership.
The targeted adapter and atomic storage fixtures below run without live provider
credentials; complete Person launch, historical processing and ongoing-ingestion
acceptance remain the separate #28–#30 release gate.

### Targeted adapters and storage

The person adapter path needs no team membership or source-wide sweep. Each
cursor freezes the selected account, source configuration bounds, and inclusive
UTC event interval. These are live upstream views: credentials limit visibility,
search indexes can lag, and deleted/private history may be unavailable. Cursor
`coverage` and `failed_items` are retained in run metadata and progress, visible
in person progress and completed run details. Failed targets make the scoped
coordinator fail or finish with warnings, never report an unqualified success.
GitHub and Jira reject endpoint changes before requesting another page. GitHub
also binds review continuations to the exact admitted source, identity and date
snapshot; Discourse continues using its frozen instance endpoint.

- **GitHub:** search each configured organisation separately for `author:` and
  `reviewed-by:`. Partition immutable PR creation timestamps at GitHub's 1,000
  search-result cap; subdivision preserves both endpoints and natural keys
  deduplicate overlapping search phases. Review discovery includes old PRs, with
  no mutable updated-time upper bound. An irreducibly saturated second is recorded
  incomplete. Excluded and
  archived repository policies apply to both search phases. Review queues hold
  at most one search page; each fetch processes one bounded review page, including
  page 2 and beyond. Only the actual selected author/reviewer and eligible event
  times emit contributions. Review totals use the API count; enrichment marks
  inline-comment truncation explicitly. Global ingestion also pages all reviews.
- **Jira Cloud:** every request combines configured projects with the saved
  opaque current-assignee account ID. Empty projects retain the all-accessible
  project meaning, restricted to that account. Quotes and backslashes are escaped.
  The authenticated API user's `/myself` timezone is frozen in the cursor;
  discovery widens the local lower bound by a day, then exact UTC issue-update
  eligibility filters results. Returned assignments are checked again. Ordering
  is `updated ASC, key ASC`; missing, repeated, or cyclic nonterminal tokens fail.
  Server/Data Center mode is rejected before targeted dispatch.
- **Discourse:** query instance-bound `/user_actions.json` for types 4/5, plus
  type 1 when `fetch_likes` is enabled. Offset pages use a ten-event overlap and
  bounded preceding-page keys to detect mutation/nonadvancement. Specific post
  details retrieve replies absent from a topic's initial stream; topic details
  provide categories, `min_posts`, tags and context. Type 4 may omit `post_id`, in
  which case only the topic's initial post is used. Context authors and like
  recipients never become selected-person contributions. The real serializer
  omits numeric action IDs; a composite type/topic/post/actor/time key records
  auditable evidence without inventing an upstream ID.

Both watermark paths and adapter watermark methods explicitly honor All/Person
policy. Person runs never create/update global coverage checkpoints, counters or
last-success timestamps. Run cursors/progress hold their checkpoint instead.
Empty filtered pages and discovery transitions count toward chunk limits.

Every person adapter delegates to one atomic repository boundary. It filters
actor/platform/instance/event eligibility and batch duplicate natural keys,
locks the admitted pipeline/run, active person, saved identity and source, and
refuses attribution conflicts. The admitted snapshot must still match exactly.
Contribution advisory locks serialize normal/scoped writes of the same keys.
Contribution IDs, queues and `activity.contribution_changes` provenance commit
together; database/queue failures roll back and are retryable in Restate.
Scoped runs never invoke the global Discourse relinking sweep. Changed enrichment
context has a content fingerprint so deferred diffs cannot be silently discarded.

Discourse likes retain `like-<post_id>-<lowercase_username>`. Only matching type-1
user-action evidence authorizes an event-time correction. Changes record previous
and current attribution, timestamps and metric inputs, source/pipeline/run IDs,
and all affected UTC weeks/months/quarters. Ordinary ingestion preserves corrected
time and its evidence, and older re-like pages cannot undo newer actions. #28
consumes these manifests for actual historical recomputation; authored-topic
received-like metrics also need its topic/post aggregation acceptance tests.

Person API reads occur outside `ctx.run()`. `checkpoint_person_fetch` journals
bounded cursor/rate-limit decisions and a page fingerprint, followed by atomic
journaled storage. Restart may repeat safe API reads. Completed pages consume
their recorded store result and cursor even if the provider response changes or
fails during replay. A changed unfinished page is rejected inside its journaled
store step, so new writes cannot silently use different history. Legacy All
fetch-result journals remain compatible with their existing wire shape. Drain
affected global chunks/coordinators before deploying changed review pagination or
chunk-counting behavior; do not wipe unrelated Restate journals. No active
invocations were present in the local cluster during this adapter rollout.

## Admission, Delivery and Cancellation

Admin launches reserve an `activity.pipelines` row before submitting work to
Restate. A partial unique index admits one `pending`, `running`, or `cancelling`
pipeline across All and Person scopes. The response contains the reserved UUID
immediately; harvesting runs asynchronously. The optional `submission_id` UUID
is also the workflow key. Retry it with the same caller and intent to recover
the original ID, including after the response was lost. Both launch UIs retain
this key while retrying a failed submission.

The server runs a short dispatch recovery poll with a database lease. This poll
only delivers persisted intents to Restate; collection and processing remain
durable Restate handlers. Restarting the server expires the lease and retries
the original UUID. A definitive rejection on the first delivery releases the
reservation as failed. Transport errors, malformed acknowledgements, and later
rejections after an uncertain attempt retain admission and retry the same
workflow. They never create a replacement workflow.

Versioned coordinators, chunks, processing handlers, and detached continuations
register exact invocation IDs and create runs with explicit pipeline ownership.
Cancellation sets persistent intent before requesting cancellation of owned
work. Registration locks the owning pipeline, so work racing with cancellation
must stop. The UI shows `cancelling` until owned work has stopped and terminal
records have been reconciled. Completed partial writes remain available for a
deduplicated rerun. Scheduled runs and unrelated invocations are not linked by
start time or included in cancellation.

Apply migrations before deploying the server and workers, then register the
updated worker endpoint with Restate. Keep the legacy workflow and handler
entrypoints registered until their invocations finish; their arguments and
journal sequences remain unchanged. No blanket journal wipe is required.
Migration admission fails explicitly if historical data contains multiple
active pipelines; inspect and resolve those records before rollout.

Person launch preflight checks active saved people, actual non-future dates,
unique enabled selected sources, exact saved account bindings, and Jira Cloud
mode. The capability reported by `GetStatus` stays disabled until #30 passes.
Saved person details expose a separate Backfill activity dialog showing source
eligibility, the release gate, exact pipeline progress, and scoped history.
Person/account resolution uses a consistent database snapshot. Production
enablement requires the adapter/storage/watermark/metrics integration tests in
stage #12.

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

External API calls inside `fetch_batch()` use `retry_transient()` from `ps-workers/src/infra/retry.rs`. It allows five retries after the initial attempt, with exponential delays of 8, 16, 32, 64 and 120 seconds for transient errors (5xx, timeouts, connection resets).

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
