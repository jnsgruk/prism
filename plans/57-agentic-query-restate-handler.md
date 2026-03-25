# Plan 57: Migrate Agentic Query to Restate Handler

## Context

The `ask_question()` streaming RPC in `ps-server/services/reasoning/agent_query.rs` is a 460-line orchestration function that manages K8s Pod lifecycle, OpenCode SSE streaming, artifact interception, and conversation persistence — all inside a spawned tokio task within a gRPC server-streaming handler.

This violates two architectural principles:

1. **Services are thin gRPC adapters** (CLAUDE.md) — `ask_question()` is the entire orchestration pipeline, not a thin adapter.
2. **All long-running background work must run as Restate handlers** (CLAUDE.md) — agentic queries can run for minutes, but currently have no durability, cancellation, or journal visibility.

**Goal:** Move the agent orchestration into a Restate handler in `ps-workers`, giving agentic queries the same durability, cancellation, and observability guarantees as ingestion runs.

**Dependencies:**
- [56-agentic-query-interface.md](./56-agentic-query-interface.md) — current implementation
- [18-code-structure.md](./18-code-structure.md) — structural guidelines

---

## Current Architecture

```
Browser → gRPC server-stream (AskQuestion) → ps-server
  └─ tokio::spawn:
      1. ensure_pod()          K8s Pod creation
      2. wait_for_pod_ready()  Poll loop (60s)
      3. OpenCode session      Create or reuse
      4. SSE subscribe         Global event stream
      5. send_text_async()     Send question
      6. Stream events         Map OpenCode → proto, intercept artifacts
      7. Store message         DB write
      8. Final answer          Close stream
```

**Problems:**
- Server restart kills in-flight queries with no recovery
- No cancellation API (abort controller is in-process only)
- No progress visibility outside the active gRPC stream
- Pod reaper is a loose `tokio::spawn` in main.rs — no durability
- `ask_question()` couples K8s orchestration, SSE streaming, artifact DB writes, and conversation management into one function

---

## Target Architecture

```
Browser → gRPC server-stream (AskQuestion) → ps-server
  └─ 1. Create/resume conversation in DB
     2. TriggerHandler → Restate AgenticQueryHandler.run_query()
     3. Poll conversation status + messages from DB
     4. Stream AgentEvent protos to client

ps-workers (Restate):
  AgenticQueryHandler.run_query(session_id, question):
     1. ctx.run("ensure_pod")    → Pod IP            [journaled]
     2. ctx.run("create_session") → OpenCode session  [journaled]
     3. SSE subscribe + event loop                    [NOT journaled — streaming]
     4. ctx.run("store_message") → DB write           [journaled]
     5. ctx.run("update_totals") → conversation stats [journaled]

  AgentPodReaperHandler.reap():
     1. ctx.run("reap_idle") → delete stale pods     [journaled]
     2. ctx.run("cleanup_sessions") → delete tokens   [journaled]
     3. ctx.object_client().reap().send_with_delay()  [durable timer]
```

### Key Design Decisions

**1. Streaming via DB polling, not Restate journal**

The gRPC server-stream stays in ps-server but changes from direct SSE forwarding to DB polling. The Restate handler writes events (tool calls, partial answers) to a `conversation_events` table. ps-server polls this table and streams to the client. This decouples the streaming frontend from the durable backend.

**Why not stream through Restate?** Restate journals are positional — streaming hundreds of SSE events through `ctx.run()` would create an enormous journal and break on any code change. The event stream is best-effort display data, not durable state.

**2. AgenticQueryHandler is a Restate Object (keyed by session_id)**

Using an Object (not Service) ensures at-most-one concurrent query per conversation. The session_id key prevents duplicate queries for the same conversation.

**3. Pod reaper becomes a Restate Service with durable self-scheduling**

Replace the `tokio::spawn` interval timer with a Restate handler that calls itself via `send_with_delay()` — the same pattern used for scheduled ingestion.

---

## Database Changes

### New table: `reasoning.conversation_events`

Ephemeral event log for streaming. Events are written by the Restate handler and read by ps-server's polling loop. Rows are deleted after the query completes (or after a TTL).

```sql
CREATE TABLE reasoning.conversation_events (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    conversation_id UUID NOT NULL REFERENCES reasoning.conversations(id) ON DELETE CASCADE,
    event_type  TEXT NOT NULL,  -- 'container_status', 'tool_call_started', 'tool_call_completed',
                               -- 'partial_answer', 'thinking', 'artifact_uploaded', 'final_answer', 'error'
    payload     JSONB NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conv_events_poll ON reasoning.conversation_events (conversation_id, id);
```

The `id` is a BIGINT identity (not UUID) for efficient cursor-based polling: `WHERE conversation_id = $1 AND id > $2 ORDER BY id`.

### Conversation status field additions

Add `query_status` to `reasoning.conversations`:

```sql
ALTER TABLE reasoning.conversations
    ADD COLUMN query_status TEXT NOT NULL DEFAULT 'idle';
    -- Values: 'idle', 'pending', 'running', 'completed', 'failed', 'cancelled'
```

---

## Implementation Steps

### Step 1: Database migration

**Files:**
| File | Action |
|------|--------|
| `migrations/0026_conversation_events.sql` | **Create** — events table + index, query_status column |

### Step 2: Conversation events repo methods

Add to `ps-core/src/repo/reasoning/conversations.rs`:

- `append_event(conversation_id, event_type, payload)` — insert event row
- `poll_events(conversation_id, after_id) -> Vec<ConversationEvent>` — cursor-based fetch
- `delete_events(conversation_id)` — cleanup after completion
- `update_query_status(conversation_id, status)` — lifecycle transitions

**Files:**
| File | Action |
|------|--------|
| `crates/ps-core/src/repo/reasoning/conversations.rs` | **Modify** — add event CRUD methods |

### Step 3: AgenticQueryHandler (Restate Object)

New handler in ps-workers. Keyed by `session_id` (conversation ID).

```rust
#[restate_sdk::object]
pub trait AgenticQueryHandler {
    /// Run an agentic query for a conversation.
    async fn run_query(request: AgenticQueryRequest) -> Result<(), TerminalError>;

    /// Cancel a running query.
    async fn cancel() -> Result<(), TerminalError>;
}
```

The `run_query` method follows this flow:

1. **`ctx.run("create_run")`** — create ingestion run record (idempotent, journaled)
2. **`ctx.run("ensure_pod")`** — call `ContainerManager::ensure_pod()`, return Pod IP (journaled)
3. **Poll for pod ready** — NOT journaled (best-effort, re-polls on replay)
4. **`ctx.run("create_session")`** — create/reuse OpenCode session (journaled)
5. **SSE event loop** — NOT journaled (streaming, re-executes on replay):
   - Subscribe to OpenCode SSE
   - Send question
   - For each event: write to `conversation_events` table
   - Intercept artifact uploads → `ctx.run("register_artifact")` (journaled)
6. **`ctx.run("store_message")`** — persist assistant message (journaled)
7. **`ctx.run("update_totals")`** — update conversation stats (journaled)
8. **`ctx.run("finalize")`** — write final_answer event, set query_status = 'completed' (journaled)

**Journaling rules:**
- DB writes (create_run, store_message, register_artifact, update_totals) → inside `ctx.run()`
- Pod creation → inside `ctx.run()` (idempotent, K8s handles duplicates)
- SSE streaming → outside `ctx.run()` (too large for journal, re-executes safely via upserts)
- Secret decryption → outside `ctx.run()` (journal security)

**Files:**
| File | Action |
|------|--------|
| `crates/ps-workers/src/handlers/agentic_query.rs` | **Create** — handler + cancel |
| `crates/ps-workers/src/handlers/mod.rs` | **Modify** — add `pub mod agentic_query` |
| `crates/ps-workers/src/main.rs` | **Modify** — instantiate + bind handler |

### Step 4: AgentPodReaperHandler (Restate Service)

Replace the `tokio::spawn` interval timer in ps-server's main.rs.

```rust
#[restate_sdk::service]
pub trait AgentPodReaperHandler {
    /// Reap idle/expired agent pods and schedule next run.
    async fn reap() -> Result<(), TerminalError>;
}
```

Flow:
1. `ctx.run("reap_pods")` — call `ContainerManager::reap_idle_pods()`
2. `ctx.run("cleanup_sessions")` — delete auth sessions for reaped pods
3. `ctx.service_client::<AgentPodReaperHandlerClient>().reap().send_with_delay(Duration::from_secs(60))` — schedule next run

**Files:**
| File | Action |
|------|--------|
| `crates/ps-workers/src/handlers/agent_reaper.rs` | **Create** |
| `crates/ps-workers/src/handlers/mod.rs` | **Modify** — add `pub mod agent_reaper` |
| `crates/ps-workers/src/main.rs` | **Modify** — instantiate + bind, seed first reap invocation |

### Step 5: Rewrite ps-server AskQuestion to poll-and-stream

Replace the current `ask_question()` in `ps-server/services/reasoning/agent_query.rs`:

1. Validate question, require auth
2. Create/resume conversation in DB
3. Set `query_status = 'pending'`
4. Fire-and-forget `AgenticQueryHandler.run_query()` via Restate
5. Return a gRPC server-stream that polls `conversation_events`:
   - Every 100ms, fetch new events since last cursor
   - Map each event to `AskQuestionResponse` proto
   - When `final_answer` or `error` event received, close stream
   - On client disconnect, fire `AgenticQueryHandler.cancel()`

This reduces `agent_query.rs` from ~460 lines to ~120 lines (thin adapter).

