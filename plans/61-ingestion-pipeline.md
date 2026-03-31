# Ingestion Pipeline Orchestration

## Problem

Running the full data pipeline requires multiple manual steps:

1. Click "Run All" (or trigger each ingestion handler individually)
2. Wait for all ingestion handlers to complete
3. Click "Run" on the enrichment handler
4. Enrichment auto-triggers embedding (but this is invisible and implicit)

This is a pipeline, but it's presented as a bag of independent buttons. Users have to watch status dots and mentally track stage dependencies. There's no single affordance for "run everything in order" and no visual representation of the data flow.

### Current trigger chain

```
Manual ─→ GithubIngestionHandler ─→ MetricsCompute (fire-and-forget)
Manual ─→ JiraIngestionHandler ─→ MetricsCompute (fire-and-forget)
Manual ─→ DiscourseIngestionHandler ─→ MetricsCompute + IdentityResolution (fire-and-forget)

Manual ─→ EnrichmentHandler ─→ InsightsHandler + EmbeddingHandler (fire-and-forget, auto)
```

Each `─→` is a `.send()` (fire-and-forget). No handler awaits another's completion. The enrichment-to-embedding trigger is hardcoded and invisible to the user.

## Design

### Pipeline model

The pipeline is a DAG with a linear spine and a fork after ingestion:

```
                                          ┌─→ Metrics → Enrichment → Embedding → Insights ─┐
TeamSync ─→ Ingestion (fan-out) ──────────┤                                                 ├─→ Done
                                          └─→ IdentityResolution ──────────────────────────-┘
```

Expanded with handler detail:

```
┌───────────┐   ┌──────────────────┐   ┌─────────┐   ┌────────────┐   ┌───────────┐   ┌──────────┐
│ Team Sync │   │    Ingestion     │   │ Metrics │   │ Enrichment │   │ Embedding │   │ Insights │
│           │   │ ┌──────────────┐ │   │         │   │            │   │           │   │          │
│  GitHub   │   │ │ GitHub       │ │   │ compute │   │            │   │           │   │ compute  │
│  team     │──→│ │ Jira         │ ├──→│ current ├──→│  run_cycle ├──→│ run_cycle ├──→│ current  │
│  sync     │   │ │ Discourse    │ │   │ periods │   │            │   │           │   │ periods  │
│           │   │ └──────────────┘ │   │         │   │            │   │           │   │          │
└───────────┘   └──────────────────┘   └─────────┘   └────────────┘   └───────────┘   └──────────┘
  sequential          fan-out           ──────── concurrent with IdentityResolution ────────
  (.call())       (concurrent .call())         (.call() each, sequential within branch)

                                       ┌──────────────────────┐
                          also after ──→│ Identity Resolution  │
                          ingestion     │  resolve_identities  │
                                       └──────────────────────┘
```

**Design principles:**

- **No handler calls another handler.** The pipeline owns all orchestration. All existing `.send()` triggers between handlers are removed. Individual handlers become pure units of work.
- **Team sync runs first** — it discovers GitHub orgs, teams, members, and repos. The ingestion stage uses this data to know which repos to fetch. It's a pre-condition, not a downstream step. Skipped if no GitHub sources are configured.
- **Fork after ingestion** — two independent branches run concurrently:
  - **Main branch:** Metrics → Enrichment → Embedding → Insights (sequential, each depends on the previous)
  - **Identity resolution branch:** resolves Discourse platform identities. Independent of metrics/enrichment. Skipped if no Discourse sources are configured.
- **Pipeline completes** when both branches finish.
- Running MetricsCompute once after all ingestion (rather than per-source) avoids redundant recomputation. Similarly, Insights runs once after embedding rather than as an implicit side-effect of enrichment.

### Key SDK discoveries

**`.call()` (request-response):** Restate SDK 0.9 supports `.call()` in addition to `.send()` (fire-and-forget). This project has only used `.send()` so far, but `.call()` returns a `CallFuture` that implements `Future` and resolves when the target handler completes. This enables direct fan-out/fan-in without polling or awakeables.

