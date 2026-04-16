---
paths:
  - "crates/ps-workers/**"
---

# Restate Handler Rules

## Handler Types

- **Objects** are keyed (per-source or per-conversation): ingestion handlers, team sync, agentic query, watchdog
- **Services** are singletons: metrics compute, enrichment, identity resolution, model catalogue
- The watchdog uses a fixed `singleton` key for exclusive serialized execution

## SharedState

All handlers receive `SharedState` (constructed once in `main.rs`, cloned into each handler). It contains `Repos`, `secret_key`, and `http_client`. Handlers never touch `PgPool` directly — always go through `state.repos`.

## Journaling Rules

| What | Inside `ctx.run()`? | Why |
| --- | --- | --- |
| DB writes (store, watermark, run lifecycle) | **Yes** | Must be idempotent on replay |
| External API calls (GitHub, Jira, AI) | **No** | Responses are large; re-executing is safe (upserts) |
| Secret decryption | **No** | Journal persists results — plaintext must never be inside |
| Progress updates | **No** | Best-effort, doesn't affect replay correctness |

All `ctx.run()` closures must have `.name("step_name")` labels for journal debugging.

## Run Lifecycle Macros

Managed by macros in `infra/run_lifecycle.rs`:

- **`create_run!`** — inside `ctx.run()`, generates `Uuid::now_v7()` inside the closure so retries reuse the journaled ID
- **`complete_run!`** — inside `ctx.run()`, marks complete + clears `current_invocation_id`
- **`complete_run_with_warnings!`** — partial failure: records failed items in metadata
- **`fail_run!`** — inside `ctx.run()`, marks failed + clears `current_invocation_id`

All log errors rather than propagating — run lifecycle failure should not abort the handler.

## `journaled!` / `journaled_value!` Macros

For ad-hoc `ctx.run()` calls, use journaling macros from `infra/run_lifecycle.rs`. They handle the double-clone dance required by Restate's `Fn` closures.

```rust
// Unit-returning:
journaled!(ctx, "step_name", [repos, some_string], {
    repos.reasoning.update_something(id, &some_string).await
        .map_err(terminal_err("failed to update"))?;
});

// Value-returning:
let items = journaled_value!(ctx, "fetch_queue", [repos], {
    repos.reasoning.find_queued(100).await
        .map_err(terminal_err("db error"))?
});
```

- The capture list `[repos, some_string]` lists variables that need cloning. `Copy` types (e.g. `Uuid`, `i32`) are captured by `move` directly — don't list them.
- Use `terminal_err("context")` instead of `.map_err(|e| TerminalError::new(format!("context: {e}")))`.
- Both macros propagate errors with `?`. For fire-and-forget calls, use the manual `ctx.run()` pattern instead.
- If the step name uses a captured variable, compute the name *before* the macro invocation to avoid borrow-after-move.

## Journal Compatibility

Changing the sequence of `ctx.run()` calls **breaks in-flight invocations**. Restate replays positionally — different steps at the same indices causes error 570. After refactoring:

1. Cancel all in-flight invocations for affected handlers
2. If needed: wipe Restate's journal storage (`/restate-data/store/`) and restart the pod
3. Re-register via Admin API: `curl -X POST http://localhost:9070/deployments -H 'content-type: application/json' -d '{"uri":"http://ps-workers:9081/","force":true}'`

## Frontend Dispatch

- Use `TriggerHandler` RPC (fire-and-forget to Restate), never synchronous RPCs for long operations
- `trigger_handler()` guards against duplicate runs
- UI shows Run/Cancel toggle with polling for status

## Adding a New Ingestion Handler

1. **Source module** — `crates/ps-workers/src/features/ingestion/<platform>/`. Implement `Source` trait. Define cursor struct.
2. **Registry** — add `Platform::NewPlatform => Some(Box::new(NewPlatformSource))` in `registry.rs`
3. **Handler** — define `IngestionSpec`, implement `ProgressTracker`, create `#[restate_sdk::object]` with `run_ingestion()` and `backfill()`. Call `execute_ingestion_chunked()`.
4. **Export** — add `pub mod` in `mod.rs`
5. **Wire up** — instantiate in `main.rs`, bind to Restate endpoint
6. **Platform enum** — add variant to `Platform` in `ps-core/src/models/enums.rs`

## Adding a New System Handler

1. Use `#[restate_sdk::service]` (singleton) or `#[restate_sdk::object]` (per-key)
2. Follow journaling rules above
3. Export in `mod.rs`, wire up in `main.rs`