**Files:**
| File | Action |
|------|--------|
| `crates/ps-server/src/services/reasoning/agent_query.rs` | **Rewrite** — poll-based streaming |
| `crates/ps-server/src/main.rs` | **Modify** — remove pod reaper spawn, remove ContainerManager init (moves to ps-workers) |

### Step 6: Move ContainerManager + dependencies to ps-workers

The `ContainerManager` is now only used by ps-workers handlers (agentic query + reaper). Move its initialization from ps-server's main.rs to ps-workers' main.rs. Add it to `SharedState`.

```rust
pub struct SharedState {
    pub repos: Repos,
    pub secret_key: Zeroizing<[u8; 32]>,
    pub http_client: reqwest::Client,
    pub container_manager: Option<ContainerManager>,  // NEW
    pub artifact_store: Option<Arc<dyn ArtifactStore>>,  // NEW (for artifact registration)
}
```

ps-server's `ReasoningServiceImpl` no longer needs `container_manager` or direct `opencode_sdk` usage. It only needs `restate_url` to trigger handlers.

**Files:**
| File | Action |
|------|--------|
| `crates/ps-workers/src/handlers/mod.rs` | **Modify** — add fields to SharedState |
| `crates/ps-workers/src/main.rs` | **Modify** — init ContainerManager + ArtifactStore |
| `crates/ps-workers/Cargo.toml` | **Modify** — add `ps-agent`, `kube`, `k8s-openapi` deps |
| `crates/ps-server/Cargo.toml` | **Modify** — remove `ps-agent` dep (no longer needed) |
| `crates/ps-server/src/services/reasoning/mod.rs` | **Modify** — remove container_manager field |
| `crates/ps-server/src/main.rs` | **Modify** — remove ContainerManager + reaper setup |

### Step 7: Update psctl ask and frontend

The streaming protocol doesn't change (still gRPC server-stream with `AskQuestionResponse`), so frontend and psctl need no changes. The only difference is that events may arrive slightly slower (100ms polling vs direct SSE forwarding), which is imperceptible.

### Step 8: Tests

| Level | Test |
|-------|------|
| Repo | `conversation_events` append, poll with cursor, delete |
| Repo | `query_status` transitions |
| API | `AskQuestion` with mock Restate (verify trigger + poll loop) |
| Handler | `AgenticQueryHandler.run_query` with mock ContainerManager + OpenCode |
| Handler | `AgentPodReaperHandler.reap` with mock ContainerManager |
| E2E | Full flow: question → handler → pod → OpenCode → events → stream |

---

## Dependency Changes

**Before:**
```
ps-server → ps-agent (ContainerManager, opencode_sdk)
ps-workers (no agent deps)
```

**After:**
```
ps-server (no ps-agent dep — triggers via Restate)
ps-workers → ps-agent (ContainerManager, opencode_sdk)
```

---

## Implementation Order

```
Week 1: Foundation
  ├─ Step 1: Database migration (conversation_events, query_status)
  ├─ Step 2: Repo methods for event CRUD
  └─ Step 4: AgentPodReaperHandler (simplest handler, validates pattern)

Week 2: Core Handler
  ├─ Step 3: AgenticQueryHandler (main orchestration)
  └─ Step 6: Move ContainerManager to ps-workers

Week 3: Integration
  ├─ Step 5: Rewrite ps-server AskQuestion to poll-and-stream
  ├─ Step 7: Verify frontend + psctl unchanged
  └─ Step 8: Tests
```

---

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Polling latency perceptible (100ms) | Low | Minor UX delay | Tune interval; 50ms if needed. DB index ensures fast cursor reads. |
| Journal compatibility on handler changes | Medium | Breaks in-flight queries | Cancel all invocations before deploying handler changes (same as ingestion). |
| Event table growth | Low | Disk usage | Delete events on query completion. Add TTL cleanup job (24h) as fallback. |
| Restate unavailable blocks queries | Low | Complete outage | Same risk as ingestion — Restate is already a hard dependency. |
| SSE replay on Restate retry | Medium | Duplicate tool calls shown | Events use DB upserts (idempotent). Frontend deduplicates by event ID. |

---

## Exit Criteria

- [ ] `AgenticQueryHandler` runs in ps-workers with Restate journal
- [ ] `AgentPodReaperHandler` replaces tokio::spawn timer
- [ ] ps-server `ask_question()` is a thin poll-and-stream adapter (~120 lines)
- [ ] ps-server no longer depends on ps-agent
- [ ] ContainerManager + opencode_sdk live exclusively in ps-workers
- [ ] In-flight queries survive ps-server restart
- [ ] `psctl ask` and frontend `/ask` work unchanged
- [ ] `conversation_events` cleaned up after query completion
- [ ] Cancellation works: client disconnect → handler cancel
- [ ] `prek run -av` passes with zero warnings