```rust
// Fan-out: start concurrent calls
let github = ctx.object_client::<GithubIngestionHandlerClient>("Github")
    .run_ingestion()
    .call();
let jira = ctx.object_client::<JiraIngestionHandlerClient>("Jira")
    .run_ingestion()
    .call();

// Fan-in: await all
let (gh_result, jira_result) = tokio::join!(github, jira);
```

Each `.call()` is a separate journal entry. On replay, Restate returns the journaled result without re-executing the target handler.

**`#[restate_sdk::workflow]`:** The SDK supports a workflow handler type designed exactly for multi-step orchestration. A workflow's `run()` handler executes exactly once per workflow ID. `#[shared]` handlers can query state or signal the workflow while `run()` is executing. Workflows also support K/V state (mutable only from `run()`, readable from shared handlers) and durable promises for cross-handler signaling. This is the right abstraction for a pipeline.

### Backend: IngestionPipelineWorkflow

New Restate **workflow** handler. Each pipeline run gets a unique workflow ID (the pipeline UUID). The `run()` handler executes exactly once per ID — Restate guarantees this. `#[shared]` handlers let the frontend query live status and signal cancellation while `run()` is in progress.

```rust
#[restate_sdk::workflow]
pub trait IngestionPipelineWorkflow {
    /// Run the full pipeline: team sync → ingestion → [metrics → enrichment → embedding → insights] + [identity resolution].
    /// Executes exactly once per workflow ID.
    async fn run() -> Result<Json<PipelineResult>, TerminalError>;

    /// Query current pipeline progress (callable while run() is executing).
    #[shared]
    async fn get_status() -> Result<Json<PipelineStatus>, TerminalError>;

    /// Signal the pipeline to cancel after the current stage completes.
    #[shared]
    async fn cancel() -> Result<(), TerminalError>;
}
```

**Context types:** `run()` receives `WorkflowContext<'_>` (can read/write K/V state, create promises, call other handlers). `get_status()` and `cancel()` receive `SharedWorkflowContext<'_>` (can read K/V state and resolve promises, but not write state or call handlers).

#### K/V state

The workflow uses Restate K/V state to expose live progress to `get_status()`:

```rust
// In run(), after each stage:
ctx.set("current_stage", "enrichment");
ctx.set("stages", Json(stages_json));

// In get_status():
let stage = ctx.get::<String>("current_stage").await?;
let stages = ctx.get::<Json<StagesProgress>>("stages").await?;
```

The `run()` handler also writes to the DB at each stage transition for persistence beyond workflow retention. The frontend polls the DB; the `get_status()` shared handler exists for programmatic consumers (other handlers, scripts) that want real-time status without a DB round-trip.

#### run() flow

```
1. Create pipeline record in DB (journaled, Uuid::now_v7())
2. Load all enabled sources (journaled)
3. Set K/V state: initial stages structure with all stages "pending"

4. ── STAGE: Team Sync (conditional) ──
   Skip if no GitHub sources are configured
   Update K/V state + DB: current_stage = "team_sync" (journaled)
   Fan-out: .call() GithubTeamSyncHandler for each GitHub source concurrently
   Fan-in: tokio::join! all calls
   Record results, check cancellation promise
   → Non-fatal: team sync failure logs a warning but does not abort the pipeline

5. ── STAGE: Ingestion ──
   Update K/V state + DB: current_stage = "ingestion" (journaled)
   Fan-out: start .call() for each source handler concurrently
   Fan-in: tokio::join! all calls
   Record per-handler results in K/V state + DB (journaled)
   Check cancellation promise (non-blocking)
   → Continue even if some handlers fail (partial data is useful)
   → Abort only if ALL handlers fail

6. ── FORK: two concurrent branches after ingestion ──

   BRANCH A (main processing chain):
   6a. Metrics
       .call() MetricsComputeHandler::compute_current_periods()
       Record result, check cancellation
   6b. Enrichment
       .call() EnrichmentHandler::run_cycle()
       Record result, check cancellation
   6c. Embedding
       .call() EmbeddingHandler::run_cycle()
       Record result, check cancellation
   6d. Insights
       .call() InsightsHandler::compute_current_periods()
       Record result

   BRANCH B (identity resolution — conditional):
   Skip if no Discourse sources are configured
   6e. .call() IdentityResolutionHandler::resolve_identities()
       Record result

   Implementation: tokio::join!(branch_a_future, branch_b_future)

7. Finalise pipeline record in DB (journaled)
   Status: completed | completed_with_warnings | failed
```

