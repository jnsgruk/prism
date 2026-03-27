# 58 — Deterministic Thinking Token & Tool Call Ordering

**Status:** Draft
**Created:** 2026-03-27
**Depends on:** plans/56, plans/57

## Problem Statement

Thinking tokens (reasoning steps) and tool calls in the agentic chat interface suffer from three categories of bugs:

1. **Disappearing/reordering during streaming** — thinking tokens appear, then vanish when tool calls arrive, because the frontend splices and reorders the mutable `steps[]` array
2. **Historical mismatch** — the trace stored in `conversation_messages.reasoning_trace` is built with different logic than the live stream, so refreshing a page can show a different ordering than what was displayed during streaming
3. **No reconnection** — navigating away from a chat with an in-flight request means the streaming state is lost; returning shows only historical messages, not the live stream

These have been partially patched (commits `bf5b293`, `fa0e797`, `d965df6`, `1bf3fd3`) but the root causes remain.

## Root Cause Analysis

### Problem 1: Mutable array with dual-purpose indexing

The frontend accumulates steps into a mutable `steps[]` array inside the `ask()` closure (`use-ask-question.ts`). Thinking events use `part_index` (from the LLM) to find and update existing entries. Tool calls append. When a thinking block is interleaved with tool calls, the code *splices* the thinking entry from its original position and *pushes it to the end* (line 234-236). This causes:

- React sees a different array shape → components unmount/remount
- Previously-visible thinking blocks move position in the UI
- The `part_index` recycling detection (cumulative text comparison) is fragile and fails when the LLM changes its thinking mid-stream

**The fundamental issue:** `part_index` conflates two concepts — "which LLM reasoning block am I updating" and "where should this appear in the UI". These need to be separate.

### Problem 2: Worker-side trace diverges from event stream

The worker builds `trace_steps` by updating the last entry in-place when `part_index` matches (lines 588-603 of `agentic_query.rs`). But the event stream writes every intermediate thinking update to `conversation_events`. The frontend builds its own ordering from these events. The final `reasoning_trace` JSONB stored in the message uses the worker's `trace_steps`, which may have a different structure than what the frontend displayed.

For example:
- Worker: `[reasoning(pi=0), tool(A), reasoning(pi=1)]` — clean, collapsed
- Events: `[thinking(pi=0, "I think"), thinking(pi=0, "I think we should"), tool_started(A), thinking(pi=0, "I think we should query"), tool_completed(A), thinking(pi=1, "Now let")]` — many intermediate updates
- Frontend tries to reconstruct the worker's collapsed view from the event stream, and sometimes gets it wrong

### Problem 3: Streaming state is ephemeral and connection-bound

