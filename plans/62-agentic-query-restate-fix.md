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

### 1.1 Add ps-agent dependency to ps-server

**File:** `crates/ps-server/Cargo.toml`

Add `ps-agent = { path = "../ps-agent" }`. This brings in `opencode_sdk` (re-exported from ps-agent), `EventMapper`, and the `OPENCODE_PORT` constant needed for direct SSE streaming.

Note: this also brings `kube` + `k8s-openapi` as transitive deps. ps-server doesn't use them directly — only the OpenCode client and event mapper. If binary size is a concern, we can feature-gate later.

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

### 1.3 Rewrite `ask_question` in ps-server

**File:** `crates/ps-server/src/services/reasoning/agent_query.rs`

Replace the "trigger Restate + DB poll loop" with "call Restate prepare + direct SSE":

```
ask_question(request):
  1. Validate, create/resume conversation, store user message (unchanged)
  2. Send conversation_created event to client (unchanged)
  3. Spawn streaming task:
     a. Call Restate prepare_query synchronously (POST, not /send):
        POST {restate_url}/AgenticQueryHandler/{conv_id}/prepare_query
        Returns { pod_ip } when pod is ready
        While waiting: poll DB for container_status events, stream to client
     b. Create opencode_sdk::Client from pod_ip
     c. Resolve/create OpenCode session (logic from query_core.rs)
     d. Subscribe to SSE, send question (logic from query_core.rs)
     e. Run event loop — for each event:
        - Map to proto (using EventMapper from ps-agent)
        - Write to DB (for resume_stream)
        - Send to gRPC response stream
     f. On completion: store assistant message, update totals, set status=completed
     g. Clean up events from DB
     h. On failure: store error message, set status=failed
```

### 1.4 Move reusable logic from ps-workers to ps-server

These functions from `crates/ps-workers/src/features/reasoning/agentic_query/` are pure (no Restate `ctx` dependency) and need to be available in ps-server:

| Function | Source file | What it does |
|----------|------------|--------------|
| `resolve_or_create_session` | `query_core.rs` | Find/create OpenCode session |
| `check_session_state` | `query_core.rs` | Detect stale/replay sessions |
| `send_prompt_or_compact` | `query_core.rs` | Send question to OpenCode |
| `run_event_loop` | `event_loop.rs` | Stream SSE events with timeout |
| `handle_artifact_upload` | `artifact.rs` | Intercept artifact events |
| `derive_trace_from_events` | `trace.rs` | Build reasoning trace |
| `StepRegistry` | `step_registry.rs` | Track step ordering |

**Approach:** Reimplement in a new module `crates/ps-server/src/services/reasoning/query_executor.rs`. These functions are self-contained (~300 lines total) and adapting them inline is simpler than restructuring crate dependencies. The ps-server version won't need the replay detection (`check_session_state`, `SessionState`) since replay is a Restate-only concern.

### 1.5 Update `resume_stream` 

**File:** `crates/ps-server/src/services/reasoning/agent_query.rs`

No changes needed for Phase 1. `resume_stream` continues polling the DB for events written by the `ask_question` streaming task. This works because step 1.3e writes events to DB.

The existing terminal status fallback (synthesize `final_answer` from stored message when status is `completed`) continues to work.

### 1.6 Update cancellation

**Frontend cancel** already aborts the gRPC stream via `AbortController`. When the stream drops:
- The `tx` channel in the spawned task becomes closed
- `tx.send()` returns `Err` → task exits → SSE subscription is dropped
- Task sets `query_status = "cancelled"` before exiting

**Backend cancel** (Restate `cancel` handler): keep as-is. It sets status to `cancelled`. The streaming task in ps-server checks status periodically and exits if cancelled.

### 1.7 Concurrency guard

Restate Object handlers serialize per key, preventing concurrent `run_query` for the same conversation. Without Restate guarding the streaming path, we need a check:

At the start of `ask_question`, if `query_status` is already `running`, reject with a clear error: "A query is already running for this conversation."

### 1.8 Reduce Restate abort timeout

**File:** `k8s/base/restate.yaml`

Change `RESTATE_WORKER__INVOKER__ABORT_TIMEOUT` from `5min` to `2min`. The longest Restate operation is now pod startup (~90s).

---

## Phase 2: Cleanup

### 2.1 Remove dead code from ps-workers

Remove `run_query`, `query_core.rs`, `event_loop.rs`, `artifact.rs`, `trace.rs`, `step_registry.rs` from `crates/ps-workers/src/features/reasoning/agentic_query/`. Keep `handler.rs` (with just `prepare_query`, `cancel`, `cleanup_storage`) and `mod.rs`.

### 2.2 Update CLAUDE.md

Update the dependency flow diagram:
```
ps-server → ps-core, ps-proto, ps-metrics, ps-agent
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
7. **Run `prek run -av`**: All lints, tests, formatters clean

---

## Key files to modify

| File | Change |
|------|--------|
| `crates/ps-server/Cargo.toml` | Add `ps-agent` dependency |
| `crates/ps-server/src/services/reasoning/agent_query.rs` | Rewrite ask_question: Restate prepare + direct SSE |
| `crates/ps-server/src/services/reasoning/query_executor.rs` | New: session mgmt, event loop, artifact handling |
| `crates/ps-workers/src/features/reasoning/agentic_query/handler.rs` | Replace run_query with prepare_query |
| `crates/ps-workers/src/features/reasoning/agentic_query/mod.rs` | Add PrepareQueryResponse, update exports |
| `k8s/base/restate.yaml` | Reduce ABORT_TIMEOUT to 2min |
