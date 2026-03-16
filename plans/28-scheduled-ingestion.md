# Plan 28 — Scheduled Ingestion via Restate

## Problem

Ingestion runs are currently manual-only — an admin must click "Run Now" on the ingestion page every time they want fresh data. The `schedule_cron` field exists in the database schema and proto definitions but nothing reads it, and there's no scheduler to trigger runs automatically.

For a production deployment ingesting from multiple sources on a 3–6 hour cadence, manual triggering is not viable. We need cron-like scheduling that:

1. Runs automatically on a per-source cadence (configurable via cron expression)
2. Survives service restarts without missing or duplicating runs
3. Is observable — the UI shows when the next run is scheduled
4. Can be configured and changed at runtime from the admin UI without redeploying

## Current State

### What exists

- **`config.source_configs.schedule_cron`** — nullable `TEXT` column, stores a cron expression per source. Already in the DB schema (migration 0001) and proto (`SourceConfig.schedule_cron`).
- **`config.global_settings`** — key-value table for system-wide defaults. Plan 03 envisioned a `default_schedule` key (e.g. `"0 */6 * * *"`). Not yet populated.
- **`CreateSourceRequest` / `UpdateSourceRequest`** — both accept `schedule_cron` in proto. The config service persists it. `update_source_schedule()` repo method exists.
- **`GetStatus` response** — has a TODO comment about computing next run from `schedule_cron`.
- **Manual triggers** — `TriggerRun`, `TriggerBackfill`, `TriggerTeamSync` RPCs fire-and-forget to Restate.

### What's missing

- No scheduler reads `schedule_cron` and triggers runs.
- No Restate `send_after()` / delayed self-invocation.
- No "next run at" display in the UI.
- No global default schedule setting in the UI.
- Frontend source dialogs don't expose the cron field.

### Restate scheduling primitives

Restate SDK 0.9 provides two mechanisms for scheduling:

1. **`ctx.object_client::<T>(key).method().send_with_delay(duration)`** — sends a message to a virtual object handler that will be delivered after the specified delay. The delay is durable (survives restarts). This is the "delayed self-invocation" pattern from Plan 03.