#### Cancellation via durable promise

The `cancel()` shared handler resolves a durable promise that `run()` checks between stages:

```rust
// In cancel() (shared handler):
ctx.resolve_promise::<()>("cancel", ());

// In run(), between stages:
let cancel = ctx.promise::<()>("cancel");
restate_sdk::select! {
    _ = cancel => {
        // Mark pipeline as cancelled in DB, return early
    }
    // ... or proceed to next stage
}
```

This is cooperative cancellation — the current stage runs to completion, then the pipeline stops before the next stage. For immediate cancellation of a long-running child handler (e.g., ingestion mid-flight), the `TriggerPipeline` RPC can also cancel the workflow invocation via Restate's admin API, which cascades to all pending `.call()` futures.

#### Source-to-handler mapping

The pipeline needs to map source configs to the correct handler and Restate key. This mapping already exists implicitly in the `TriggerHandler` RPC logic and `HANDLER_DEFS`. We extract it into a shared function:

```rust
fn handler_for_source(source: &SourceConfig) -> Option<(HandlerName, RestateKey, Method)> {
    match source.platform {
        Platform::Github => Some(("GithubIngestionHandler", source.source_type.clone(), "run_ingestion")),
        Platform::Jira => Some(("JiraIngestionHandler", source.source_type.clone(), "run_ingestion")),
        Platform::Discourse => Some(("DiscourseIngestionHandler", source.source_type.clone(), "run_ingestion")),
        _ => None,
    }
}
```

However, since we're using `.call()` from within the workflow (not HTTP), we call the typed client directly. The platform → client mapping is a match expression in the workflow's `run()` handler, dispatching to the correct `object_client::<XHandlerClient>(key)`.

### Auto-trigger removal

**No handler calls another handler.** The pipeline is the sole orchestrator. All existing fire-and-forget `.send()` triggers between handlers are removed:

1. **Ingestion → MetricsCompute** — remove `.send()` to MetricsComputeHandler from all three ingestion handlers (GitHub, Jira, Discourse)
2. **Enrichment → EmbeddingHandler** — remove `.send()` to EmbeddingHandler from enrichment completion path
3. **Enrichment → InsightsHandler** — remove `.send()` to InsightsHandler from enrichment completion path
4. **Discourse → IdentityResolution** — remove `.send()` to IdentityResolutionHandler from Discourse handler

After this change, every handler is a pure unit of work with no knowledge of what runs before or after it. All orchestration lives in the pipeline workflow.

Users who trigger individual handlers manually (outside the pipeline) won't get automatic downstream execution. This is fine — individual triggers remain available for debugging and one-off use, and the pipeline is the primary way to run the full chain.

### Database schema

New table in the `activity` schema:

```sql
CREATE TABLE activity.pipelines (
    id                    UUID PRIMARY KEY,
    status                TEXT NOT NULL DEFAULT 'running',
    current_stage         TEXT,
    started_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at          TIMESTAMPTZ,
    stages                JSONB NOT NULL DEFAULT '{}',
    current_invocation_id TEXT,
    error                 TEXT
);

CREATE INDEX idx_pipelines_status ON activity.pipelines (status, started_at DESC);
```

The `stages` JSONB stores structured progress, updated after each stage:

```json
{
  "team_sync": {
    "status": "completed",
    "started_at": "2026-03-31T09:59:50Z",
    "completed_at": "2026-03-31T10:00:00Z",
    "handlers": [
      { "name": "Github Team Sync", "status": "completed" }
    ]
  },
  "ingestion": {
    "status": "completed",
    "started_at": "2026-03-31T10:00:00Z",
    "completed_at": "2026-03-31T10:05:00Z",
    "handlers": [
      { "name": "Github", "status": "completed", "items": 142 },
      { "name": "Jira", "status": "failed", "error": "auth token expired" },
      { "name": "Discourse: Ubuntu", "status": "completed", "items": 87 }
    ]
  },
  "metrics": {
    "status": "completed",
    "started_at": "2026-03-31T10:05:01Z",
    "completed_at": "2026-03-31T10:05:08Z",
    "branch": "main",
    "handlers": [
      { "name": "Metrics", "status": "completed" }
    ]
  },
  "enrichment": {
    "status": "running",
    "started_at": "2026-03-31T10:05:09Z",
    "branch": "main",
    "handlers": [
      { "name": "Enrichment", "status": "running" }
    ]
  },
  "embedding": {
    "status": "pending",
    "branch": "main",
    "handlers": [
      { "name": "Embedding", "status": "pending" }
    ]
  },
  "insights": {
    "status": "pending",
    "branch": "main",
    "handlers": [
      { "name": "Insights", "status": "pending" }
    ]
  },
  "identity_resolution": {
    "status": "completed",
    "started_at": "2026-03-31T10:05:01Z",
    "completed_at": "2026-03-31T10:05:45Z",
    "branch": "identity",
    "handlers": [
      { "name": "Identity Resolution", "status": "completed" }
    ]
  }
}
```

The workflow's `run()` handler updates this JSONB between stages (journaled writes). The frontend polls the DB for the graph visualisation. Live status is also available via the workflow's `get_status()` shared handler (reads Restate K/V state).

### Frontend: pipeline visualisation

New component at the top of the ingestion page, above the existing source list and AI pipeline cards (which remain unchanged). Shows the pipeline as a horizontal DAG (GitHub Actions-style):

```
                                                    ┌─ Metrics ─┐   ┌─ Enrichment ──┐   ┌─ Embedding ─┐   ┌─ Insights ─┐
┌─ Team Sync ─┐   ┌─ Ingestion ────────────┐   ┌──→│  ✓  Done  │──→│  ● Running... │──→│  ○ Pending  │──→│  ○ Pending │
│             │   │  ● Github      ✓  142  │   │   └───────────┘   └───────────────┘   └─────────────┘   └────────────┘
│  ✓  Done    │──→│  ● Jira        ✗  err  │───┤
│             │   │  ● Discourse   ✓   87  │   │   ┌─ Identity Resolution ─┐
└─────────────┘   └─────────────────────────┘   └──→│  ✓  Done             │
                                                    └──────────────────────┘

                               [ Run Pipeline ]                          [ Cancel ]
```

#### React Flow (`@xyflow/react`)

The DAG is rendered using React Flow in read-only mode. This gives us custom node rendering, automatic edge routing (including the fork after ingestion), bezier connectors, and `fitView` for responsive sizing — without hand-rolling SVG path math.

```tsx
<ReactFlow
  nodes={nodes}
  edges={edges}
  nodeTypes={nodeTypes}
  nodesDraggable={false}
  nodesConnectable={false}
  elementsSelectable={false}
  panOnDrag={false}
  zoomOnScroll={false}
  preventScrolling={false}
  fitView
  fitViewOptions={{ padding: 0.2 }}
  proOptions={{ hideAttribution: true }}
/>
```

**Custom node types:**

| Node type | Purpose | Content |
| --- | --- | --- |
| `stageNode` | Single-handler stages (Metrics, Enrichment, etc.) | Stage name header, status indicator, optional item count |
| `fanOutNode` | Multi-handler stages (Ingestion, Team Sync) | Stage name header, list of handler rows with individual status dots |

Each custom node is a React component using shadcn Card styling, the existing `StatusDot` component for state indicators, and Lucide icons (Check, X, Loader2) for completion states. Nodes expose source/target handles for React Flow edge routing.

**Edge styling:**
- Default: `stroke: hsl(var(--border))`, bezier type
- Active (connecting completed → running): `stroke: hsl(var(--primary))`, animated
- Dimmed (connecting to pending stages): `stroke: hsl(var(--border))`, `opacity: 0.4`

