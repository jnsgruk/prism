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
| `crates/ps-core/src/repo/reasoning/conversations.rs` | **Modify** — add event CRUD methods + `ConversationEvent` struct |

**Testing:** 6 repo tests — see [Step 2 testing detail](#step-2-conversation-events-repo-define_repo_test) in the Testing Strategy section.

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

**Testing:** 5 handler tests with wiremock OpenCode — see [Step 3 testing detail](#step-3-agenticqueryhandler-define_source_test--wiremock) in the Testing Strategy section. The handler's core orchestration logic (SSE parsing, event writing, artifact interception) is extracted into a testable function separate from the `ctx.run()` wrappers.

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

**Testing:** 1 unit test — see [Step 4 testing detail](#step-4-agentpodreaperhandler-unit-test) in the Testing Strategy section.

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

**Testing:** 6 API tests — see [Step 5 testing detail](#step-5-ps-server-poll-and-stream-define_api_test) in the Testing Strategy section. Tests seed `conversation_events` directly in DB, bypassing Restate, and verify the poll-stream adapter emits correct proto events.

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

Testing is distributed across steps — each step includes its own tests (see per-step testing sections below). This section provides the overall strategy and test matrix.

---

## Testing Strategy

### Approach

Tests follow the same infrastructure as the rest of the codebase:

- **Repo tests** use `define_repo_test!` — real PostgreSQL (testcontainers), no gRPC server
- **API tests** use `define_api_test!` — full gRPC server + real PostgreSQL
- **Handler tests** use `define_source_test!` — real PostgreSQL + wiremock for HTTP mocking (OpenCode API)
- **Frontend tests** use vitest + happy-dom with mocked Connect transport

The Restate SDK context (`ctx`) is **not directly testable** in integration tests — Restate handlers are tested by exercising the logic they call, not the journaling framework. This matches the existing pattern: ingestion handler tests verify source adapters (fetch/store/watermark), not the `ctx.run()` wrappers.

### Test Matrix

| Step | Level | Test | Macro / Tool | Count |
|------|-------|------|-------------|-------|
| 2 | Repo | `conversation_events_append_and_poll` — append 5 events, poll with cursor=0 returns all, poll with cursor=3 returns last 2 | `define_repo_test!` | 1 |
| 2 | Repo | `conversation_events_poll_empty` — poll on conversation with no events returns empty vec | `define_repo_test!` | 1 |
| 2 | Repo | `conversation_events_delete` — append events, delete, poll returns empty | `define_repo_test!` | 1 |
| 2 | Repo | `conversation_events_cursor_ordering` — events returned in insertion order (by BIGINT id, not timestamp) | `define_repo_test!` | 1 |
| 2 | Repo | `query_status_transitions` — create conversation (idle), update to pending, running, completed; verify each state persists | `define_repo_test!` | 1 |
| 2 | Repo | `query_status_cancel` — running → cancelled transition works | `define_repo_test!` | 1 |
| 3 | Handler | `agentic_query_writes_events` — call handler orchestration logic (extracted as testable fn) with wiremock OpenCode, verify events written to DB | `define_source_test!` | 1 |
| 3 | Handler | `agentic_query_stores_message_on_completion` — verify assistant message created in DB after SSE stream completes | `define_source_test!` | 1 |
| 3 | Handler | `agentic_query_handles_opencode_error` — OpenCode returns error event, verify query_status set to 'failed' and error event written | `define_source_test!` | 1 |
| 3 | Handler | `agentic_query_intercepts_artifact_upload` — tool_call_completed for `prism_upload_artifact` triggers artifact DB record | `define_source_test!` | 1 |
| 3 | Handler | `agentic_query_updates_totals` — verify conversation totals (tool_calls, tokens, cost) updated after completion | `define_source_test!` | 1 |
| 4 | Unit | `reaper_deletes_expired_pods` — inline `#[cfg(test)]` with mock ContainerManager trait, verify reap call + session cleanup | unit test | 1 |
| 5 | API | `ask_question_triggers_and_polls` — create conversation + events directly in DB, call AskQuestion RPC, verify events streamed back in order | `define_api_test!` | 1 |
| 5 | API | `ask_question_streams_final_answer` — seed DB with events including final_answer, verify stream closes after final_answer received | `define_api_test!` | 1 |
| 5 | API | `ask_question_streams_error` — seed DB with error event, verify error proto emitted and stream closes | `define_api_test!` | 1 |
| 5 | API | `ask_question_validates_empty_question` — empty question returns InvalidArgument | `define_api_test!` | 1 |
| 5 | API | `ask_question_validates_long_question` — >4000 char question returns InvalidArgument | `define_api_test!` | 1 |
| 5 | API | `ask_question_requires_auth` — unauthenticated request returns Unauthenticated | `define_api_test!` | 1 |
| 5 | Frontend | `useAskQuestion` state transitions remain unchanged — existing tests continue to pass | vitest | 0 (existing) |
| 5 | Frontend | `psctl ask` output formatting — existing tests continue to pass | cargo test | 0 (existing) |
| **Total** | | | | **18 new** |

### Per-Step Testing Detail

#### Step 2: Conversation Events Repo (`define_repo_test!`)

Six repo-level tests validating the new `conversation_events` table and `query_status` column.

**Pattern:**
```rust
define_repo_test!(conversation_events_append_and_poll, |repos, pool| async move {
    let user_id = insert_user(&pool).await;
    let conv = repos.reasoning.create_conversation(&CreateConversationParams {
        user_id, title: Some("test"), model_name: "test",
    }).await.unwrap();

    // Append events
    repos.reasoning.append_event(conv.id, "container_status",
        &serde_json::json!({"status": "creating", "message": "Starting..."})).await.unwrap();
    repos.reasoning.append_event(conv.id, "tool_call_started",
        &serde_json::json!({"tool_name": "list_teams", "arguments_json": "{}"})).await.unwrap();

    // Poll from start
    let events = repos.reasoning.poll_events(conv.id, 0).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "container_status");
    assert_eq!(events[1].event_type, "tool_call_started");

    // Poll from cursor (after first event)
    let events = repos.reasoning.poll_events(conv.id, events[0].id).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "tool_call_started");
});
```

**Files:**
| File | Action |
|------|--------|
| `tests/integration/src/repo/reasoning.rs` | **Modify** — add 6 tests after existing conversation tests |

#### Step 3: AgenticQueryHandler (`define_source_test!` + wiremock)

Five handler-level tests that exercise the orchestration logic. The handler's core logic is extracted into a testable function that takes dependencies as parameters (repos, HTTP client, config) — the Restate `ctx.run()` wrappers are thin and not tested directly, matching the existing ingestion pattern.

**OpenCode mocking strategy:** Use `wiremock::MockServer` to simulate OpenCode's HTTP + SSE API:

- **Session creation:** `POST /sessions` → returns `{"id": "sess-1"}`
- **Send message:** `POST /sessions/sess-1/messages` → returns `202`
- **SSE subscription:** `GET /events` → returns SSE stream with:
  - `event: message.part.updated` (text/tool/reasoning parts)
  - `event: session.idle` (signals completion)
  - `event: session.error` (signals failure)

The wiremock mock returns pre-built SSE payloads matching OpenCode's event format. This validates that the handler correctly:
1. Parses OpenCode events
2. Writes corresponding `conversation_events` rows
3. Intercepts artifact uploads
4. Stores the final message
5. Updates conversation totals

**Note:** Pod creation (`ContainerManager::ensure_pod()`) is **not** called in these tests — the handler logic is tested with a pre-existing "pod IP" (the wiremock server URL). ContainerManager has its own unit tests in `ps-agent/pod_spec.rs` (9 existing tests).

**Pattern:**
```rust
define_source_test!(agentic_query_writes_events, |ctx| async move {
    let user_id = insert_user(&ctx.pool).await;
    let conv = ctx.repos.reasoning.create_conversation(&CreateConversationParams {
        user_id, title: Some("test query"), model_name: "test-model",
    }).await.unwrap();

    // Mock OpenCode session creation
    Mock::given(method("POST")).and(path("/sessions"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({"id": "sess-1"})))
        .mount(&ctx.mock_server).await;

    // Mock SSE stream with tool call + final answer
    let sse_body = build_sse_stream(&[
        sse_tool_pending("mcp_prism_list_teams", "{}"),
        sse_tool_completed("mcp_prism_list_teams", "3 teams found"),
        sse_text_part("The team has 42 members."),
        sse_session_idle(),
    ]);
    Mock::given(method("GET")).and(path("/events"))
        .respond_with(ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(sse_body))
        .mount(&ctx.mock_server).await;

    // Mock send message
    Mock::given(method("POST")).and(path_regex("/sessions/.*/messages"))
        .respond_with(ResponseTemplate::new(202))
        .mount(&ctx.mock_server).await;

    // Run the handler's core logic (extracted from ctx.run wrappers)
    let pod_url = ctx.mock_server.uri();
    run_agentic_query_core(&ctx.repos, &ctx.http_client, conv.id, &pod_url,
        "sess-1", "How many team members?").await.unwrap();

    // Verify events written
    let events = ctx.repos.reasoning.poll_events(conv.id, 0).await.unwrap();
    assert!(events.iter().any(|e| e.event_type == "tool_call_started"));
    assert!(events.iter().any(|e| e.event_type == "tool_call_completed"));
    assert!(events.iter().any(|e| e.event_type == "final_answer"));

    // Verify message stored
    let messages = ctx.repos.reasoning.list_messages(conv.id).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, "assistant");
    assert!(messages[0].content.contains("42 members"));
});
```

**SSE helper functions** (new file `tests/integration/src/common/opencode_helpers.rs`):

```rust
/// Build an SSE response body from a sequence of events.
pub fn build_sse_stream(events: &[String]) -> String { ... }

/// SSE event: tool call pending (maps to ToolCallStarted proto)
pub fn sse_tool_pending(tool_name: &str, input: &str) -> String { ... }

/// SSE event: tool call completed (maps to ToolCallCompleted proto)
pub fn sse_tool_completed(tool_name: &str, output: &str) -> String { ... }

/// SSE event: text part (maps to PartialAnswer proto)
pub fn sse_text_part(text: &str) -> String { ... }

/// SSE event: session idle (signals query complete)
pub fn sse_session_idle() -> String { ... }

/// SSE event: session error
pub fn sse_session_error(message: &str) -> String { ... }
```

**Files:**
| File | Action |
|------|--------|
| `tests/integration/src/source/agentic_query.rs` | **Create** — 5 handler tests |
| `tests/integration/src/source/mod.rs` | **Modify** — add `mod agentic_query` |
| `tests/integration/src/common/opencode_helpers.rs` | **Create** — SSE response builders |
| `tests/integration/src/common/mod.rs` | **Modify** — add `pub mod opencode_helpers` |

#### Step 4: AgentPodReaperHandler (unit test)

One unit test using a mock `ContainerManager`. Since `ContainerManager` takes a `kube::Client` which is hard to mock, the reaper logic is tested by extracting the orchestration into a function that takes a trait:

```rust
#[cfg(test)]
mod tests {
    // Test that reap() calls reap_idle_pods and returns reaped session IDs
    #[tokio::test]
    async fn reaper_deletes_expired_pods() {
        // Uses a mock that returns 2 reaped token session IDs
        // Verifies the handler would call delete_session for each
    }
}
```

**Files:**
| File | Action |
|------|--------|
| `crates/ps-workers/src/handlers/agent_reaper.rs` | Inline `#[cfg(test)]` module |

#### Step 5: ps-server Poll-and-Stream (`define_api_test!`)

Six API-level tests validating the rewritten `ask_question()` thin adapter. These tests bypass Restate entirely — they seed `conversation_events` directly in the DB (simulating what the handler would write), then call the gRPC `AskQuestion` RPC and verify the poll-and-stream loop emits the correct proto events.

**Why this works:** The poll-and-stream adapter reads from `conversation_events` regardless of who wrote them. By seeding events directly, we test the streaming logic in isolation without needing a running Restate cluster or agent container.

**Pattern:**
```rust
define_api_test!(ask_question_triggers_and_polls, |server| async move {
    let (user_id, token) = create_admin_user(&server.pool).await;
    let repos = Repos::new(server.pool.clone());

    // Create conversation and seed events (simulating handler output)
    let conv = repos.reasoning.create_conversation(&CreateConversationParams {
        user_id, title: Some("test"), model_name: "test",
    }).await.unwrap();
    repos.reasoning.update_query_status(conv.id, "running").await.unwrap();

    repos.reasoning.append_event(conv.id, "container_status",
        &json!({"status": "ready", "message": "Agent ready"})).await.unwrap();
    repos.reasoning.append_event(conv.id, "tool_call_started",
        &json!({"tool_name": "list_teams", "arguments_json": "{}"})).await.unwrap();
    repos.reasoning.append_event(conv.id, "final_answer",
        &json!({"answer": "There are 5 teams.", "conversation_id": conv.id.to_string(),
                 "tool_call_count": 1, "duration_ms": 2500})).await.unwrap();

    // Call AskQuestion RPC with the existing conversation ID
    let mut client = ReasoningServiceClient::new(server.channel.clone());
    let mut req = Request::new(AskQuestionRequest {
        question: "How many teams?".into(),
        conversation_id: Some(conv.id.to_string()),
    });
    auth(&mut req, &token);

    let resp = client.ask_question(req).await.expect("ask_question");
    let mut stream = resp.into_inner();

    // Collect all events
    let mut events = vec![];
    while let Some(msg) = stream.message().await.unwrap() {
        events.push(msg);
    }

    // Verify event sequence
    assert!(events.len() >= 3);
    // First event: container_status
    assert!(matches!(events[0].event.as_ref().unwrap(),
        ask_question_response::Event::ContainerStatus(_)));
    // Last event: final_answer
    assert!(matches!(events.last().unwrap().event.as_ref().unwrap(),
        ask_question_response::Event::FinalAnswer(_)));
});
```

**Note:** The `TestServer` in `tests/integration/src/common/server.rs` sets `restate_url` to `http://127.0.0.1:1` (dummy, connection-refused). The rewritten `ask_question()` must handle the Restate trigger failure gracefully in tests — either by checking if the handler is already running (events already seeded), or by making the trigger best-effort with the poll loop as the primary mechanism.

**Files:**
| File | Action |
|------|--------|
| `tests/integration/src/api/reasoning.rs` | **Modify** — add 6 tests |

### Test Infrastructure Changes

| File | Action | Purpose |
|------|--------|---------|
| `tests/integration/src/common/opencode_helpers.rs` | **Create** | SSE response builders for wiremock (tool events, text parts, session lifecycle) |
| `tests/integration/src/common/mod.rs` | **Modify** | Add `pub mod opencode_helpers` |
| `tests/integration/src/source/agentic_query.rs` | **Create** | Handler-level tests with wiremock OpenCode |
| `tests/integration/src/source/mod.rs` | **Modify** | Add `mod agentic_query` |

### What Is NOT Tested (and why)

| Concern | Reason |
|---------|--------|
| Restate `ctx.run()` journaling | Restate SDK correctness is upstream's responsibility; we test the logic inside closures, not the journal |
| Actual K8s Pod creation | Requires a live K8s cluster; covered by manual E2E testing and `ps-agent/pod_spec.rs` unit tests (9 existing) |
| OpenCode server behaviour | OpenCode is a third-party tool; wiremock validates our SSE parsing, not OpenCode's behaviour |
| gRPC streaming backpressure | Difficult to test deterministically; covered by manual load testing |
| Multi-instance ps-server polling | Requires multiple server instances; architectural guarantee (DB polling is inherently multi-reader safe) |

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
- [ ] 6 repo tests for conversation_events and query_status
- [ ] 5 handler tests with wiremock OpenCode (events, messages, artifacts, errors, totals)
- [ ] 6 API tests for poll-and-stream adapter (trigger, stream, final, error, validation, auth)
- [ ] 1 unit test for pod reaper logic
- [ ] Existing frontend + psctl tests continue to pass unchanged
- [ ] `prek run -av` passes with zero warnings