2. **`ctx.sleep(duration)`** — durable sleep within a handler. The invocation is suspended and resumed after the delay. Useful for rate limiting but not ideal for scheduling because it keeps an invocation alive (occupying the virtual object's concurrency slot).

**Approach: delayed self-invocation.** At the end of each run (success or failure), the handler calculates the next run time from the cron expression and calls itself via `send_with_delay()`. This is the established Restate pattern for recurring work — no external cron daemon, no polling loop, and the timer is durable.

## Design

### Architecture

```
                                    ┌──────────────┐
                                    │  Restate      │
                                    │  (durable     │
  ┌──────────────┐  trigger/send    │   timers)     │
  │  ps-server   │ ───────────────► │               │
  │  (gRPC API)  │                  │  GithubIngest │──► run_ingestion
  └──────────────┘                  │  Handler      │        │
        ▲                           └──────────────┘        │
        │                                                    │
   admin UI                               on completion:     │
   sets cron                              send_with_delay ◄──┘
                                          (next cron tick)
```

The scheduling loop is self-sustaining: each run schedules the next one. Starting the loop requires a one-time "kick" when:
- A source is created with a cron schedule
- A source's schedule is changed from null to a cron expression
- The ingestion service starts up (recover any schedules that should be active)

### Phase 1: Scheduler Handler

Add a new method to `GithubIngestionHandler` (and eventually to every handler type):

```rust
#[restate_sdk::object]
pub trait GithubIngestionHandler {
    async fn run_ingestion() -> Result<(), TerminalError>;
    async fn backfill(since_date: String) -> Result<(), TerminalError>;

    // New: start the scheduling loop for this source
    async fn start_schedule() -> Result<(), TerminalError>;
}
```

**`start_schedule` flow:**

1. Load the source config from DB (get `schedule_cron`)
2. If `schedule_cron` is null/empty, log and return (no schedule to start)
3. Parse the cron expression using the `cron` crate
4. Calculate the next occurrence from `now()`
5. Calculate the delay as `next_occurrence - now()`
6. Call `ctx.object_client::<GithubIngestionHandlerClient>(ctx.key()).scheduled_run().send_with_delay(delay)`
7. Store the scheduled next-run time in Restate KV state for observability

**`scheduled_run` — new handler method:**

```rust
async fn scheduled_run() -> Result<(), TerminalError>;
```

1. Load source config
2. If source is disabled or `schedule_cron` is null, return without rescheduling
3. Run the ingestion (delegate to `execute_ingestion`)
4. On completion (success or failure), reload config to get the latest `schedule_cron`
5. Parse cron, calculate next occurrence, `send_with_delay()` for the next run
6. Update KV state with next scheduled time

This separation between `run_ingestion` (manual, one-shot) and `scheduled_run` (auto-reschedules) means manual runs don't interfere with the schedule, and changing the schedule doesn't require cancelling in-flight runs.

### Phase 2: Schedule Lifecycle

**Starting schedules on boot:**

When the ingestion service starts (`main.rs`), after registering with Restate:

1. Query all enabled sources with non-null `schedule_cron`
2. For each, send `start_schedule()` to the corresponding handler
3. Restate's virtual object concurrency guarantees that duplicate `start_schedule` calls are safe — if one is already queued, the second queues behind it

**Starting/updating schedules on config change:**

When the config service updates a source's `schedule_cron`:

1. The `UpdateSource` RPC handler (in `ps-server`) sends `start_schedule()` to Restate after persisting the change
2. The `start_schedule` handler reads the latest cron from DB, so it always uses the current value
3. If the schedule was removed (set to null), `start_schedule` returns without scheduling — the loop stops naturally

**Cancelling a schedule:**

There's no explicit "cancel schedule" needed. When `scheduled_run` fires and finds `schedule_cron` is null or the source is disabled, it simply doesn't reschedule. The loop dies naturally.

For immediate cancellation (e.g. source deleted), we can cancel pending invocations via Restate's admin API — the same mechanism already used for `CancelRun`.

### Phase 3: Observability — Next Run Time

**Backend:**

Add a `next_scheduled_run` field to the `GetStatus` response:

```protobuf
message SourceStatus {
  // ... existing fields ...
  google.protobuf.Timestamp next_scheduled_run = 12;
  string schedule_cron = 13;  // echo back for UI display
}
```

**Approach: Restate KV state.** The `scheduled_run` and `start_schedule` handlers write the next run time to Restate's per-object key-value state (`ctx.set("next_scheduled_run", timestamp)`). To read it back:

- Add a **`get_schedule_info` shared handler** on each virtual object that reads KV state and returns the next scheduled time + active cron expression. This is a cheap read-only call.
- `GetStatus` in `ps-server` calls `get_schedule_info` for each source via Restate's HTTP ingress (or the admin SQL API: `SELECT value FROM sys_keyed_state WHERE key = 'next_scheduled_run' AND service_name = '...'`).

This is the source of truth — it reflects exactly what Restate has queued, not a re-derivation. If the schedule was just changed, the KV state is updated in the same handler invocation that schedules the next run, so it's always accurate. It also correctly shows "no next run" when scheduling is paused or disabled, without needing to distinguish between "null cron" and "cron set but loop hasn't started yet".

**Frontend:**

The `SourceStatusCard` on the ingestion page shows:
- Current schedule in human-readable form (e.g. "Every 6 hours") with the raw cron expression as a tooltip
- "Next run: in 2h 34m" countdown (or "Next run: 14:30 UTC")
- If no schedule: "Manual only"

### Phase 4: Admin UI — Schedule Configuration

**Source create/edit dialogs:**

Add a schedule section to the source configuration dialog (already accepts `schedule_cron` in the proto, just not exposed in UI):

```
Schedule
┌─────────────────────────────────────────────┐
│  ○ Manual only (no automatic runs)          │
│  ● Run on schedule                          │
│                                             │
│  Preset: [Every 6 hours ▾]                  │
│                                             │
│  ┌─────────────────────────────────┐        │
│  │ 0 */6 * * *                     │        │
│  └─────────────────────────────────┘        │
│  Runs at 00:00, 06:00, 12:00, 18:00 UTC    │
│                                             │
│  Next run: ~2h 34m from now                 │
└─────────────────────────────────────────────┘
```

**Preset options:**
| Label | Cron | Notes |
|-------|------|-------|
| Every hour | `0 * * * *` | High-frequency, for active development tracking |
| Every 3 hours | `0 */3 * * *` | Good balance for most teams |
| Every 6 hours | `0 */6 * * *` | Default recommendation |
| Every 12 hours | `0 */12 * * *` | Low-activity sources |
| Daily at midnight | `0 0 * * *` | Once per day |
| Custom | (free text) | Advanced users |

**Validation:** Parse the cron expression client-side (use `croner` or `cron-parser` npm package) to show the human-readable description and next N run times. Reject invalid expressions before submission.

**Global default schedule:**

Add a setting in the admin settings page:

```
Default Ingestion Schedule
┌─────────────────────────────────────────────┐
│  [Every 6 hours ▾]                          │
│  0 */6 * * *                                │
│                                             │
│  Applied to sources without a custom        │
│  schedule. Currently affects 3 sources.     │
└─────────────────────────────────────────────┘
```

Stored in `config.global_settings` as `key = 'default_schedule'`, `value = '"0 */6 * * *"'`.

When a source has `schedule_cron = NULL`, the scheduler falls back to the global default. If the global default is also unset, no automatic scheduling occurs.

## Cron Expression Format

Use standard 5-field cron: `minute hour day-of-month month day-of-week`.

**Rust crate:** `cron` (crates.io) — mature, well-maintained, supports standard 5-field and extended 7-field expressions. `Schedule::from_str("0 */6 * * *")` → iterator of upcoming `DateTime<Utc>` values.

**Frontend crate:** `croner` (npm) — lightweight, supports the same 5-field format, provides `next()` for preview and `toString()` for human-readable descriptions.

## File Changes

### Backend

| File | Change |
|------|--------|
| `crates/ps-ingestion/src/handlers/github_ingestion.rs` | Add `start_schedule`, `scheduled_run` methods; self-invocation via `send_with_delay` |
| `crates/ps-ingestion/src/handlers/github_team_sync.rs` | Add `start_schedule`, `scheduled_run` for team sync scheduling |
| `crates/ps-ingestion/src/main.rs` | On startup, query enabled sources and send `start_schedule` to each |
| `crates/ps-ingestion/Cargo.toml` | Add `cron` dependency |
| `crates/ps-server/src/services/ingestion.rs` | Populate `next_scheduled_run` in `GetStatus`; send `start_schedule` after schedule updates |
| `crates/ps-server/src/services/config.rs` | After `update_source` with schedule change, notify ingestion service |
| `crates/ps-core/src/repo/config.rs` | Add `get_global_setting` / `set_global_setting` repo methods |
| `crates/ps-core/src/schedule.rs` | Shared cron parsing + next-run computation (used by both server and ingestion) |
| `proto/prism/v1/ingestion.proto` | Add `next_scheduled_run`, `schedule_cron` to `SourceStatus` |
| `proto/prism/v1/config.proto` | Add `GetGlobalSetting` / `SetGlobalSetting` RPCs |

### Frontend

| File | Change |
|------|--------|
| `frontend/views/ingestion/components/source-status-card.tsx` | Show schedule info + next run countdown |
| `frontend/views/sources/components/schedule-field.tsx` | New: reusable schedule picker (presets + custom cron input + preview) |
| `frontend/views/sources/components/add-source-dialog.tsx` | Add schedule field to create flow |
| `frontend/views/sources/components/edit-source-dialog.tsx` | Add schedule field to edit flow |
| `frontend/views/admin/components/settings-tab.tsx` | Add default schedule setting |
| `frontend/package.json` | Add `croner` dependency |

## Implementation Order

- [ ] **1. Cron parsing module** — `crates/ps-core/src/schedule.rs` with `next_run_after(cron_expr, after) -> Option<DateTime<Utc>>` and validation
- [ ] **2. Add `scheduled_run` + `start_schedule` to `GithubIngestionHandler`** — the core scheduling loop with `send_with_delay`
- [ ] **3. Startup schedule recovery** — `main.rs` queries enabled sources and sends `start_schedule` on boot
- [ ] **4. Schedule lifecycle on config change** — `ps-server` sends `start_schedule` when `schedule_cron` is updated
- [ ] **5. `GetStatus` next-run via Restate KV** — call `get_schedule_info` or query Restate admin SQL to populate `next_scheduled_run`
- [ ] **6. Proto updates** — add fields to `SourceStatus`, add `GetGlobalSetting` / `SetGlobalSetting` RPCs
- [ ] **7. Global default schedule** — repo methods for `config.global_settings`, fallback logic in scheduler
- [ ] **8. Frontend: schedule picker component** — presets, custom input, cron preview with `croner`
- [ ] **9. Frontend: wire into source create/edit dialogs** — add schedule field
- [ ] **10. Frontend: ingestion page next-run display** — countdown/timestamp on status cards
- [ ] **11. Frontend: admin default schedule setting** — settings tab addition
- [ ] **12. Add `start_schedule` / `scheduled_run` to `GithubTeamSyncHandler`** — same pattern, separate cadence

## Scope Notes

- **No second-granularity scheduling.** Cron's minimum resolution is 1 minute. This is fine — sub-minute ingestion makes no sense given API rate limits.
- **No distributed locking.** Restate's virtual object concurrency (one invocation per key at a time) already prevents overlapping runs for the same source. If a scheduled run arrives while a manual run is in progress, Restate queues it.
- **No catch-up runs.** If the service is down and misses a scheduled window, the next `start_schedule` on boot computes the next future occurrence — it doesn't try to run all missed intervals. This is intentional: catch-up would hammer APIs unnecessarily. The incremental watermark ensures no data is lost.
- **Team sync scheduling** reuses the same pattern but with its own `schedule_cron` (likely a separate field or a convention like daily-at-midnight). Detailed in step 12.

## Open Questions

1. ~~**Should `scheduled_run` and `run_ingestion` share the virtual object concurrency slot?**~~ **Yes — decided.** They're methods on the same virtual object, so Restate serialises them. A manual "Run Now" during a scheduled run queues behind it. This is correct — you don't want two concurrent runs for the same source hitting the same API with the same token.

2. ~~**Should we support timezone-aware cron?**~~ **No — UTC only.** All cron expressions are evaluated in UTC. Simpler, avoids DST edge cases.

3. ~~**Should changing the global default retroactively update sources with null schedules?**~~ **Yes — make it explicit.** The global default admin UI shows "N sources using this default" with a list of affected source names. Source status cards show "Using default schedule (every 6 hours)" rather than just "Every 6 hours" so it's clear the cadence is inherited, not per-source. The source edit dialog shows "Use default (every 6 hours)" as the selected option when `schedule_cron` is null.
