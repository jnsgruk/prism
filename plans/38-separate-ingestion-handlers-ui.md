# Plan 38: Separate Ingestion & Handlers UI

## Problem

The ingestion page and admin handlers page share the same `ListRuns` RPC and
display the same run table, but they serve different audiences:

| Page | Purpose | Should show |
|------|---------|-------------|
| `/ingestion` | Monitor data source ingestion | Only ingestion runs (GitHub, Jira, Discourse) |
| `/admin` → Handlers tab | Manage all Restate handlers | All handler runs (ingestion + team sync + identity resolution + metrics compute) |

### Current bugs

1. **Ingestion page shows `_system` runs.** `useListRuns(undefined)` returns all
   100 most recent runs unfiltered — so MetricsCompute and IdentityResolution
   runs appear as `_system` in the Source column, confusing users.

2. **Handlers tab shows `_system` as source name.** Service handlers
   (IdentityResolution, MetricsCompute) create runs with `source_name = "_system"`
   because they have no source. The table shows this raw value instead of the
   actual handler name.

3. **No cancel from run detail dialog.** Clicking a running row opens a
   read-only detail dialog — there's no way to cancel from there.

4. **Duplicate trigger UX.** The handlers tab has a "Trigger" button on each
   handler card, but the run table also has an inline cancel button. The
   ingestion page already handles run/cancel on the source cards — the table
   shouldn't also allow triggering. Neither page prevents queueing the same
   handler twice.

5. **Changes to one page break the other.** Both pages import from the same
   hooks and share table column definitions, so refactoring one page risks
   breaking the other.

## Design

### Principle: two views, one data model

The underlying `activity.ingestion_runs` table and `ListRuns` RPC remain
unified — every handler writes to the same table. The split is purely at the
**frontend presentation layer**: each page filters and displays runs
differently.

### Backend changes

#### 1. Add `is_ingestion` filter to `ListRuns` RPC

Add an optional `bool` field to `ListRunsRequest`:

```proto
message ListRunsRequest {
  optional string source_name = 1;
  optional string handler_name = 2;
  optional bool ingestion_only = 3;  // NEW — when true, exclude _system runs
}
```

In the repo query, when `ingestion_only = true`, add
`AND source_name != '_system'` to the WHERE clause. This is simpler than
maintaining a list of ingestion handler names.

**Alternative considered:** filter client-side. Rejected because the LIMIT 100
means we'd lose real ingestion runs when system runs dominate the top 100.

#### 2. Add handler status tracking

Currently there's no equivalent of `get_source_statuses()` for non-ingestion
handlers. The handlers tab needs to know if a handler is currently running to
disable duplicate triggers.

Add a repo method `get_active_handler_runs()` that returns currently-running
rows grouped by `handler_name`:

```sql
SELECT handler_name, handler_method, source_name, started_at, id
FROM activity.ingestion_runs
WHERE status = 'running'
ORDER BY started_at DESC
```

Expose via a new field on `ListHandlersResponse`:

```proto
message HandlerInfo {
  string name = 1;
  repeated string methods = 2;
  string description = 3;
  bool requires_key = 4;
  optional ActiveRun active_run = 5;  // NEW
}

message ActiveRun {
  string run_id = 1;
  string method = 2;
  optional string key = 3;
  Timestamp started_at = 4;
}
```

The `ListHandlers` RPC already returns static handler defs — enrich each with
its active run (if any) from the DB.

#### 3. Generic cancel RPC

`CancelRun` currently takes `source_name` which doesn't work for service
handlers (`_system`). Add a new RPC or extend the existing one:

```proto
// Option A: new RPC
rpc CancelHandlerRun(CancelHandlerRunRequest) returns (CancelHandlerRunResponse);

message CancelHandlerRunRequest {
  string run_id = 1;  // UUID from ingestion_runs
}
```

This is cleaner than the current approach of cancelling by source name. The
implementation looks up the run's `source_name` and `handler_name` from the DB,
then uses the existing Restate kill logic.

The old `CancelRun` (by source name) can remain for backward compat but
internally can delegate to the same logic.

### Frontend changes

#### 4. Ingestion page — filter to ingestion runs only