The `useAskQuestion` hook holds all streaming state in React state (`useState`). The gRPC stream is tied to the browser tab's connection. When navigating away:
- The `for await` loop stops (component unmounts, abort fires)
- State resets to `idle`
- Returning loads historical messages (which may not have the assistant response yet if the worker hasn't finished)
- Even if the worker is still running and writing events, there's no mechanism to reconnect to the event stream

The server-side poll loop (in `agent_query.rs`) dies when the client disconnects (`tx.send()` fails), and events continue to accumulate in the DB with no consumer.

## Design

### Core Principle: The database event log is the single source of truth

Instead of building streaming state client-side from a single gRPC connection, the frontend should be able to reconstruct the full ordered step list at any time by reading the event log. This makes streaming, reconnection, and historical display all use the same code path.

### Architecture Changes

```
Current:
  Worker → events DB → poll loop → gRPC stream → frontend accumulates in useState
  Worker → trace_steps (separate logic) → conversation_messages.reasoning_trace

Proposed:
  Worker → events DB (with step_id + step_seq on every event)
       ↕
  Frontend polls events → derives ordered step list (single pure derivation function)
  Worker → stores final trace from events (not separate accumulation)
```

---

## Detailed Implementation Plan

### Phase 1: Database — stable identity columns on events

**Goal:** Every event gets a server-assigned `step_id` (stable identity) and `step_seq` (display ordering) so the frontend never needs to guess.

#### Step 1.1: Migration

**File:** `migrations/NNNN_event_step_identity.sql` (new)

```sql
-- Add server-assigned step identity and display ordering to conversation events.
-- step_id: stable identity for the logical step (e.g. "think-0-0", "tool-abc123")
-- step_seq: monotonically increasing display order within a conversation
ALTER TABLE reasoning.conversation_events
  ADD COLUMN step_id TEXT,
  ADD COLUMN step_seq INT;

-- Optimise resume queries that filter by conversation + sort by step_seq.
CREATE INDEX idx_conversation_events_step_seq
  ON reasoning.conversation_events (conversation_id, step_seq)
  WHERE step_seq IS NOT NULL;
```

Both columns are nullable for backward compatibility with any in-flight events from a previous deployment. The frontend and server code will handle `NULL` gracefully (fall back to `id` ordering).

#### Step 1.2: Update `ConversationEvent` struct

**File:** `crates/ps-core/src/repo/reasoning/conversations.rs`

Add to the `ConversationEvent` struct (line 31):

```rust
pub struct ConversationEvent {
    pub id: i64,
    pub conversation_id: Uuid,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub step_id: Option<String>,    // NEW
    pub step_seq: Option<i32>,      // NEW
    pub created_at: OffsetDateTime,
}
```

#### Step 1.3: Update `append_event` to accept step identity

**File:** `crates/ps-core/src/repo/reasoning/conversations.rs`

Change the `append_event` method (line 264) to accept optional `step_id` and `step_seq`:

```rust
pub async fn append_event(
    &self,
    conversation_id: Uuid,
    event_type: &str,
    payload: &serde_json::Value,
    step_id: Option<&str>,      // NEW
    step_seq: Option<i32>,      // NEW
) -> Result<ConversationEvent, Error> {
    let row = sqlx::query_as!(
        ConversationEvent,
        r#"
        INSERT INTO reasoning.conversation_events
          (conversation_id, event_type, payload, step_id, step_seq)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, conversation_id, event_type, payload, step_id, step_seq, created_at
        "#,
        conversation_id,
        event_type,
        payload,
        step_id,
        step_seq,
    )
    .fetch_one(&self.pool)
    .await?;
    Ok(row)
}
```

Update the `poll_events` query (line 287) to include the new columns in its SELECT:

```rust
pub async fn poll_events(
    &self,
    conversation_id: Uuid,
    after_id: i64,
) -> Result<Vec<ConversationEvent>, Error> {
    let rows = sqlx::query_as!(
        ConversationEvent,
        r#"
        SELECT id, conversation_id, event_type, payload, step_id, step_seq, created_at
        FROM reasoning.conversation_events
        WHERE conversation_id = $1 AND id > $2
        ORDER BY id
        "#,
        conversation_id,
        after_id,
    )
    .fetch_all(&self.pool)
    .await?;
    Ok(rows)
}
```

#### Step 1.4: Add `get_all_events` method for trace derivation

**File:** `crates/ps-core/src/repo/reasoning/conversations.rs`

Add a new method that returns all events for a conversation, for deriving the final trace:

```rust
/// Return all events for a conversation, ordered by insertion.
/// Used by the worker to derive the final reasoning trace.
pub async fn get_all_events(
    &self,
    conversation_id: Uuid,
) -> Result<Vec<ConversationEvent>, Error> {
    self.poll_events(conversation_id, 0).await
}
```

#### Step 1.5: Update sqlx query cache

Run `cargo sqlx prepare --workspace` and commit `.sqlx/` changes separately.

**Testing (Phase 1):**

**File:** `tests/integration/src/repo/reasoning.rs`

Add test `conversation_events_step_identity`:
- Append events with `step_id` and `step_seq` values
- Poll and verify the columns are returned correctly
- Append events with `NULL` step_id/step_seq (backward compat)
- Verify ordering is still by `id`

---

### Phase 2: Proto — add step identity fields and ResumeStream RPC

**Goal:** Wire `step_id`/`step_seq` through the proto layer and add the `ResumeStream` RPC.

#### Step 2.1: Add fields to existing proto messages

**File:** `proto/canonical/prism/v1/reasoning.proto`

Update `AgentThinking` (line 409):
```protobuf
message AgentThinking {
  string text = 1;
  int32 part_index = 2;
  // Server-assigned stable identity for this reasoning block.
  string step_id = 3;
  // Server-assigned display order (monotonically increasing per conversation).
  int32 step_seq = 4;
}
```

Update `AgentToolCallStarted` (line 380):
```protobuf
message AgentToolCallStarted {
  string tool_name = 1;
  string arguments_json = 2;
  string call_id = 3;
  string step_id = 4;
  int32 step_seq = 5;
}
```

Update `AgentToolCallCompleted` (line 390):
```protobuf
message AgentToolCallCompleted {
  string tool_name = 1;
  string result_summary = 2;
  int32 duration_ms = 3;
  bool success = 4;
  string call_id = 5;
  string step_id = 6;
  int32 step_seq = 7;
}
```

#### Step 2.2: Add `ResumeStream` RPC and message

**File:** `proto/canonical/prism/v1/reasoning.proto`

Add to the `ReasoningService` (after line 57):
```protobuf
// ResumeStream reconnects to an in-flight agentic query, replaying all
// events from last_event_id (0 = start). Returns the same event stream
// as AskQuestion. Terminates when the query completes or is cancelled.
rpc ResumeStream(ResumeStreamRequest) returns (stream AskQuestionResponse);
```

Add message definition (after `AskQuestionRequest`):
```protobuf
message ResumeStreamRequest {
  string conversation_id = 1;
  // Resume from this event cursor. 0 replays all events from the start.
  int64 last_event_id = 2;
}
```

#### Step 2.3: Regenerate code

```bash
buf lint && buf generate
```

This regenerates:
- `crates/ps-proto/src/gen/` (Rust)
- `frontend/lib/api/gen/` (TypeScript)

**Testing (Phase 2):**

No new tests — proto changes are structural. Verified by compilation after `buf generate`.

---

### Phase 3: Worker — StepRegistry and server-side ordering

**Goal:** Move all step identity and ordering logic to the worker. Remove `trace_steps` accumulation; derive trace from events.

#### Step 3.1: Create `StepRegistry`

**File:** `crates/ps-workers/src/handlers/step_registry.rs` (new)

```rust
use std::collections::HashMap;

/// Assigns stable identities and display ordering to agentic query steps.
///
/// Each logical step (reasoning block or tool call) gets a unique `step_id`
/// and a monotonically increasing `step_seq`. Updates to existing steps
/// (cumulative thinking text, tool completion) reuse the original identity.
pub struct StepRegistry {
    next_seq: i32,
    /// Maps step_id → step_seq for active steps.
    steps: HashMap<String, i32>,
    /// Tracks the current generation and text prefix per part_index,
    /// so we can detect when OpenCode recycles a part_index for a new block.
    thinking: HashMap<i32, ThinkingState>,
}

struct ThinkingState {
    generation: u32,
    /// First 80 chars of the thinking text, used to detect continuations
    /// vs recycled part_index values.
    text_prefix: String,
}

/// Identity assigned to an event.
pub struct StepIdentity {
    pub step_id: String,
    pub step_seq: i32,
}

impl StepRegistry {
    pub fn new() -> Self {
        Self {
            next_seq: 0,
            steps: HashMap::new(),
            thinking: HashMap::new(),
        }
    }

    /// Assign identity for a thinking event. Returns existing identity if
    /// this is a cumulative update to the same reasoning block, or creates
    /// a new step if the part_index has been recycled.
    pub fn thinking_step(&mut self, part_index: i32, text: &str) -> StepIdentity {
        if let Some(state) = self.thinking.get(&part_index) {
            let is_continuation = text.starts_with(&state.text_prefix)
                || state.text_prefix.starts_with(text);

            if is_continuation {
                let step_id = format!("think-{part_index}-{}", state.generation);
                let step_seq = self.steps[&step_id];
                // Update prefix to latest text (in case it grew)
                if text.len() > state.text_prefix.len() {
                    self.thinking.get_mut(&part_index).unwrap().text_prefix =
                        text.chars().take(80).collect();
                }
                return StepIdentity { step_id, step_seq };
            }
            // Recycled part_index — fall through to create new generation
        }

        let generation = self
            .thinking
            .get(&part_index)
            .map(|s| s.generation + 1)
            .unwrap_or(0);

        self.thinking.insert(
            part_index,
            ThinkingState {
                generation,
                text_prefix: text.chars().take(80).collect(),
            },
        );

        let step_id = format!("think-{part_index}-{generation}");
        let step_seq = self.next_seq;
        self.next_seq += 1;
        self.steps.insert(step_id.clone(), step_seq);
        StepIdentity { step_id, step_seq }
    }

    /// Assign identity for a tool call started event.
    pub fn tool_started(&mut self, call_id: &str) -> StepIdentity {
        let step_id = format!("tool-{call_id}");
        let step_seq = self.next_seq;
        self.next_seq += 1;
        self.steps.insert(step_id.clone(), step_seq);
        StepIdentity { step_id, step_seq }
    }

    /// Assign identity for a tool call completed event.
    /// Reuses the step_seq from the started event if it exists.
    pub fn tool_completed(&mut self, call_id: &str) -> StepIdentity {
        let step_id = format!("tool-{call_id}");
        if let Some(&step_seq) = self.steps.get(&step_id) {
            StepIdentity { step_id, step_seq }
        } else {
            // Started event was missed — assign new seq
            let step_seq = self.next_seq;
            self.next_seq += 1;
            self.steps.insert(step_id.clone(), step_seq);
            StepIdentity { step_id, step_seq }
        }
    }
}
```

**Export:** Add `pub mod step_registry;` to `crates/ps-workers/src/handlers/mod.rs`.

#### Step 3.2: Update the worker event loop

**File:** `crates/ps-workers/src/handlers/agentic_query.rs`

In `run_agentic_query_core()` (line 392):

**Remove:** The `trace_steps: Vec<serde_json::Value>` accumulation (line 444) and all the thinking/tool trace_steps logic (lines 550-612).

**Add:** A `StepRegistry` instance and pass identity to every `append_event` call.

Changes to the event loop (starting at line 528):

```rust
use super::step_registry::StepRegistry;

// Replace: let mut trace_steps: Vec<serde_json::Value> = Vec::new();
let mut registry = StepRegistry::new();

// ... inside the match on evt:

Event::ToolCallStarted(s) => {
    let identity = registry.tool_started(&s.call_id);
    let _ = repos
        .reasoning
        .append_event(
            conversation_id,
            "tool_call_started",
            &serde_json::json!({
                "tool_name": s.tool_name,
                "arguments_json": s.arguments_json,
                "call_id": s.call_id,
            }),
            Some(&identity.step_id),
            Some(identity.step_seq),
        )
        .await;
}
Event::ToolCallCompleted(c) => {
    tool_calls += 1;
    let identity = registry.tool_completed(&c.call_id);
    let _ = repos
        .reasoning
        .append_event(
            conversation_id,
            "tool_call_completed",
            &serde_json::json!({
                "tool_name": c.tool_name,
                "result_summary": c.result_summary,
                "duration_ms": c.duration_ms,
                "success": c.success,
                "call_id": c.call_id,
            }),
            Some(&identity.step_id),
            Some(identity.step_seq),
        )
        .await;
}
Event::PartialAnswer(a) => {
    answer_text.clone_from(&a.text);
    let _ = repos
        .reasoning
        .append_event(
            conversation_id,
            "partial_answer",
            &serde_json::json!({"text": a.text}),
            None, // No step identity for partial answers
            None,
        )
        .await;
}
Event::Thinking(t) => {
    let identity = registry.thinking_step(t.part_index, &t.text);
    let _ = repos
        .reasoning
        .append_event(
            conversation_id,
            "thinking",
            &serde_json::json!({
                "text": t.text,
                "part_index": t.part_index,
            }),
            Some(&identity.step_id),
            Some(identity.step_seq),
        )
        .await;
}
Event::Error(e) => {
    let _ = repos
        .reasoning
        .append_event(
            conversation_id,
            "error",
            &serde_json::json!({
                "message": e.message,
                "retryable": e.retryable,
            }),
            None,
            None,
        )
        .await;
}
```

#### Step 3.3: Derive trace from events instead of accumulating separately

**File:** `crates/ps-workers/src/handlers/agentic_query.rs`

Add a function to derive the final trace from events:

```rust
/// Derive the reasoning trace from conversation events.
/// This produces the same structure as the frontend's `deriveSteps()`.
fn derive_trace_from_events(events: &[ConversationEvent]) -> Vec<serde_json::Value> {
    use std::collections::BTreeMap;

    // BTreeMap keyed by step_seq for deterministic ordering
    let mut steps: BTreeMap<i32, serde_json::Value> = BTreeMap::new();

    for event in events {
        let Some(step_seq) = event.step_seq else {
            continue;
        };
        let Some(ref step_id) = event.step_id else {
            continue;
        };

        match event.event_type.as_str() {
            "thinking" => {
                let text = event.payload.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let part_index = event.payload.get("part_index").and_then(|v| v.as_i64()).unwrap_or(0);
                // Always overwrite — later events have more complete text
                steps.insert(step_seq, serde_json::json!({
                    "kind": "reasoning",
                    "text": text,
                    "part_index": part_index,
                    "step_id": step_id,
                }));
            }
            "tool_call_started" => {
                let tool_name = event.payload.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                let call_id = event.payload.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                let args = event.payload.get("arguments_json").and_then(|v| v.as_str()).unwrap_or("{}");
                steps.entry(step_seq).or_insert_with(|| serde_json::json!({
                    "kind": "tool",
                    "tool_name": tool_name,
                    "call_id": call_id,
                    "arguments": args,
                    "step_id": step_id,
                }));
            }
            "tool_call_completed" => {
                if let Some(step) = steps.get_mut(&step_seq) {
                    if let Some(obj) = step.as_object_mut() {
                        obj.insert("result_summary".into(),
                            event.payload.get("result_summary").cloned().unwrap_or_default());
                        obj.insert("duration_ms".into(),
                            event.payload.get("duration_ms").cloned().unwrap_or_default());
                        obj.insert("success".into(),
                            event.payload.get("success").cloned().unwrap_or(serde_json::Value::Bool(true)));
                    }
                }
            }
            _ => {}
        }
    }

    steps.into_values().collect()
}
```

Update `QueryResult` to remove `trace_steps` and instead return a marker that the trace should be derived:

Actually, simpler: keep `QueryResult` but populate `trace_steps` by calling `derive_trace_from_events` after the event loop:

```rust
// After the event loop, before returning QueryResult:
let all_events = repos.reasoning.get_all_events(conversation_id).await.unwrap_or_default();
let trace_steps = derive_trace_from_events(&all_events);
```

This replaces the manually-accumulated `trace_steps` vec entirely.

#### Step 3.4: Move event cleanup to after message persistence

**File:** `crates/ps-workers/src/handlers/agentic_query.rs`

In `run_query()`, after the `store_message` ctx.run block (around line 251), add event deletion:

```rust
// Clean up ephemeral events after message is persisted.
{
    let repos = self.state.repos.clone();
    let cid = conv_id;
    ctx.run(move || {
        let repos = repos.clone();
        async move {
            let _ = repos.reasoning.delete_events(cid).await;
            Ok(Json::from(()))
        }
    })
    .name("cleanup_events")
    .await?;
}
```

**File:** `crates/ps-server/src/services/reasoning/agent_query.rs`

Remove the `delete_events` call from the server poll loop (line 203). The poll loop should just return when it sees a terminal event:

```rust
// Before (line 201-203):
if is_terminal {
    let _ = repos.reasoning.delete_events(conv_id).await;
    return;
}

// After:
if is_terminal {
    return;
}
```

#### Step 3.5: Update all non-worker `append_event` call sites

Search for all call sites that call `append_event` and add the new `None, None` parameters for step_id/step_seq. These are non-step events (container_status, artifact_uploaded) that don't need step identity:

**File:** `crates/ps-workers/src/handlers/agentic_query.rs` — the container_status and artifact_uploaded events (around lines 470-525)

**Testing (Phase 3):**

**File:** `crates/ps-workers/src/handlers/step_registry.rs` — inline `#[cfg(test)]` module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_thinking_block_gets_identity() {
        let mut reg = StepRegistry::new();
        let id = reg.thinking_step(0, "I think we should");
        assert_eq!(id.step_id, "think-0-0");
        assert_eq!(id.step_seq, 0);
    }

    #[test]
    fn cumulative_thinking_reuses_identity() {
        let mut reg = StepRegistry::new();
        let id1 = reg.thinking_step(0, "I think");
        let id2 = reg.thinking_step(0, "I think we should");
        assert_eq!(id1.step_id, id2.step_id);
        assert_eq!(id1.step_seq, id2.step_seq);
    }

    #[test]
    fn recycled_part_index_gets_new_generation() {
        let mut reg = StepRegistry::new();
        let id1 = reg.thinking_step(0, "First thought");
        // Tool call between thinking blocks advances seq
        let _ = reg.tool_started("tool-1");
        let id2 = reg.thinking_step(0, "Completely different thought");
        assert_eq!(id1.step_id, "think-0-0");
        assert_eq!(id2.step_id, "think-0-1");
        assert_ne!(id1.step_seq, id2.step_seq);
    }

    #[test]
    fn tool_started_and_completed_share_identity() {
        let mut reg = StepRegistry::new();
        let started = reg.tool_started("abc-123");
        let completed = reg.tool_completed("abc-123");
        assert_eq!(started.step_id, completed.step_id);
        assert_eq!(started.step_seq, completed.step_seq);
    }

    #[test]
    fn tool_completed_without_started_gets_new_seq() {
        let mut reg = StepRegistry::new();
        let completed = reg.tool_completed("orphan");
        assert_eq!(completed.step_id, "tool-orphan");
        assert_eq!(completed.step_seq, 0);
    }

    #[test]
    fn interleaved_thinking_and_tools_preserve_order() {
        let mut reg = StepRegistry::new();
        let t0 = reg.thinking_step(0, "Thinking about query");
        let tool1 = reg.tool_started("call-1");
        let t0_update = reg.thinking_step(0, "Thinking about query structure");
        let tool1_done = reg.tool_completed("call-1");
        let t1 = reg.thinking_step(1, "Now analyzing results");

        // Thinking update keeps original seq
        assert_eq!(t0.step_seq, t0_update.step_seq);
        // Tool keeps its seq between started/completed
        assert_eq!(tool1.step_seq, tool1_done.step_seq);
        // Order: think-0 (0) < tool-1 (1) < think-1 (2)
        assert!(t0.step_seq < tool1.step_seq);
        assert!(tool1.step_seq < t1.step_seq);
    }

    #[test]
    fn seq_is_monotonically_increasing() {
        let mut reg = StepRegistry::new();
        let ids: Vec<i32> = (0..5)
            .map(|i| reg.tool_started(&format!("call-{i}")).step_seq)
            .collect();
        for window in ids.windows(2) {
            assert!(window[0] < window[1]);
        }
    }
}
```

**File:** `tests/integration/src/repo/reasoning.rs` — add test:

```rust
define_repo_test!(conversation_events_step_identity, |repos: Repos, _pool: PgPool| async move {
    // Create conversation, append events with step identity, poll and verify
    let conv = repos.reasoning.create_conversation(&CreateConversationParams {
        user_id: Uuid::now_v7(),
        title: Some("test"),
        model_name: "test/model",
    }).await.unwrap();

    repos.reasoning.append_event(
        conv.id, "thinking",
        &json!({"text": "thinking", "part_index": 0}),
        Some("think-0-0"), Some(0),
    ).await.unwrap();

    repos.reasoning.append_event(
        conv.id, "tool_call_started",
        &json!({"tool_name": "bash", "call_id": "c1", "arguments_json": "{}"}),
        Some("tool-c1"), Some(1),
    ).await.unwrap();

    let events = repos.reasoning.poll_events(conv.id, 0).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].step_id.as_deref(), Some("think-0-0"));
    assert_eq!(events[0].step_seq, Some(0));
    assert_eq!(events[1].step_id.as_deref(), Some("tool-c1"));
    assert_eq!(events[1].step_seq, Some(1));
});
```

---

### Phase 4: Server — pass step identity through to proto and implement ResumeStream

**Goal:** The server poll loop passes `step_id`/`step_seq` from DB events to proto messages, and a new `ResumeStream` RPC enables reconnection.

#### Step 4.1: Update `map_db_event_to_proto` to populate step fields

**File:** `crates/ps-server/src/services/reasoning/agent_query.rs`

Change the function signature (line 241) to accept the full event row instead of just type + payload:

```rust
fn map_db_event_to_proto(event: &ConversationEvent) -> Option<AskQuestionResponse> {
    let event_type = &event.event_type;
    let payload = &event.payload;
    let step_id = event.step_id.clone().unwrap_or_default();
    let step_seq = event.step_seq.unwrap_or(0);

    let evt = match event_type.as_str() {
        // ... existing match arms, but now populate step_id/step_seq:
        "thinking" => ask_question_response::Event::Thinking(AgentThinking {
            text: /* ... same ... */,
            part_index: /* ... same ... */,
            step_id,
            step_seq,
        }),
        "tool_call_started" => ask_question_response::Event::ToolCallStarted(AgentToolCallStarted {
            tool_name: /* ... same ... */,
            arguments_json: /* ... same ... */,
            call_id: /* ... same ... */,
            step_id,
            step_seq,
        }),
        "tool_call_completed" => ask_question_response::Event::ToolCallCompleted(AgentToolCallCompleted {
            // ... same fields ...
            step_id,
            step_seq,
        }),
        // Other event types unchanged (no step identity)
        // ...
    };
    // ...
}
```

Update the call site in the poll loop (line 187) to pass the full event:

```rust
// Before:
let proto_event = map_db_event_to_proto(&event.event_type, &event.payload);

