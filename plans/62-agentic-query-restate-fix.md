# Plan 62: Fix Agentic Query Restate Suspension & Stuck Conversations

## Context

The agentic query system (`AgenticQueryHandler.run_query`) runs up to 5 minutes of non-journaled SSE streaming inside a Restate Object handler. Restate's `ABORT_TIMEOUT` is also 5 minutes. When these timers race:

1. **Restate suspends** the invocation (error 599) during or just after SSE streaming
2. **On replay**, the journaled `cleanup_events` step has already deleted events the recovery logic needs, producing empty answers
3. **The handler retries forever** — each attempt: subscribes to SSE, times out, gets suspended, replays
4. **The Restate Object serializes per key** (conversation_id), so ALL subsequent messages/retries for that conversation queue behind the stuck invocation indefinitely

The root cause is an architectural mismatch: long-running non-journaled work inside a system designed for short, journaled operations.

## Approach: Move SSE Streaming to ps-server

Split the Restate handler into a fast "prepare" operation (pod lifecycle) and move SSE streaming into ps-server, which is already holding the gRPC stream open for 5 minutes anyway.

### Current flow
```
Frontend → askQuestion gRPC → ps-server
  ps-server: trigger Restate /send (fire-and-forget), start DB poll loop (5 min)
  Restate run_query: pod setup (journaled) → SSE streaming (NOT journaled, 5 min) → finalize (journaled)
  ps-server poll loop: reads events from DB every 100ms, streams to frontend
```

### New flow
```
Frontend → askQuestion gRPC → ps-server
  ps-server: call Restate prepare_query (sync, ~30-90s) → get pod_ip
  ps-server: connect to OpenCode SSE directly → stream to frontend AND write to DB
  ps-server: finalize (store message, update status) — no Restate involved
```

What this eliminates:
- No long-running non-journaled work in Restate (prepare_query completes in <90s)
- No journal replay issues (SSE streaming has no journal)
- No event cleanup ordering problems (cleanup happens synchronously after streaming)
- No stuck invocations blocking subsequent calls
- No DB polling latency (events flow directly OpenCode → server → client)