**Graph construction:** A `buildPipelineGraph(pipeline: Pipeline)` function in `pipeline-graph.tsx` converts the pipeline status response into React Flow `Node[]` and `Edge[]` arrays. Node positions are computed with a simple column layout (fixed x spacing per stage, y offset for the fork). No auto-layout library needed since the topology is fixed.

**Controls:**
- "Run Pipeline" button — triggers `IngestionPipelineWorkflow::run()` via `TriggerPipeline` RPC
- "Cancel" button — visible when pipeline is running
- Individual source/handler Run buttons remain available below the pipeline card for one-off use

**Polling:** Reuse the existing adaptive polling infrastructure (burst → active → idle) from `ingestion-page.tsx`. Add a `usePipelineStatus()` query hook that fetches the latest pipeline record.

#### Component structure

```
views/ingestion/
├── components/
│   ├── pipeline-graph.tsx          # ReactFlow canvas, buildPipelineGraph(), edge styling
│   ├── pipeline-stage-node.tsx     # Custom ReactFlow node for single-handler stages
│   ├── pipeline-fan-out-node.tsx   # Custom ReactFlow node for multi-handler stages (ingestion)
│   └── pipeline-actions.tsx        # Run Pipeline / Cancel buttons
├── hooks/
│   └── use-pipeline.ts             # usePipelineStatus(), useTriggerPipeline()
```

### gRPC API additions

Extend `HandlersService` (or add to a new `PipelineService`):

```protobuf
// In handlers.proto or a new pipeline.proto

rpc GetPipelineStatus(GetPipelineStatusRequest) returns (GetPipelineStatusResponse);
rpc TriggerPipeline(TriggerPipelineRequest) returns (TriggerPipelineResponse);
rpc CancelPipeline(CancelPipelineRequest) returns (CancelPipelineResponse);

message GetPipelineStatusRequest {}

message GetPipelineStatusResponse {
  optional Pipeline current = 1;
  repeated Pipeline recent = 2;
}

message Pipeline {
  string id = 1;
  string status = 2;
  string current_stage = 3;
  string started_at = 4;
  optional string completed_at = 5;
  repeated PipelineStage stages = 6;
  optional string error = 7;
}

message PipelineStage {
  string name = 1;
  string status = 2;
  optional string started_at = 3;
  optional string completed_at = 4;
  repeated PipelineHandler handlers = 5;
}

message PipelineHandler {
  string name = 1;
  string status = 2;
  optional int32 items = 3;
  optional string error = 4;
}
```

`TriggerPipeline` creates a pipeline DB record, generates a workflow ID (the pipeline UUID), and invokes the workflow's `run()` handler via Restate's HTTP API. `CancelPipeline` calls the workflow's `cancel()` shared handler (cooperative) and optionally kills the Restate invocation via admin API (immediate). `GetPipelineStatus` reads from the DB; it can optionally call the workflow's `get_status()` shared handler for a running pipeline's real-time K/V state.

## Handler file structure