Change `useListRuns` call on ingestion page to pass `ingestion_only: true`:

```typescript
// ingestion-page.tsx
const { data: runs } = useListRuns(undefined, {
  refetchInterval: runsInterval,
  ingestionOnly: true,  // NEW
});
```

This ensures the run history table only shows source-specific ingestion runs.
No column changes needed — the existing Source/Started/Duration/Items/Status
columns are correct for ingestion.

#### 5. Handlers tab — card-based layout with inline status

Redesign the handlers tab to mirror the ingestion page pattern:

**Handler cards** (top section):
- Each handler gets a card similar to `SourceStatusRow`
- Shows: handler name, description, methods as badges
- Shows active run status if running (method, source/key, duration so far)
- Actions: Run button (opens trigger dialog) + Cancel button (if running)
- **Disable Run button when handler is already active** — prevents duplicate
  queueing

**Run history table** (bottom section):
- Keep current table but fix the Handler column:
  - Show `{handlerName}.{method}` as primary text (strip "Handler" suffix)
  - Show source/key as secondary text (only if not `_system`)
  - Never display `_system` anywhere
- Add cancel button in the run detail dialog for running rows
- Keep handler + status filter toggles

#### 6. Run detail dialog — add cancel action

Add an optional `onCancel` callback to `RunDetailDialog`:

```typescript
interface RunDetailDialogProps {
  run: HandlerRun;
  // ...existing props
  onCancel?: (runId: string) => void;  // NEW
}
```

When `run.status === "running"` and `onCancel` is provided, show a Cancel
button in the dialog footer. Wire this up on both pages.

#### 7. Split shared code cleanly

The hooks in `views/ingestion/hooks/use-ingestion.ts` are used by both pages.
This is fine — they're data-fetching hooks that belong to the ingestion/handlers
feature. But:

- **Don't split the hooks file.** Both pages query the same backend service.
  Keep `useListRuns`, `useListHandlers`, `useTriggerHandler`, `useCancelRun` in
  one file.
- **Do split the table components.** The ingestion page and handlers tab should
  each own their own table column definitions. The shared `RunDetailDialog`
  stays in `components/`.
- **Move `run-utils.ts` and `format.ts` to stay in `lib/`** — they're already
  there and shared correctly.

### File changes summary

| File | Change |
|------|--------|
| `proto/prism/v1/handlers.proto` | Add `ingestion_only` to `ListRunsRequest`, `ActiveRun` message, `active_run` to `HandlerInfo`, `CancelHandlerRun` RPC |
| `crates/ps-core/src/repo/activity.rs` | Add `ingestion_only` param to `list_runs`, add `get_active_handler_runs()` |
| `crates/ps-server/src/services/handlers.rs` | Wire new proto fields, implement `CancelHandlerRun`, enrich `ListHandlers` with active runs |
| `frontend/views/ingestion/hooks/use-ingestion.ts` | Add `ingestionOnly` option to `useListRuns`, add `useCancelHandlerRun` hook |
| `frontend/views/ingestion/pages/ingestion-page.tsx` | Pass `ingestionOnly: true` to `useListRuns` |
| `frontend/views/ingestion/components/ingestion-runs-table.tsx` | Add cancel support to `RunDetailDialog` integration |
| `frontend/views/admin/components/handlers-tab.tsx` | Redesign handler cards with active status, fix `_system` display, add cancel to detail dialog |
| `frontend/components/run-detail-dialog.tsx` | Add optional cancel button for running rows |

## Implementation order

1. **Proto + backend**: Add `ingestion_only`, `ActiveRun`, `CancelHandlerRun`
2. **`buf generate`** + rebuild
3. **Ingestion page**: Pass `ingestionOnly: true` — immediate fix, no UI redesign
4. **Run detail dialog**: Add cancel action
5. **Handlers tab**: Redesign cards + fix table display
6. **Test**: Verify both pages show correct data independently

## Non-goals

- **No new DB tables or schemas.** The unified `ingestion_runs` table is the
  right model — the split is presentation-only.
- **No handler scheduling UI.** Cron config for handlers is a separate feature.
- **No run log/output streaming.** The detail dialog shows metadata only.