// After:
let proto_event = map_db_event_to_proto(&event);
```

#### Step 4.2: Implement `ResumeStream` RPC

**File:** `crates/ps-server/src/services/reasoning/agent_query.rs`

Add a new function `resume_stream` that extracts the poll loop into a shared helper:

```rust
pub type ResumeStreamStream = AskQuestionStream;  // Same stream type

pub async fn resume_stream(
    svc: &ReasoningServiceImpl,
    request: Request<ResumeStreamRequest>,
) -> Result<Response<ResumeStreamStream>, Status> {
    let ctx = require_auth(&request)?;
    let req = request.into_inner();

    let conv_id = req
        .conversation_id
        .parse::<Uuid>()
        .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

    // Verify conversation exists and belongs to this user.
    let conv = svc
        .repos
        .reasoning
        .get_conversation(conv_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("conversation not found"))?;

    if conv.user_id != ctx.user_id {
        return Err(Status::not_found("conversation not found"));
    }

    // If query is already terminal, send final state and close.
    if matches!(conv.query_status.as_str(), "completed" | "failed" | "cancelled" | "idle") {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        // No events to stream — just close the stream immediately.
        drop(tx);
        return Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)));
    }

    // Start the shared poll loop from the requested cursor.
    let repos = svc.repos.clone();
    let cursor = req.last_event_id;
    let (tx, rx) = tokio::sync::mpsc::channel(64);

    tokio::spawn(async move {
        stream_events(repos, conv_id, cursor, tx).await;
    });

    Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
}
```

Extract the existing poll loop from `ask_question` into a shared function:

```rust
/// Shared poll loop used by both AskQuestion and ResumeStream.
async fn stream_events(
    repos: Repos,
    conv_id: Uuid,
    initial_cursor: i64,
    tx: tokio::sync::mpsc::Sender<Result<AskQuestionResponse, Status>>,
) {
    let mut cursor = initial_cursor;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);

    loop {
        if tokio::time::Instant::now() >= deadline {
            let _ = tx.send(Ok(AskQuestionResponse {
                event: Some(ask_question_response::Event::Error(AgentError {
                    message: "Stream timed out".into(),
                    retryable: true,
                })),
            })).await;
            return;
        }

        match repos.reasoning.poll_events(conv_id, cursor).await {
            Ok(events) => {
                for event in events {
                    cursor = event.id;
                    let proto_event = map_db_event_to_proto(&event);
                    if let Some(response) = proto_event {
                        let is_terminal = matches!(
                            response.event,
                            Some(
                                ask_question_response::Event::FinalAnswer(_)
                                    | ask_question_response::Event::Error(_)
                            )
                        );
                        if tx.send(Ok(response)).await.is_err() {
                            return;
                        }
                        if is_terminal {
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "failed to poll conversation events");
                let _ = tx.send(Ok(AskQuestionResponse {
                    event: Some(ask_question_response::Event::Error(AgentError {
                        message: "Internal error polling events".into(),
                        retryable: true,
                    })),
                })).await;
                return;
            }
        }

        // Check for cancelled/failed status without events.
        if let Ok(Some(conv)) = repos.reasoning.get_conversation(conv_id).await
            && matches!(conv.query_status.as_str(), "cancelled" | "failed")
        {
            return;
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
```

Then refactor `ask_question` to use this shared function instead of its inline poll loop.

#### Step 4.3: Wire `ResumeStream` into the service trait

**File:** `crates/ps-server/src/services/reasoning/mod.rs`

Add the `resume_stream` method to the tonic service implementation. The exact wiring depends on how the service is structured, but it follows the same pattern as `ask_question`.

**Testing (Phase 4):**

**File:** `tests/integration/src/api/reasoning.rs`

Add tests:

```rust
define_api_test!(resume_stream_replays_events, |server: TestServer| async move {
    // 1. Create admin, create conversation, seed events with step_id/step_seq
    // 2. Set query_status to "running"
    // 3. Call ResumeStream with last_event_id=0
    // 4. Verify all events are replayed in order with step_id/step_seq
    // 5. Seed a final_answer event
    // 6. Verify stream terminates
});

define_api_test!(resume_stream_completed_returns_empty, |server: TestServer| async move {
    // 1. Create conversation with query_status="completed"
    // 2. Call ResumeStream
    // 3. Verify stream closes immediately (no events)
});

define_api_test!(resume_stream_from_cursor, |server: TestServer| async move {
    // 1. Seed 5 events, note the id of event 3
    // 2. Call ResumeStream with last_event_id = event_3.id
    // 3. Verify only events 4 and 5 are returned
});

define_api_test!(ask_question_includes_step_identity, |server: TestServer| async move {
    // 1. Seed thinking + tool events with step_id/step_seq
    // 2. Stream via AskQuestion
    // 3. Verify proto messages have step_id and step_seq populated
});
```

---

### Phase 5: Frontend — pure step derivation and event-driven state

**Goal:** Replace the mutable `steps[]` array with a pure `deriveSteps()` function that builds steps from events sorted by `step_seq`.

#### Step 5.1: Create `deriveSteps` utility

**File:** `frontend/views/ask/lib/derive-steps.ts` (new)

```typescript
import type { AgentStep, ReasoningStep, ToolCallStep } from "../hooks/use-ask-question";

/**
 * Raw event from the server stream, carrying server-assigned identity.
 */
export type StreamEvent = {
  /** DB auto-increment id (arrival order, used as cursor). */
  id: number;
  eventType: string;
  stepId: string;
  stepSeq: number;
  payload: Record<string, unknown>;
};

/**
 * Derive an ordered, deduplicated step list from a sequence of stream events.
 *
 * This is a pure function — same events in, same steps out.
 * Display order is determined solely by server-assigned `stepSeq`.
 * Thinking updates (cumulative) replace text but never change position.
 * Tool completions update an existing started step in-place.
 */
export const deriveSteps = (events: StreamEvent[]): AgentStep[] => {
  const stepMap = new Map<string, { seq: number; step: AgentStep }>();

  for (const event of events) {
    if (!event.stepId) continue;

    switch (event.eventType) {
      case "thinking": {
        const existing = stepMap.get(event.stepId);
        const text = (event.payload.text as string) ?? "";
        const partIndex = (event.payload.part_index as number) ?? 0;

        if (existing && existing.step.kind === "reasoning") {
          // Cumulative update — replace text, keep position
          stepMap.set(event.stepId, {
            seq: existing.seq,
            step: { kind: "reasoning", text, partIndex, stepId: event.stepId },
          });
        } else {
          stepMap.set(event.stepId, {
            seq: event.stepSeq,
            step: { kind: "reasoning", text, partIndex, stepId: event.stepId },
          });
        }
        break;
      }

      case "tool_call_started": {
        stepMap.set(event.stepId, {
          seq: event.stepSeq,
          step: {
            kind: "tool",
            callId: (event.payload.call_id as string) ?? "",
            toolName: (event.payload.tool_name as string) ?? "",
            argumentsJson: (event.payload.arguments_json as string) ?? "{}",
            status: "running",
            stepId: event.stepId,
          } as ToolCallStep,
        });
        break;
      }

      case "tool_call_completed": {
        const existing = stepMap.get(event.stepId);
        if (existing && existing.step.kind === "tool") {
          stepMap.set(event.stepId, {
            seq: existing.seq,
            step: {
              ...existing.step,
              resultSummary: event.payload.result_summary as string,
              durationMs: event.payload.duration_ms as number,
              success: event.payload.success as boolean,
              status: (event.payload.success as boolean) ? "completed" : "error",
            },
          });
        }
        break;
      }
    }
  }

  return [...stepMap.values()]
    .sort((a, b) => a.seq - b.seq)
    .map(({ step }) => step);
};
```

#### Step 5.2: Add `stepId` to step types

**File:** `frontend/views/ask/hooks/use-ask-question.ts`

Update the type definitions:

```typescript
export type ToolCallStep = {
  kind: "tool";
  callId: string;
  toolName: string;
  argumentsJson: string;
  resultSummary?: string;
  durationMs?: number;
  success?: boolean;
  status: "running" | "completed" | "error";
  stepId?: string;  // NEW — server-assigned stable identity
};

export type ReasoningStep = {
  kind: "reasoning";
  text: string;
  partIndex: number;
  stepId?: string;  // NEW
};
```

#### Step 5.3: Refactor `useAskQuestion` to use event buffer + derivation

**File:** `frontend/views/ask/hooks/use-ask-question.ts`

Replace the mutable `steps` array approach with an event buffer:

```typescript
import { deriveSteps, type StreamEvent } from "../lib/derive-steps";

export const useAskQuestion = (): {
  state: AgentState;
  ask: (question: string, conversationId?: string) => Promise<void>;
  cancel: () => void;
  reset: () => void;
  resume: (conversationId: string) => Promise<void>;  // NEW
} => {
  const [events, setEvents] = useState<StreamEvent[]>([]);
  const [meta, setMeta] = useState<StreamMeta>({ status: "idle" });
  const abortRef = useRef<AbortController | null>(null);
  const queryClient = useQueryClient();
  const nextEventId = useRef(0);

  // Derive steps from events — pure, deterministic, memoized
  const steps = useMemo(() => deriveSteps(events), [events]);

  // Helper to append an event from a proto response
  const appendEvent = useCallback((response: AskQuestionResponse): void => {
    const { event } = response;
    if (!event.case) return;

    // Extract step_id and step_seq from the proto message
    let stepId = "";
    let stepSeq = 0;
    let eventType = "";
    let payload: Record<string, unknown> = {};

    switch (event.case) {
      case "thinking":
        stepId = event.value.stepId;
        stepSeq = event.value.stepSeq;
        eventType = "thinking";
        payload = { text: event.value.text, part_index: event.value.partIndex };
        break;
      case "toolCallStarted":
        stepId = event.value.stepId;
        stepSeq = event.value.stepSeq;
        eventType = "tool_call_started";
        payload = {
          tool_name: event.value.toolName,
          arguments_json: event.value.argumentsJson,
          call_id: event.value.callId,
        };
        break;
      case "toolCallCompleted":
        stepId = event.value.stepId;
        stepSeq = event.value.stepSeq;
        eventType = "tool_call_completed";
        payload = {
          tool_name: event.value.toolName,
          result_summary: event.value.resultSummary,
          duration_ms: event.value.durationMs,
          success: event.value.success,
          call_id: event.value.callId,
        };
        break;
      default:
        return; // Non-step events handled separately
    }

    if (!stepId) return;

    const id = nextEventId.current++;
    setEvents((prev) => [...prev, { id, eventType, stepId, stepSeq, payload }]);
  }, []);

  const ask = useCallback(async (question: string, conversationId?: string) => {
    const abort = new AbortController();
    abortRef.current = abort;
    setEvents([]);
    nextEventId.current = 0;

    let partialAnswer = "";
    const artifacts: ArtifactInfo[] = [];
    let streamConversationId: string | undefined = conversationId;

    setMeta({ status: "container_starting", message: "Initialising agent...", question, conversationId });

    try {
      const stream = client.askQuestion({ question, conversationId }, { signal: abort.signal });

      for await (const response of stream) {
        if (abort.signal.aborted) break;
        const { event } = response;
        if (!event.case) continue;

        // Handle non-step events (meta updates)
        switch (event.case) {
          case "conversationCreated":
            streamConversationId = event.value.conversationId;
            queryClient.invalidateQueries({ queryKey: conversationKeys.list() });
            setMeta((prev) => ({ ...prev, conversationId: streamConversationId }));
            break;
          case "containerStatus":
            setMeta({ status: "container_starting", message: event.value.message, question, conversationId: streamConversationId });
            break;
          case "partialAnswer":
            partialAnswer = event.value.text;
            setMeta({ status: "streaming", question, conversationId: streamConversationId, partialAnswer, artifacts: [...artifacts] });
            break;
          case "artifactUploaded":
            artifacts.push(toArtifactInfo(event.value));
            setMeta((prev) => ({ ...prev, artifacts: [...artifacts] }));
            break;
          case "finalAnswer": { /* ... same completion logic ... */ break; }
          case "error": { /* ... same error logic ... */ break; }
        }

        // Append step events to the event buffer (thinking, tool_call_*)
        appendEvent(response);

        // Transition to streaming if we got step events
        if (event.case === "thinking" || event.case === "toolCallStarted") {
          setMeta((prev) =>
            prev.status === "container_starting"
              ? { status: "streaming", question, conversationId: streamConversationId, partialAnswer, artifacts: [...artifacts] }
              : prev
          );
        }
      }
    } catch (err) { /* ... same ... */ }
  }, [queryClient, appendEvent]);

  // ... cancel, reset ...

  // Build the full AgentState by combining meta + derived steps
  const state: AgentState = useMemo(() => {
    if (meta.status === "streaming") {
      return { ...meta, steps };
    }
    if (meta.status === "completed") {
      return { ...meta, steps };
    }
    return meta as AgentState;
  }, [meta, steps]);

  return { state, ask, cancel, reset, resume };
};
```

Note: The `meta` state type needs to be split — it holds everything except `steps`, which are derived. The `AgentState` union type stays the same for consumers.

#### Step 5.4: Update React keys in `ThinkingSteps`

**File:** `frontend/views/ask/components/thinking-steps.tsx`

Use `stepId` as the React key instead of array index:

```typescript
// Before (line 49):
<ThinkingStep key={step.kind === "tool" ? step.callId : `reasoning-${i}`} step={step} />

// After:
<ThinkingStep key={step.stepId ?? (step.kind === "tool" ? step.callId : `reasoning-${i}`)} step={step} />
```

This provides stable keys from the server when available, with fallback for historical traces without `stepId`.

**Testing (Phase 5):**

**File:** `frontend/views/ask/lib/derive-steps.test.ts` (new)

```typescript
import { describe, expect, it } from "vitest";
import { deriveSteps, type StreamEvent } from "./derive-steps";

describe("deriveSteps", () => {
  it("returns empty array for no events", () => {
    expect(deriveSteps([])).toEqual([]);
  });

  it("creates reasoning step from thinking event", () => {
    const events: StreamEvent[] = [
      { id: 1, eventType: "thinking", stepId: "think-0-0", stepSeq: 0,
        payload: { text: "I should query the database", part_index: 0 } },
    ];
    const steps = deriveSteps(events);
    expect(steps).toHaveLength(1);
    expect(steps[0].kind).toBe("reasoning");
    expect((steps[0] as any).text).toBe("I should query the database");
  });

  it("cumulative thinking updates replace text without changing order", () => {
    const events: StreamEvent[] = [
      { id: 1, eventType: "thinking", stepId: "think-0-0", stepSeq: 0,
        payload: { text: "I think", part_index: 0 } },
      { id: 2, eventType: "tool_call_started", stepId: "tool-abc", stepSeq: 1,
        payload: { tool_name: "bash", call_id: "abc", arguments_json: "{}" } },
      { id: 3, eventType: "thinking", stepId: "think-0-0", stepSeq: 0,
        payload: { text: "I think we should query", part_index: 0 } },
    ];
    const steps = deriveSteps(events);
    expect(steps).toHaveLength(2);
    // Thinking step stays first (seq=0), tool second (seq=1)
    expect(steps[0].kind).toBe("reasoning");
    expect((steps[0] as any).text).toBe("I think we should query");
    expect(steps[1].kind).toBe("tool");
  });

  it("tool started + completed merge into single step", () => {
    const events: StreamEvent[] = [
      { id: 1, eventType: "tool_call_started", stepId: "tool-abc", stepSeq: 0,
        payload: { tool_name: "bash", call_id: "abc", arguments_json: "{}" } },
      { id: 2, eventType: "tool_call_completed", stepId: "tool-abc", stepSeq: 0,
        payload: { tool_name: "bash", call_id: "abc", result_summary: "ok", duration_ms: 100, success: true } },
    ];
    const steps = deriveSteps(events);
    expect(steps).toHaveLength(1);
    expect(steps[0].kind).toBe("tool");
    expect((steps[0] as any).status).toBe("completed");
    expect((steps[0] as any).resultSummary).toBe("ok");
  });

  it("preserves server-determined order regardless of event arrival", () => {
    // Events arrive out of step_seq order (e.g. from resume replay)
    const events: StreamEvent[] = [
      { id: 3, eventType: "tool_call_started", stepId: "tool-b", stepSeq: 2,
        payload: { tool_name: "read", call_id: "b", arguments_json: "{}" } },
      { id: 1, eventType: "thinking", stepId: "think-0-0", stepSeq: 0,
        payload: { text: "first", part_index: 0 } },
      { id: 2, eventType: "tool_call_started", stepId: "tool-a", stepSeq: 1,
        payload: { tool_name: "bash", call_id: "a", arguments_json: "{}" } },
    ];
    const steps = deriveSteps(events);
    expect(steps.map((s) => s.stepId)).toEqual(["think-0-0", "tool-a", "tool-b"]);
  });

  it("ignores events without stepId", () => {
    const events: StreamEvent[] = [
      { id: 1, eventType: "partial_answer", stepId: "", stepSeq: 0,
        payload: { text: "answer text" } },
    ];
    expect(deriveSteps(events)).toEqual([]);
  });

  it("handles recycled part_index as separate steps", () => {
    const events: StreamEvent[] = [
      { id: 1, eventType: "thinking", stepId: "think-0-0", stepSeq: 0,
        payload: { text: "first thought", part_index: 0 } },
      { id: 2, eventType: "tool_call_started", stepId: "tool-a", stepSeq: 1,
        payload: { tool_name: "bash", call_id: "a", arguments_json: "{}" } },
      { id: 3, eventType: "thinking", stepId: "think-0-1", stepSeq: 2,
        payload: { text: "second thought", part_index: 0 } },
    ];
    const steps = deriveSteps(events);
    expect(steps).toHaveLength(3);
    expect(steps[0].kind).toBe("reasoning");
    expect((steps[0] as any).text).toBe("first thought");
    expect(steps[2].kind).toBe("reasoning");
    expect((steps[2] as any).text).toBe("second thought");
  });
});
```

---

### Phase 6: Frontend — reconnectable streaming

**Goal:** Navigating away and back to a conversation with an active query resumes the live stream.

#### Step 6.1: Add `resume()` method to `useAskQuestion`

**File:** `frontend/views/ask/hooks/use-ask-question.ts`

```typescript
const resume = useCallback(async (conversationId: string) => {
  const abort = new AbortController();
  abortRef.current = abort;
  setEvents([]);
  nextEventId.current = 0;

  setMeta({
    status: "streaming",
    question: "",  // Will be populated from conversation data
    conversationId,
    partialAnswer: "",
    artifacts: [],
  });

  try {
    const stream = client.resumeStream(
      { conversationId, lastEventId: BigInt(0) },
      { signal: abort.signal },
    );

    for await (const response of stream) {
      if (abort.signal.aborted) break;
      // Same event handling as ask() — append step events, update meta
      // ... (shared via appendEvent helper)
    }
  } catch (err) {
    if (!abort.signal.aborted) {
      setMeta({ status: "error", message: err instanceof Error ? err.message : "Connection lost", retryable: true });
    }
  }
}, [queryClient, appendEvent]);
```

#### Step 6.2: Auto-resume on page mount

**File:** `frontend/views/ask/pages/ask-page.tsx`

Add auto-resume logic when navigating to a conversation with an active query:

```typescript
const AskPage = (): React.ReactElement => {
  const { conversationId } = useParams<{ conversationId?: string }>();
  const navigate = useNavigate();
  const { state, ask, cancel, reset, resume } = useAskQuestion();
  const { data: conversationData, isLoading } = useGetConversation(conversationId ?? "");

  // ... existing streamConvIdRef logic ...

  // Auto-resume if we navigate to a conversation with an active query.
  const queryStatus = conversationData?.conversation?.queryStatus;
  const hasResumed = useRef(false);

  useEffect(() => {
    if (
      conversationId &&
      (queryStatus === "running" || queryStatus === "pending") &&
      state.status === "idle" &&
      !hasResumed.current
    ) {
      hasResumed.current = true;
      resume(conversationId);
    }
  }, [conversationId, queryStatus, state.status, resume]);

  // Reset the resume guard when conversation changes
  useEffect(() => {
    hasResumed.current = false;
  }, [conversationId]);

  // ... rest unchanged ...
};
```

#### Step 6.3: Update `useGetConversation` to expose `queryStatus`

**File:** `frontend/views/ask/hooks/use-conversations.ts`

The `queryStatus` field is already on `ConversationSummary` in the proto (field 12), so it's already available in `conversationData.conversation.queryStatus`. No changes needed here — just need to access it in the page component as shown above.

**Testing (Phase 6):**

Manual testing is the most valuable here, but we can add a hook test:

**File:** `frontend/views/ask/hooks/use-conversations.test.ts`

Add a test that verifies `queryStatus` is accessible from the conversation data:

```typescript
it("exposes queryStatus from conversation summary", async () => {
  const transport = createRouterTransport(({ service }) => {
    service(ReasoningService, {
      getConversation: () => ({
        conversation: { queryStatus: "running", /* ... */ },
        messages: [],
        artifacts: [],
      }),
    });
  });
  // ... render hook, assert queryStatus is "running"
});
```

---

### Phase 7: Unify historical trace format

**Goal:** Historical messages use the same trace format with `step_id`, so `parseReasoningTrace` and live display are identical.

#### Step 7.1: Update `parseReasoningTrace`

**File:** `frontend/views/ask/components/conversation-thread.tsx`

The `parseReasoningTrace` function (line 18) needs to extract `step_id` from stored traces so React keys are stable:

```typescript
const parseReasoningTrace = (json?: string): AgentStep[] => {
  if (!json) return [];
  try {
    const trace = JSON.parse(json);
    return (trace.steps ?? [])
      .filter(
        (s: { kind?: string; text?: string }) =>
          !(s.kind === "reasoning" && !s.text),
      )
      .map((s: Record<string, unknown>, i: number): AgentStep => {
        const stepId = (s.step_id as string) ?? undefined;
        if (s.kind === "reasoning") {
          return {
            kind: "reasoning" as const,
            text: (s.text as string) ?? "",
            partIndex: (s.part_index as number) ?? i,
            stepId,
          };
        }
        return {
          kind: "tool" as const,
          callId: (s.call_id as string) ?? `trace-${i}`,
          toolName: (s.tool_name as string) ?? "unknown",
          argumentsJson: (s.arguments as string) ?? "{}",
          resultSummary: s.result_summary as string | undefined,
          durationMs: s.duration_ms as number | undefined,
          success: true,
          status: "completed" as const,
          stepId,
        };
      });
  } catch {
    return [];
  }
};
```

**Testing (Phase 7):**

**File:** `frontend/views/ask/components/thinking-step.test.tsx`

Add test for step_id propagation:

```typescript
it("uses stepId as key when available", () => {
  const step: ReasoningStep = {
    kind: "reasoning",
    text: "thinking...",
    partIndex: 0,
    stepId: "think-0-0",
  };
  // Render ThinkingStep, verify it renders without remounting
});
```

---

### Phase 8: Event cleanup safety net

**Goal:** Prevent event table bloat from crashed workers.

#### Step 8.1: Add cleanup query to repo

**File:** `crates/ps-core/src/repo/reasoning/conversations.rs`

```rust
/// Delete stale events for conversations that are no longer active.
/// Used as a safety net for cases where the worker crashes before cleanup.
pub async fn cleanup_stale_events(&self, max_age_hours: i32) -> Result<u64, Error> {
    let result = sqlx::query!(
        r#"
        DELETE FROM reasoning.conversation_events
        WHERE conversation_id IN (
            SELECT id FROM reasoning.conversations
            WHERE query_status IN ('completed', 'failed', 'cancelled')
        )
        AND created_at < now() - make_interval(hours => $1)
        "#,
        max_age_hours,
    )
    .execute(&self.pool)
    .await?;
    Ok(result.rows_affected())
}
```

This can be called from a periodic task (e.g. metrics compute handler which already runs periodically) or a dedicated cleanup handler. For now, adding it as a repo method is sufficient — the actual scheduling can be added later.

---

## Complete File Manifest

### Files to create

| File | Purpose |
|------|---------|
| `migrations/NNNN_event_step_identity.sql` | Add `step_id`, `step_seq` columns |
| `crates/ps-workers/src/handlers/step_registry.rs` | `StepRegistry` for server-side identity assignment |
| `frontend/views/ask/lib/derive-steps.ts` | Pure `deriveSteps()` function |
| `frontend/views/ask/lib/derive-steps.test.ts` | Tests for `deriveSteps()` |

### Files to modify

| File | Changes |
|------|---------|
| **Proto** | |
| `proto/canonical/prism/v1/reasoning.proto` | Add `step_id`/`step_seq` to 3 messages, add `ResumeStream` RPC + request message |
| **Rust — ps-core** | |
| `crates/ps-core/src/repo/reasoning/conversations.rs` | Update `ConversationEvent` struct, `append_event` signature, `poll_events` SELECT, add `get_all_events`, `cleanup_stale_events` |
| **Rust — ps-workers** | |
| `crates/ps-workers/src/handlers/mod.rs` | Export `step_registry` module |
| `crates/ps-workers/src/handlers/agentic_query.rs` | Replace `trace_steps` with `StepRegistry`, add `derive_trace_from_events`, move event cleanup, update all `append_event` calls |
| **Rust — ps-server** | |
| `crates/ps-server/src/services/reasoning/agent_query.rs` | Update `map_db_event_to_proto` to pass `step_id`/`step_seq`, extract `stream_events` helper, implement `resume_stream`, remove `delete_events` from poll loop |
| `crates/ps-server/src/services/reasoning/mod.rs` | Wire `resume_stream` to service trait |
| **Frontend** | |
| `frontend/views/ask/hooks/use-ask-question.ts` | Add `stepId` to step types, refactor to event buffer + `deriveSteps`, add `resume()` method |
| `frontend/views/ask/pages/ask-page.tsx` | Add auto-resume on mount for active queries |
| `frontend/views/ask/components/thinking-steps.tsx` | Use `stepId` as React key |
| `frontend/views/ask/components/conversation-thread.tsx` | Update `parseReasoningTrace` to extract `step_id` |
| **Generated (auto)** | |
| `crates/ps-proto/src/gen/` | Regenerated from proto |
| `frontend/lib/api/gen/` | Regenerated from proto |
| `.sqlx/` | Updated query cache (separate commit) |

### Test files to create/modify

| File | Changes |
|------|---------|
| `crates/ps-workers/src/handlers/step_registry.rs` | Inline `#[cfg(test)]` module — 7 unit tests |
| `frontend/views/ask/lib/derive-steps.test.ts` | New — 7 vitest tests |
| `tests/integration/src/repo/reasoning.rs` | Add `conversation_events_step_identity` test |
| `tests/integration/src/api/reasoning.rs` | Add 4 tests: `resume_stream_replays_events`, `resume_stream_completed_returns_empty`, `resume_stream_from_cursor`, `ask_question_includes_step_identity` |
| `frontend/views/ask/hooks/use-conversations.test.ts` | Add `queryStatus` accessibility test |

## Implementation Order (with commit boundaries)

| # | Commit | Scope | Depends on |
|---|--------|-------|------------|
| 1 | `feat: add step_id and step_seq columns to conversation events` | Migration | — |
| 2 | `chore: update sqlx query cache` | `.sqlx/` | 1 |
| 3 | `feat: update ConversationEvent and append_event for step identity` | ps-core repo | 1 |
| 4 | `chore: update sqlx query cache` | `.sqlx/` | 3 |
| 5 | `feat: add step identity fields and ResumeStream RPC to proto` | proto + buf generate | — |
| 6 | `feat: implement StepRegistry for server-side step ordering` | ps-workers, with unit tests | 3 |
| 7 | `feat: wire StepRegistry into agentic query handler` | ps-workers | 3, 6 |
| 8 | `feat: derive trace from events instead of separate accumulation` | ps-workers | 7 |
| 9 | `feat: move event cleanup to worker, remove from poll loop` | ps-workers + ps-server | 7 |
| 10 | `feat: pass step_id/step_seq through server poll loop to proto` | ps-server | 3, 5 |
| 11 | `feat: implement ResumeStream RPC` | ps-server, with integration tests | 5, 10 |
| 12 | `feat: add deriveSteps utility for pure step derivation` | frontend, with vitest tests | 5 |
| 13 | `refactor: replace mutable steps array with event-driven derivation` | frontend hook | 12 |
| 14 | `feat: add stream resume on page mount for active queries` | frontend page | 11, 13 |
| 15 | `feat: unify historical trace format with step_id` | frontend + backend | 8, 12 |
| 16 | `feat: add stale event cleanup method` | ps-core | 3 |

Commits 1-4 and 5 can be done in parallel. Commits 6-9 are sequential (worker changes). Commits 10-11 are sequential (server changes). Commits 12-15 are sequential (frontend changes).

## Invariants

1. **Events are append-only** — once written to `conversation_events`, an event's `step_id` and `step_seq` never change
2. **`step_seq` is monotonically increasing** — new steps always get a higher seq than existing steps; updates to existing steps keep their original seq
3. **`step_id` is stable** — a thinking block or tool call keeps the same `step_id` for its entire lifecycle
4. **Display order = `step_seq` order** — the frontend sorts by `step_seq`, period. No client-side reordering.
5. **Thinking updates don't move** — updating a thinking block's text does not change its position; it stays at its original `step_seq`
6. **Events are the source of truth** — the stored `reasoning_trace` is derived from events, not built separately
7. **Reconnection is seamless** — the same event-polling mechanism serves both initial streams and reconnections

## Risks & Mitigations

| Risk | Mitigation |
|------|-----------|
| Event DB table grows large during long queries | TTL cleanup + batch deletion after message persisted |
| `step_seq` gaps if events are lost | Frontend sorts by `step_seq`, gaps are fine — ordering is relative, not absolute |
| Backward compatibility with existing traces | `parseReasoningTrace` already handles missing fields with defaults; `step_id`/`step_seq` are additive proto fields (0 defaults) |
| OpenCode changes `part_index` behaviour | Server-side `StepRegistry` is the single place to handle this; frontend is insulated |
| Poll loop latency (100ms) feels laggy | Already acceptable for tool calls; could reduce to 50ms for thinking tokens if needed |
| In-flight events from old deployment lack `step_id` | Columns are nullable; frontend falls back to `id` ordering and index-based keys when `stepId` is absent |