The pipeline workflow follows the existing feature-first layout (plan #18, plan #59). New module under `features/pipeline/`:

```
crates/ps-workers/src/features/
├── mod.rs                        ← add `pub mod pipeline;`
├── pipeline/
│   ├── mod.rs                    ← bind() function, pub use workflow types
│   ├── workflow.rs               ← IngestionPipelineWorkflow trait + impl (run, get_status, cancel)
│   └── stages.rs                 ← Stage execution helpers (fan-out, progress recording)
├── ingestion/                    ← existing (unchanged)
├── metrics/                      ← existing (unchanged)
├── reasoning/                    ← existing (unchanged)
└── identity_resolution/          ← existing (unchanged)
```

### `pipeline/mod.rs`

Follows the same pattern as `metrics/mod.rs` and `identity_resolution/mod.rs`:

```rust
pub mod workflow;
mod stages;

pub use workflow::{IngestionPipelineWorkflow, IngestionPipelineWorkflowImpl};

use restate_sdk::endpoint::Builder;
use crate::infra::SharedState;

pub fn bind(endpoint: Builder, state: &SharedState) -> Builder {
    let pipeline = IngestionPipelineWorkflowImpl {
        state: state.clone(),
    };
    endpoint.bind(pipeline.serve())
}
```

### `pipeline/workflow.rs`

The Restate workflow trait and implementation. Contains the `#[restate_sdk::workflow]` trait with `run()`, `get_status()`, and `cancel()`. The `run()` handler receives `WorkflowContext<'_>` and orchestrates the full pipeline. Shared handlers receive `SharedWorkflowContext<'_>`.

### `pipeline/stages.rs`

Extracted helpers to keep the workflow file focused on flow control:

- `run_team_sync_stage()` — discovers GitHub sources, fans out team sync calls, returns results
- `run_ingestion_stage()` — loads sources, builds fan-out calls, awaits all, returns per-handler results
- `run_main_branch()` — sequential: metrics → enrichment → embedding → insights, with cancellation checks between each
- `run_identity_resolution_branch()` — calls IdentityResolutionHandler if Discourse sources exist
- `record_stage_result()` — updates both K/V state and DB with a stage outcome
- `check_cancellation()` — non-blocking check of the cancel promise, returns bool

These are plain async functions called from within `run()`, receiving `&WorkflowContext` and `&SharedState`. This mirrors the existing pattern where `MetricsComputeHandlerImpl` has private helper methods alongside the handler impl.

### Binding in `main.rs`

Add alongside existing feature bindings:

```rust
let endpoint = ps_workers::features::ingestion::bind(endpoint, &state);
let endpoint = ps_workers::features::identity_resolution::bind(endpoint, &state);
let endpoint = ps_workers::features::metrics::bind(endpoint, &state);
let endpoint = ps_workers::features::reasoning::bind(endpoint, &state, ai_router);
let endpoint = ps_workers::features::pipeline::bind(endpoint, &state);  // new
```

## Implementation steps

### Phase 1 — Schema + repository layer

1. **Migration** — add `activity.pipelines` table (see Database schema section above)
2. **Repository** — add pipeline methods to `ActivityRepo`:
   - `create_pipeline(id, invocation_id) -> Pipeline`
   - `update_pipeline_stage(id, stage, stages_json)`
   - `complete_pipeline(id, status, stages_json)`
   - `get_latest_pipeline() -> Option<Pipeline>`
   - `list_recent_pipelines(limit) -> Vec<Pipeline>`
3. **Domain type** — add `Pipeline` struct to `ps-core/src/models/` (id, status, current_stage, started_at, completed_at, stages JSONB, error)

**Tests (Phase 1):**

Repository tests using `define_repo_test!` in `tests/integration/src/repo/activity.rs`:

- `create_pipeline_and_retrieve` — create pipeline, verify fields via `get_latest_pipeline()`
- `update_pipeline_stage_advances` — create, update through stages, verify `current_stage` and `stages` JSONB updates correctly
- `complete_pipeline_sets_status` — verify completed/failed/completed_with_warnings status and `completed_at` timestamp
- `list_recent_pipelines_ordered` — create multiple pipelines, verify ordering by `started_at DESC`

### Phase 2 — Pipeline workflow

4. **Pipeline workflow module** — create `features/pipeline/` with `mod.rs`, `workflow.rs`, `stages.rs` as described in the file structure section
5. **Wire up** — add `pub mod pipeline;` to `features/mod.rs`, bind in `main.rs`
6. **Remove all auto-triggers** — delete MetricsCompute `.send()` from all three ingestion handlers, delete EmbeddingHandler + InsightsHandler `.send()` from enrichment handler, delete IdentityResolution `.send()` from Discourse handler
7. **sqlx prepare** — update offline query cache for new pipeline queries

**Tests (Phase 2):**

The workflow itself orchestrates Restate calls and can't be unit-tested without a Restate runtime. Testing strategy:

- **Stage helper unit tests** — inline `#[cfg(test)]` in `stages.rs` for `record_stage_result()` logic (JSONB construction, status derivation from per-handler results). These are pure functions that don't need Restate.
- **Auto-trigger removal verification** — `cargo clippy` will catch any unused imports after removing the `.send()` calls. Manually verify the enrichment and ingestion handler files no longer reference downstream handler clients.
- **Verify `.call()` and workflow compile** — the workflow code must compile with `.call()` on both object and service clients, and with the `#[restate_sdk::workflow]` macro. This is a type-level check — if the SDK supports it, the compiler confirms it.

Full end-to-end pipeline testing requires a running Restate instance and is deferred to manual validation (same as existing handlers — none of the current Restate handlers have automated integration tests against a real Restate runtime).

### Phase 3 — gRPC API

8. **Proto** — add pipeline messages and RPCs to `handlers.proto` (or a new `pipeline.proto` if it gets large):
   - `GetPipelineStatus`, `TriggerPipeline`, `CancelPipeline` RPCs
   - `Pipeline`, `PipelineStage`, `PipelineHandler` messages
9. **buf lint + buf generate** — regenerate Rust + TypeScript clients
10. **Server** — implement the three RPCs in `ps-server/src/services/handlers/`:
    - `GetPipelineStatus` — delegates to `ActivityRepo::get_latest_pipeline()` + `list_recent_pipelines()`
    - `TriggerPipeline` — checks no active pipeline exists, generates workflow ID (pipeline UUID), sends to Restate workflow via HTTP, returns workflow ID
    - `CancelPipeline` — calls the workflow's `cancel()` shared handler via Restate HTTP (cooperative); optionally also cancels the invocation via admin API (immediate)

**Tests (Phase 3):**

API tests using `define_api_test!` in `tests/integration/src/api/ingestion.rs` (or a new `pipeline.rs`):

- `get_pipeline_status_empty` — no pipeline records exist, response has no `current` and empty `recent`
- `get_pipeline_status_with_records` — seed pipeline records via repo, verify the gRPC response correctly maps domain types to proto types (status, stages, handler details)
- `trigger_pipeline_rejects_when_active` — seed an active (running) pipeline record, verify `TriggerPipeline` returns an appropriate error
- `cancel_pipeline_not_found` — cancel when no pipeline running, verify error handling

Note: `TriggerPipeline` and `CancelPipeline` make HTTP calls to Restate which won't be available in the test environment. These RPCs should be tested for input validation and DB state checks only — the actual Restate dispatch is tested manually.

### Phase 4 — Frontend

11. **Hooks** — `views/ingestion/hooks/use-pipeline.ts`:
    - `usePipelineStatus()` — React Query hook polling `GetPipelineStatus`, adaptive interval matching existing ingestion polling
    - `useTriggerPipeline()` — mutation calling `TriggerPipeline` RPC
    - `useCancelPipeline()` — mutation calling `CancelPipeline` RPC
12. **Pipeline graph components**:
    - `pipeline-graph.tsx` — top-level card: horizontal layout of stage columns with connecting arrows
    - `pipeline-stage.tsx` — single stage column: header + handler rows
    - `pipeline-handler-row.tsx` — handler status within a stage (status dot, name, items/error)
    - `pipeline-actions.tsx` — Run Pipeline / Cancel buttons with loading states
13. **Integrate** — add `PipelineGraph` card to `ingestion-page.tsx` above the existing `SourceList` and `AiPipelineStatus` cards

**Tests (Phase 4):**

Colocated vitest tests alongside components:

- `views/ingestion/hooks/use-pipeline.test.ts` — test `usePipelineStatus()` hook with `createRouterTransport` mock returning various pipeline states (running, completed, failed, empty). Verify query key structure and polling interval behaviour.
- `views/ingestion/components/pipeline-stage.test.tsx` — render `PipelineStage` with different status combinations (all pending, mixed success/failure, all complete). Verify correct status indicators, handler names, item counts, and error display.
- `views/ingestion/components/pipeline-actions.test.tsx` — verify Run Pipeline button disabled when pipeline is running, Cancel button visible only when running, loading states during mutation.

## Considerations

### Fan-out with mixed handler types

Team sync and ingestion handlers are virtual objects (keyed by source type), while metrics, enrichment, embedding, insights, and identity resolution are services (keyless). The workflow calls both:

```rust
// Object call (keyed)
ctx.object_client::<GithubIngestionHandlerClient>("Github")
    .run_ingestion().call().await

// Service call (keyless)
ctx.service_client::<EnrichmentHandlerClient>()
    .run_cycle().call().await
```

Both return `CallFuture` and can be `.await`ed the same way.

### Dynamic source list

The fan-out isn't static — it depends on which sources are configured and enabled. The workflow loads the source list from the DB at runtime and builds the call set dynamically. Sources added or removed between pipeline runs automatically change the fan-out set.

Since Restate journals are positional, changing the number of sources between retries of the same invocation would break replay. This is safe because the source list is loaded inside a `ctx.run()` closure (journaled), so retries see the same list.

### Partial failure semantics

**Team sync failure:**
- Non-fatal. Logs a warning, records the failure in stages JSONB, but proceeds to ingestion. Ingestion will use whatever team data already exists from prior syncs.

**Some ingestion handlers fail:**
- Pipeline continues to both branches (partial data is better than none)
- Pipeline final status: `completed_with_warnings`
- The stages JSONB records which handlers failed and why
- UI shows failed handlers with error details in the graph

**ALL ingestion handlers fail:**
- Pipeline skips both downstream branches
- Pipeline status: `failed`

**Main branch failure** (metrics, enrichment, embedding, or insights):
- That branch marks the failing stage and stops (no point running downstream stages within the branch)
- The identity resolution branch is unaffected (already running concurrently or already completed)
- Pipeline status: `failed` with error on the failing stage

**Identity resolution failure:**
- Main branch is unaffected
- Pipeline status: `completed_with_warnings` (if main branch succeeded) or `failed` (if both branches failed)

### Concurrent pipeline prevention

Each pipeline run uses a unique workflow ID (the pipeline UUID), so Restate doesn't inherently prevent concurrent runs. The `TriggerPipeline` RPC checks the DB for an active pipeline (`status = 'running'`) and returns an error if one exists. This is a server-side guard, not a Restate-level guarantee — but it's sufficient since all pipeline triggers flow through our gRPC server.

### Journal compatibility

Adding the new workflow handler doesn't affect existing handler journals. The existing handler changes are:
- Removing MetricsCompute `.send()` from ingestion handlers — changes their journal shape
- Removing EmbeddingHandler + InsightsHandler `.send()` from enrichment — changes its journal shape
- Removing IdentityResolution `.send()` from Discourse handler — changes its journal shape

Cancel all in-flight ingestion, enrichment, and Discourse invocations before deploying.

### Backfill

The pipeline always runs incremental ingestion (`run_ingestion()`). Backfills remain manual per-source operations via the existing backfill UI. A future enhancement could add a "backfill pipeline" variant.

### Workflow retention

Restate retains workflow state for 24 hours after `run()` completes (configurable via admin API). This means `get_status()` returns results for up to 24 hours after a pipeline finishes. After that, the workflow state is garbage collected — which is fine because we persist everything to the DB at each stage transition. The DB is the source of truth for historical pipeline records; Restate K/V state is a convenience for real-time queries during execution.

### Why `#[restate_sdk::workflow]` over `#[restate_sdk::object]`?

A pipeline is a workflow — Restate provides exactly the right abstraction. Using it gives us:
- **Exactly-once `run()` per workflow ID** — no accidental re-execution of the same pipeline
- **`#[shared]` handlers** — `get_status()` and `cancel()` run concurrently with `run()`, no custom polling or admin API hacking needed
- **K/V state** — live progress readable from shared handlers without touching the DB
- **Durable promises** — clean cooperative cancellation pattern
- **Future-proof** — if Restate adds workflow features (e.g., history, observability), we benefit automatically

The alternative (virtual object keyed by `"singleton"`) would require reimplementing status queries and cancellation manually. The workflow type gives us these for free.