What we keep:
- Events still written to DB (for resume/reconnect via `resume_stream`)
- Restate for durable pod lifecycle (what it's good at)
- Frontend unchanged (same proto events)
- DB schema unchanged (no migrations)

---

## Phase 1: Core Architecture Change

### 1.1 Feature-gate k8s deps in ps-agent, add to ps-server

ps-server only needs `EventMapper`, `opencode_sdk`, and `OPENCODE_PORT` from ps-agent — none of which touch Kubernetes. Feature-gate the k8s modules so ps-server doesn't pull in `kube` + `k8s-openapi`.

**File:** `crates/ps-agent/Cargo.toml`

Add a `kube` feature that gates the k8s dependencies:

```toml
[features]
default = []
kube = ["dep:kube", "dep:k8s-openapi"]

[dependencies]
kube = { version = "3.1", features = ["runtime", "client", "derive"], optional = true }
k8s-openapi = { version = "0.27", features = ["latest"], optional = true }
```

**File:** `crates/ps-agent/src/lib.rs`

Gate `container_manager` and `pod_spec` behind `#[cfg(feature = "kube")]`. Move `OPENCODE_PORT` to the crate root so it's always available:

```rust
#[cfg(feature = "kube")]
pub mod container_manager;
pub mod event_mapper;
#[cfg(feature = "kube")]
pub mod pod_spec;

/// The port OpenCode listens on inside agent pods.
pub const OPENCODE_PORT: u16 = 4096;

#[cfg(feature = "kube")]
pub use container_manager::{ContainerManager, PodOverrides, PodStatus, pvc_name_for_session};
#[cfg(feature = "kube")]
pub use pod_spec::{ANNOTATION_TOKEN_SESSION_ID, AgentPodConfig, WORKSPACE_MOUNT_PATH};

pub use opencode_sdk;
```

**File:** `crates/ps-agent/src/container_manager.rs`

Remove the `OPENCODE_PORT` constant (now in `lib.rs`). Update `opencode_client()` to use `crate::OPENCODE_PORT`.

**File:** `crates/ps-workers/Cargo.toml`

Enable the `kube` feature:

```toml
ps-agent = { path = "../ps-agent", features = ["kube"] }
```

**File:** `crates/ps-server/Cargo.toml`

Add without the `kube` feature — only brings `opencode_sdk` and `event_mapper`:

```toml
ps-agent = { path = "../ps-agent" }
```

### 1.2 New Restate handler: `prepare_query`

**File:** `crates/ps-workers/src/features/reasoning/agentic_query/handler.rs`

Replace `run_query` with `prepare_query` that returns pod_ip. Keep the existing journaled steps for pod lifecycle:

```
prepare_query(request) -> PrepareQueryResponse:
  1. journaled: update_status_running
  2. journaled: create_agent_session (generate service token)
  3. journaled: event_container_creating
  4. journaled: ensure_pod
  5. NOT journaled: wait_for_ready (~30-60s, well within 5min abort timeout)
  6. journaled: event_container_ready
  7. return { pod_ip }
```

Total wall time: <90s. Remove `run_query` entirely.

**File:** `crates/ps-workers/src/features/reasoning/agentic_query/mod.rs`

Add response type:
```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrepareQueryResponse {
    pub pod_ip: String,
}
```

### 1.3 Promote `agent_query` to a subdirectory

The current `agent_query.rs` is already 642 lines. Adding SSE streaming, session management, artifact handling, and step tracking into one or two files would create a god file. Promote to a feature directory mirroring the ps-workers structure:

```
crates/ps-server/src/services/reasoning/agent_query/
├── mod.rs              # ask_question + resume_stream RPC handlers (~200 lines)
├── event_loop.rs       # SSE streaming + DB event writing (~200 lines)
├── event_mapping.rs    # DB event → proto mapping (extracted from old agent_query.rs, ~210 lines)
├── session.rs          # OpenCode session resolution + prompt sending (~100 lines)
├── artifact.rs         # Artifact upload interception (~80 lines)
└── step_registry.rs    # Step identity tracking (~190 lines, includes tests)
```

All modules stay well under the 500-line guideline. Each has a single responsibility.

#### `mod.rs` — RPC handlers

The two RPC entry points: `ask_question` and `resume_stream`. Validation, conversation creation, and stream setup live here. The streaming task delegates to `event_loop::run_event_loop()`.

```
ask_question(request):
  1. Validate, create/resume conversation, store user message (unchanged)
  2. Concurrency guard: reject if query_status is already "running"
  3. Send conversation_created event to client (unchanged)
  4. Spawn streaming task:
     a. Call Restate prepare_query synchronously (POST, not /send):
        POST {restate_url}/AgenticQueryHandler/{conv_id}/prepare_query
        Returns { pod_ip } when pod is ready
        While waiting: poll DB for container_status events, stream to client
     b. Create opencode_sdk::Client from pod_ip
     c. Resolve/create OpenCode session (session::resolve_or_create)
     d. Send question (session::send_prompt_or_compact)
     e. Run event loop (event_loop::run_event_loop)
     f. On completion: store assistant message, update totals, set status=completed
     g. Clean up events from DB
     h. On failure: store error message, set status=failed

resume_stream(request):
  Unchanged — polls DB for events, uses event_mapping for proto conversion.
```

#### `event_loop.rs` — SSE streaming + DB writes

Adapted from `ps-workers/agentic_query/event_loop.rs`. Subscribes to OpenCode SSE, maps events via `EventMapper` (from ps-agent), writes each event to DB (for resume_stream), and sends to the gRPC response channel. No Restate `ctx` dependency.

#### `event_mapping.rs` — DB event → proto conversion

Extracted from the current `agent_query.rs` (the two `map_db_event_to_*` functions, ~210 lines). Pure mapping, no I/O.

#### `session.rs` — OpenCode session management

Adapted from `ps-workers/agentic_query/query_core.rs`, minus the Restate-only `check_session_state`/`SessionState` replay detection. Two functions:
- `resolve_or_create_session()` — find or create an OpenCode session
- `send_prompt_or_compact()` — send the question, compacting if context is too large

#### `artifact.rs` — Artifact upload handling

Direct adaptation of `ps-workers/agentic_query/artifact.rs` (~80 lines). Inspects SSE events for artifact tool calls and registers them in the DB.

#### `step_registry.rs` — Step identity tracking

Direct copy of `ps-workers/agentic_query/step_registry.rs` (~190 lines including tests). Self-contained, no external dependencies beyond std. Tracks step ordering for thinking blocks and tool calls.

### 1.4 Update `resume_stream`

No changes needed. `resume_stream` continues polling the DB for events written by the `ask_question` streaming task (step 1.3 event_loop). The existing terminal status fallback (synthesize `final_answer` from stored message when status is `completed`) continues to work.

### 1.5 Update cancellation

**Frontend cancel** already aborts the gRPC stream via `AbortController`. When the stream drops:
- The `tx` channel in the spawned task becomes closed
- `tx.send()` returns `Err` → task exits → SSE subscription is dropped
- Task sets `query_status = "cancelled"` before exiting

**Backend cancel** (Restate `cancel` handler): keep as-is. It sets status to `cancelled`. The streaming task in ps-server checks status periodically and exits if cancelled.

### 1.6 Atomic concurrency guard

The Plan 57 architecture used Restate Object keying for at-most-one concurrency per conversation — a hard guarantee. Moving SSE streaming back to ps-server loses that guarantee for the streaming phase. A naive status check (`if query_status == 'running' { reject }`) is vulnerable to TOCTOU races where two near-simultaneous requests both read `idle` and both proceed.

**Fix:** Use an atomic compare-and-swap query instead of read-then-write.

**File:** `crates/ps-core/src/repo/reasoning/conversations.rs`

Add `try_claim_query` method:
```rust
/// Atomically claim a conversation for query execution.
/// Returns `true` if the claim succeeded (status was `idle`),
/// `false` if another request already claimed it.
pub async fn try_claim_query(&self, conversation_id: Uuid) -> Result<bool, Error> {
    let result = sqlx::query!(
        r#"
        UPDATE reasoning.conversations
        SET query_status = 'pending', last_activity_at = now()
        WHERE id = $1 AND query_status = 'idle'
        "#,
        conversation_id,
    )
    .execute(&self.pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
```

**File:** `crates/ps-server/src/services/reasoning/agent_query/mod.rs`

Replace the current `update_query_status(conversation_id, "pending")` call with:
```rust
let claimed = svc.repos.reasoning.try_claim_query(conversation_id).await.map_err(db_err)?;
if !claimed {
    return Err(Status::already_exists("a query is already running for this conversation"));
}
```

The single `UPDATE ... WHERE query_status = 'idle'` is atomic at the DB level — PostgreSQL's row-level locking prevents two concurrent transactions from both succeeding. `prepare_query` in Restate still provides Object-level serialization for the pod lifecycle phase, but the streaming phase in ps-server is now also protected.

### 1.7 Query watchdog handler

Moving SSE streaming from Restate to ps-server means we lose Restate's durability guarantee for failure cleanup. If ps-server crashes, is OOM-killed, or the streaming task panics, conversations can get permanently stuck in `running` or `pending` status with no one to clean them up. The Plan 57 architecture used journaled `fail_run!` steps that Restate guaranteed would execute even after crashes.

**Fix:** Add a periodic Restate watchdog handler that detects and resets stuck conversations.

**File:** `crates/ps-core/src/repo/reasoning/conversations.rs`

Add `reset_stale_queries` method:
```rust
/// Reset conversations stuck in `running`/`pending` for longer than the
/// threshold. Returns the IDs of conversations that were reset.
pub async fn reset_stale_queries(&self, stale_minutes: i32) -> Result<Vec<Uuid>, Error> {
    let rows = sqlx::query_scalar!(
        r#"
        UPDATE reasoning.conversations
        SET query_status = 'failed', last_activity_at = now()
        WHERE query_status IN ('running', 'pending')
          AND last_activity_at < now() - make_interval(mins => $1)
        RETURNING id
        "#,
        stale_minutes,
    )
    .fetch_all(&self.pool)
    .await?;
    Ok(rows)
}
```

**File:** `crates/ps-workers/src/features/reasoning/query_watchdog.rs` (new)

Follows the same durable self-scheduling pattern as `AgentPodReaperHandler`:

```rust
/// Conversations stuck longer than this are reset to `failed`.
/// Covers: pod startup (~90s) + SSE timeout (300s) + margin.
const STALE_THRESHOLD_MINUTES: i32 = 10;
const WATCHDOG_INTERVAL_SECS: u64 = 60;
pub const WATCHDOG_KEY: &str = "singleton";

#[restate_sdk::object]
pub trait QueryWatchdogHandler {
    async fn check() -> Result<(), TerminalError>;
}

impl QueryWatchdogHandler for QueryWatchdogHandlerImpl {
    async fn check(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let repos = &self.state.repos;

        let stale_ids = journaled!(ctx, "reset_stale_queries", [repos], {
            repos.reasoning.reset_stale_queries(STALE_THRESHOLD_MINUTES)
                .await.map_err(|e| TerminalError::new(format!("...: {e}")))?
        });

        if !stale_ids.is_empty() {
            warn!(count = stale_ids.len(), "reset stuck conversations to failed");
            for conv_id in &stale_ids {
                // Write an error message so the user sees an explanation.
                let _ = repos.reasoning.create_message(&CreateMessageParams {
                    conversation_id: *conv_id,
                    role: "error",
                    content: "This query was terminated because it stopped responding. Please retry.",
                    ..
                }).await;
                let _ = repos.reasoning.delete_events(*conv_id).await;
            }
        }

        ctx.object_client::<QueryWatchdogHandlerClient>(WATCHDOG_KEY)
            .check().send_after(Duration::from_secs(WATCHDOG_INTERVAL_SECS));
        Ok(())
    }
}
```

**File:** `crates/ps-workers/src/features/reasoning/mod.rs`

Add `pub mod query_watchdog`, instantiate `QueryWatchdogHandlerImpl`, bind to endpoint.

**File:** `crates/ps-workers/src/main.rs`

Add `bootstrap_watchdog()` following the same duplicate-prevention pattern as `bootstrap_reaper()` — query Restate admin for existing `QueryWatchdogHandler` invocations before sending a new one.

### 1.8 Explicit timeout layering

The system has multiple timeout boundaries that must be layered correctly. Without explicit coordination, a timeout at the wrong level can leave orphaned work or confusing error messages.

**Timeout hierarchy (outer → inner):**

| Layer | Timeout | Rationale |
|-------|---------|-----------|
| gRPC stream (ps-server poll loop) | **5 min** (300s) | Client-facing deadline; longest a user waits |
| HTTP call to Restate `prepare_query` | **2 min** (120s) | Must be < gRPC stream timeout to leave room for SSE streaming |
| Restate `ABORT_TIMEOUT` | **2 min** (120s) | Must be >= HTTP call timeout so Restate doesn't abort before the caller gives up |
| `prepare_query` wall time | **~90s** | Pod startup; must be < HTTP/Restate timeouts |
| SSE streaming (OpenCode) | **~3 min** | Remaining budget after prepare_query within gRPC stream timeout |

**Rule:** Each inner timeout must be strictly less than the layer above it, so failures propagate cleanly inward rather than causing orphaned work at outer layers.

**File:** `crates/ps-server/src/services/reasoning/agent_query/mod.rs`

Define named constants at the top of the module:
```rust
/// Maximum time the gRPC stream stays open (client-facing).
const STREAM_TIMEOUT: Duration = Duration::from_secs(300);

/// Maximum time to wait for Restate `prepare_query` to return the pod IP.
/// Must be < STREAM_TIMEOUT to leave budget for SSE streaming.
const PREPARE_TIMEOUT: Duration = Duration::from_secs(120);
```

Use `PREPARE_TIMEOUT` on the HTTP call to Restate:
```rust
let resp = http_client
    .post(&url)
    .timeout(PREPARE_TIMEOUT)
    .header("content-type", "application/json")
    .body(body)
    .send()
    .await?;
```

Use `STREAM_TIMEOUT` for the poll loop deadline (currently hardcoded as `300`).

**File:** `k8s/base/restate.yaml`

Change `RESTATE_WORKER__INVOKER__ABORT_TIMEOUT` from `5min` to `2min`.

---

## Phase 2: Cleanup

### 2.1 Remove dead code from ps-workers

Remove `run_query`, `query_core.rs`, `event_loop.rs`, `artifact.rs`, `trace.rs`, `step_registry.rs` from `crates/ps-workers/src/features/reasoning/agentic_query/`. Keep `handler.rs` (with just `prepare_query`, `cancel`, `cleanup_storage`) and `mod.rs`.

### 2.2 Update CLAUDE.md

Update the dependency flow diagram:
```
ps-server → ps-core, ps-proto, ps-metrics, ps-agent (no kube feature)
ps-workers → ps-core, ps-proto, ps-metrics, ps-agent (kube feature)
```

Document the new architecture in the Restate Handler Architecture section.

---

## Deployment

1. **Cancel all in-flight AgenticQueryHandler invocations** via Restate admin API:
   ```
   # Query for all active invocations
   curl -H 'accept: application/json' 'http://restate:9070/query' \
     -d '{"query": "SELECT id FROM sys_invocation WHERE target_service_name = '\''AgenticQueryHandler'\''"}'
   # Kill each one
   curl -X DELETE 'http://restate:9070/invocations/{id}?mode=kill'
   ```

2. **Reset stuck conversations** in DB:
   ```sql
   UPDATE reasoning.conversations SET query_status = 'idle'
   WHERE query_status IN ('running', 'pending');
   ```

3. **Re-register deployment** with Restate (the handler signature changed):
   ```
   restate deployments register http://ps-workers:9081/ --force --yes
   ```

4. Deploy ps-workers and ps-server together.

---

## Verification

1. **New conversation**: Ask a question, verify chart generation works end-to-end, answer appears, state transitions to completed
2. **Follow-up message**: Ask a follow-up in same conversation, verify it works without queueing
3. **Cancel**: Start a query, click cancel, verify streaming stops and input re-enables
4. **Resume**: Start a query, navigate away, navigate back — verify auto-resume shows progress
5. **Error handling**: Trigger an error (e.g., bad model), verify inline error with retry button
6. **Pod restart**: Kill the agent pod mid-query, verify graceful error (not infinite loop)
7. **Concurrency guard**: Double-click the send button rapidly — second request should get `ALREADY_EXISTS`, not a duplicate query
8. **Watchdog recovery**: Start a query, kill ps-server mid-stream, wait 10+ minutes — verify the watchdog resets the conversation to `failed` with an error message
9. **Watchdog bootstrap**: Restart ps-workers twice rapidly — verify only one watchdog chain runs (no geometric growth)
10. **Timeout layering**: Verify `prepare_query` HTTP call uses `PREPARE_TIMEOUT` (120s), poll loop uses `STREAM_TIMEOUT` (300s)
11. **Run `prek run -av`**: All lints, tests, formatters clean

---

## Key files to modify

| File | Change |
|------|--------|
| `crates/ps-agent/Cargo.toml` | Feature-gate `kube` + `k8s-openapi` behind `kube` feature |
| `crates/ps-agent/src/lib.rs` | Gate k8s modules, move `OPENCODE_PORT` to crate root |
| `crates/ps-agent/src/container_manager.rs` | Remove `OPENCODE_PORT` (now in lib.rs), use `crate::OPENCODE_PORT` |
| `crates/ps-workers/Cargo.toml` | Enable `kube` feature on ps-agent |
| `crates/ps-server/Cargo.toml` | Add `ps-agent` (no `kube` feature) |
| `crates/ps-server/src/services/reasoning/agent_query/mod.rs` | RPC handlers: ask_question (rewritten), resume_stream (moved); atomic concurrency guard via `try_claim_query`; named timeout constants |
| `crates/ps-server/src/services/reasoning/agent_query/event_loop.rs` | New: SSE streaming + DB event writing |
| `crates/ps-server/src/services/reasoning/agent_query/event_mapping.rs` | Extracted: DB event → proto mapping |
| `crates/ps-server/src/services/reasoning/agent_query/session.rs` | New: OpenCode session resolution + prompt sending |
| `crates/ps-server/src/services/reasoning/agent_query/artifact.rs` | New: artifact upload interception |
| `crates/ps-server/src/services/reasoning/agent_query/step_registry.rs` | New: step identity tracking (with tests) |
| `crates/ps-server/src/services/reasoning/mod.rs` | Update `mod agent_query` (file → directory) |
| `crates/ps-core/src/repo/reasoning/conversations.rs` | Add `try_claim_query` (atomic CAS) and `reset_stale_queries` repo methods |
| `crates/ps-workers/src/features/reasoning/agentic_query/handler.rs` | Replace run_query with prepare_query |
| `crates/ps-workers/src/features/reasoning/agentic_query/mod.rs` | Add PrepareQueryResponse, update exports |
| `crates/ps-workers/src/features/reasoning/query_watchdog.rs` | New: periodic watchdog handler to reset stuck conversations |
| `crates/ps-workers/src/features/reasoning/mod.rs` | Add `pub mod query_watchdog`, bind handler |
| `crates/ps-workers/src/main.rs` | Add `bootstrap_watchdog()` with duplicate-prevention |
| `k8s/base/restate.yaml` | Reduce ABORT_TIMEOUT to 2min |
