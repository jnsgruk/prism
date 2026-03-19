# Plan 45: ps-workers Handler Review and Remediation

> Comprehensive review of all 8 Restate handlers in the `ps-workers` crate.
> Findings organised into bugs, inconsistencies, duplication opportunities, and
> observability improvements.
>
> **Relationship to Plan 44:** Plan 44 (unified item iteration and failure
> isolation) has been fully implemented. This plan builds on that work. Two
> items from Plan 44 were not completed and are re-identified here:
> - Remove `build_jira_cursor()` / `build_discourse_cursor()` from handlers
>   (Plan 44 Steps 1.3e, 1.4e → this plan item 2.4)
> - Enrichment handler adopt `CompletedWithWarnings` (Plan 44 Step 2.5,
>   deferred as separate PR — not covered here, still outstanding)
>
> Bug 1.3 below (lost email/api_username) was introduced by Plan 44's Phase
> 0.5 refactor of `fetch_batch`.

## Restate Journal Replay Model

**Critical context for safe refactoring.** Verified by reading the Restate SDK
v0.9.0 source (`restate-sdk-shared-core` `vm/transitions/journal.rs`):

- **Replay is positional.** Journal entries are matched by sequential index
  (`commands.pop_front()`), not by `.name()`. Restate replays the Nth
  `ctx.run()` result for the Nth `ctx.run()` call.
- **`.name()` is for observability only.** The SDK docs say: *"This is used
  mainly for observability."* Duplicate names are a debugging inconvenience,
  not a correctness bug.
- **Changing `ctx.run()` order or count breaks replay.** Adding, removing, or
  reordering `ctx.run()` calls between deployments will cause in-flight
  invocations to fail or produce wrong results on replay.

**Safety rule for all refactoring:** The sequence and count of `ctx.run()` calls
within each handler method must remain identical. Extracting code into shared
functions is safe; changing what gets journaled is not. When adding new
`ctx.run()` calls (e.g. `set_current_invocation_id`), acknowledge that
in-flight invocations will fail on replay — this is acceptable for correctness
fixes but must be noted in the deployment plan.

---

## Handler Inventory

| Handler | Type | File | Lines |
| --- | --- | --- | --- |
| `GithubIngestionHandler` | Object | `github_ingestion.rs` | 487 |
| `JiraIngestionHandler` | Object | `jira_ingestion.rs` | 368 |
| `DiscourseIngestionHandler` | Object | `discourse_ingestion.rs` | 351 |
| `GithubTeamSyncHandler` | Object | `github_team_sync.rs` | 450 |
| `IdentityResolutionHandler` | Service | `identity_resolution.rs` | 587 |
| `MetricsComputeHandler` | Service | `metrics_compute.rs` | 131 |
| `EnrichmentHandler` | Service | `enrichment.rs` | 411 |
| `ModelCatalogueHandler` | Service | `model_catalogue.rs` | 221 |
| _Shared boilerplate_ | — | `ingestion_common.rs` | 462 |

---

## 1. Bugs

### 1.1 Missing `set_current_invocation_id` call

**Severity: High**

The `clear_current_invocation_id` is called in `complete_ingestion_run`,
`complete_ingestion_run_with_warnings`, and `fail_ingestion_run`
(ingestion_common.rs:123, 165, 203). However, `set_current_invocation_id` is
**never called anywhere** in the codebase. The CLAUDE.md documents that
"Invocation ID stored in `ingestion_watermarks` via
`set_current_invocation_id()` — enables stale-run reconciliation and
cancellation", but this never actually happens.

**Impact:** Stale-run reconciliation and cancellation are broken. If a handler
dies mid-run, there is no way to detect that the previous invocation is stale
because no invocation ID was ever recorded.

**Fix:** Call `set_current_invocation_id(source_name, invocation_id)` inside a
`ctx.run()` closure immediately after `create_ingestion_run` in each ingestion
handler. The Restate invocation ID should be available via
`ctx.invocation_id()` or equivalent API.

### 1.2 GitHub and Jira handlers don't fail the run on `fetch_store_loop` error

**Severity: High**

When `fetch_store_loop` returns an error, the Discourse handler correctly
catches it and calls `fail_ingestion_run` before propagating
(discourse_ingestion.rs:121-128):

```rust
let result = self.fetch_store_loop(...).await;
let (total_items, final_cursor) = match result {
    Ok(v) => v,
    Err(e) => {
        fail_ingestion_run(...).await;
        return Err(TerminalError::new(...));
    }
};
```

But GitHub (github_ingestion.rs:111) and Jira (jira_ingestion.rs:110) propagate
the error directly with `?`:

```rust
let (total_items, final_cursor) = self.fetch_store_loop(...).await?;
```

**Impact:** If a fetch or store fails mid-loop, the run stays in "Running"
status permanently (orphaned). The UI will show it as still active. Only
Discourse correctly transitions to "Failed".

**Fix:** Wrap `fetch_store_loop` in a match in GitHub and Jira handlers, call
`fail_ingestion_run` before returning the error, matching the Discourse pattern.

### 1.3 `fetch_batch` and `store_batch` discard `email` and `api_username`

**Severity: Medium**

The shared `fetch_batch` (ingestion_common.rs:289-316) and `store_batch`
(ingestion_common.rs:319-362) construct an `IngestionContext` with
`email: None, api_username: None`, discarding credentials that Jira and
Discourse sources may need:

```rust
let ic = IngestionContext {
    repos: state.repos.clone(),
    source_config: config.clone(),
    http_client: state.http_client.clone(),
    token: token.map(String::from),
    email: None,           // <-- always None
    api_username: None,    // <-- always None
};
```

Jira decrypts `email` and Discourse decrypts `api_username`, but these are only
passed to `build_ingestion_context` for `plan()`. If `fetch_batch` or
`store_batch` needs these credentials (e.g. Jira Basic Auth requires
`email:token`), they'd silently fail or fall back to a different auth path.

**Fix:** Change `fetch_batch` and `store_batch` to accept `&IngestionContext`
instead of rebuilding one. This also addresses dedup item 3.4.

### 1.4 Non-unique `ctx.run()` names in identity resolution

**Severity: Low (observability only — see journal replay model above)**

In `identity_resolution.rs`, the side-effect names `email_lookup` (line 415),
`probe_username` (line 448), `store_resolution` (line 515), and
`store_unresolved` (line 542) are static strings. When processing multiple
people in a loop, the journal will contain multiple entries with identical names.

Since replay is positional, this is **not a correctness bug** — but it makes
debugging replays and journal inspection significantly harder because you can't
tell which person a journal entry belongs to.

**Fix:** Include the person_id (or an index) in the side-effect name:
`format!("email_lookup_{}", person.person_id)`,
`format!("probe_{candidate}")`, etc.

### 1.5 Enrichment handler has duplicate `ctx.run()` names across batches

**Severity: Low (observability only)**

Same as 1.4. When the enrichment handler processes multiple batches of the same
type, `log_cost_{type}` (enrichment.rs:309) and `delete_fully_enriched`
(enrichment.rs:377) are reused across batches.

**Fix:** Include a batch counter:
`format!("find_{type}_{batch_num}")`, `format!("log_cost_{type}_{batch_num}")`,
`format!("cleanup_{batch_num}")`.

### 1.6 `model_catalogue` handler doesn't use Restate journaling at all

**Severity: Medium**

The `ModelCatalogueHandler.refresh_catalogue` method accepts `_ctx: Context<'_>`
(model_catalogue.rs:21) but never uses it. All operations including run
creation run outside `ctx.run()`:

- `Uuid::now_v7()` is called outside `ctx.run()`, so retries generate a new
  UUID each time, creating duplicate/orphaned run records.
- API calls and DB writes are not journaled, so the entire handler re-executes
  on Restate replay, including redundant API calls.

**Fix:** At minimum, wrap the `create_run` call in `ctx.run()` so retries reuse
the same run ID. Consider whether the full refresh should be journaled or
whether the current fire-and-forget approach is acceptable given its
idempotency.

### 1.7 `model_catalogue` silently swallows DB errors on key decryption

**Severity: Low**

`decrypt_provider_key` (model_catalogue.rs:207-219) uses
`.ok().flatten()` which collapses database connection errors into `None`
(treated as "no key configured"). A transient DB failure would cause the handler
to skip a provider entirely with no warning.

**Fix:** Log the error before returning `None`, or propagate it to trigger a
retry.

### 1.8 `model_catalogue` should return errors instead of always `Ok(())`

**Severity: Low**

`ModelCatalogueHandler::do_refresh` always returns `Ok(())` even when all
providers fail (model_catalogue.rs:189). This prevents Restate from retrying.

**Fix:** Return `Err(TerminalError)` when `total_models == 0 && had_error`.

---

## 2. Inconsistencies

### 2.1 Secret decryption patterns diverge across handlers

Three different patterns exist for decrypting secrets:

| Pattern | Used by | Description |
| --- | --- | --- |
| DB read outside `ctx.run()`, decrypt outside | `ingestion_common::decrypt_required_secret` | Simplest, used by 3 ingestion handlers |
| DB read inside `ctx.run()`, decrypt outside | `github_team_sync::decrypt_token` | More durable (DB read journaled) |
| DB read outside `ctx.run()`, decrypt outside, swallow errors | `model_catalogue::decrypt_provider_key` | Least safe |
| DB read outside `ctx.run()`, decrypt outside, custom impl | `identity_resolution::decrypt_source_secret_optional` | Reimplements `ingestion_common::decrypt_optional_secret` |

The `github_team_sync` approach is arguably more correct: journaling the
encrypted bytes means Restate won't re-read the DB on replay. However, since
secrets don't change mid-invocation, the difference is academic.

**Fix:** Standardise on one pattern. The `ingestion_common` versions are the
simplest. The team sync and identity resolution handlers should use them.

### 2.2 `github_team_sync` doesn't call `clear_current_invocation_id`

The `complete_run` in `github_team_sync.rs:154-180` only calls
`repos.activity.complete_run()` but does not call
`clear_current_invocation_id()`, unlike the ingestion_common versions.

While `set_current_invocation_id` is currently never called (bug 1.1), once
fixed this inconsistency would mean team sync invocations never clean up their
invocation ID.

**Fix:** Use shared run lifecycle functions (see dedup section 3.1).

### 2.3 Run completion error handling is inconsistent

| Handler | On `complete_run` failure | On `fail_run` failure |
| --- | --- | --- |
| Ingestion (common) | `error!()`, silent | `error!()`, silent |
| Team sync | `error!()`, silent | `error!()`, silent |
| Enrichment | `warn!()`, silent | `warn!()`, silent |
| Identity resolution | `error!()`, silent | N/A (no `fail_run`) |
| Metrics compute | `error!()`, silent | `error!()`, silent |
| Model catalogue | `warn!()`, silent | `warn!()`, silent |

All handlers silently swallow run-lifecycle failures, but the log level varies
between `error!` and `warn!`. These should be uniform.

**Fix:** Standardise on `error!` for run lifecycle failures (they indicate the
system can't track its own state, which is operationally significant).

### 2.4 Cursor building lives in handlers instead of Source trait

GitHub's `fetch_store_loop` uses `source.initial_cursor(&plan)` to get the
first cursor, delegating to the `Source` trait. But Jira and Discourse build
their own cursors in the handler files with `build_jira_cursor()` and
`build_discourse_cursor()`, reaching into source-internal types
(`crate::jira::source::Cursor`, `crate::discourse::source::Cursor`).

This breaks the Source trait abstraction — the handler shouldn't need to know
the internal cursor structure of each source.

**Fix:** Ensure `source.initial_cursor(&plan)` works correctly for all three
sources, and remove `build_jira_cursor` and `build_discourse_cursor` from the
handler files. If the sources need additional config for cursor construction,
pass it through the `IngestionPlan` or `IngestionContext`.

### 2.5 Watermark field names are hardcoded in handlers

Each handler passes a different watermark field name to `advance_watermark`:
- GitHub: `"max_updated_at"` (github_ingestion.rs:140)
- Jira: `"max_updated_at"` (jira_ingestion.rs:137)
- Discourse: `"max_bumped_at"` (discourse_ingestion.rs:147)

This is knowledge that belongs in the Source trait, not the handler.

**Fix:** Add a `watermark_field() -> &str` method to the Source trait, or have
`advance_watermark` be a Source method.

### 2.6 `etag`-based cursor update only used by Jira and Discourse

The pattern where `batch.etag` updates the cursor mid-loop exists in Jira
(jira_ingestion.rs:214-216) and Discourse (discourse_ingestion.rs:234-236) but
not GitHub. GitHub updates `cursor` from `batch.next_cursor`. This works but
makes the loop structures needlessly different.

**Fix:** Unify cursor advancement logic in the shared fetch_store_loop (see 3.2).

---

## 3. Duplication and Refactoring Opportunities

### 3.1 Run lifecycle wrappers duplicated 5 times

The `create_run`, `complete_run`, and `fail_run` ctx.run() wrappers are
reimplemented in every handler:

- `ingestion_common.rs` — `create_ingestion_run`, `complete_ingestion_run`,
  `fail_ingestion_run`, `complete_ingestion_run_with_warnings`
- `github_team_sync.rs` — `create_run`, `complete_run`, `fail_run`
- `enrichment.rs` — `create_run`, `complete_run`, `fail_run`
- `identity_resolution.rs` — `create_run`, `complete_run`
- `metrics_compute.rs` — `create_run`, `complete_run`, `fail_run`

Each copy is 20-30 lines of nearly identical boilerplate. The only variation is
the handler/source name passed to `create_run`, and whether
`clear_current_invocation_id` is called.

**Fix:** Extract generic functions. The Restate SDK provides
`ContextSideEffects` as a blanket-impl trait over all context types
(`Context`, `ObjectContext`, etc.), so we can write generic helpers. See Phase 2
implementation details.

### 3.2 `execute_ingestion` is 80% identical across all three ingestion handlers

The three ingestion handlers follow an identical sequence:

1. Load config by source type key
2. Create run record
3. Decrypt secrets (varies: 1-2 secrets)
4. Build ingestion context
5. Call `source.plan()`
6. Apply override watermark
7. Log plan info
8. Build initial cursor (varies: trait method vs custom function)
9. Run fetch_store_loop (varies: counter types and progress shape)
10. Extract failed_items from final cursor
11. Decide outcome: complete / fail / complete_with_warnings
12. Trigger downstream handlers (varies: metrics only vs metrics + identity)
13. Log completion

Steps 1-2, 5-6, 10-12 are byte-for-byte identical. Steps 3-4 vary only in
which secrets are decrypted. Steps 8-9 could be unified if the Source trait
provided `initial_cursor` and `watermark_field`.

**Fix:** Create a generic `run_ingestion` function in `ingestion_common`. See
Phase 3 implementation details.

### 3.3 `fetch_store_loop` nearly identical across all three handlers

The three `fetch_store_loop` implementations differ in:
- **Counter names**: `prs_fetched`/`reviews_fetched` vs `tickets_fetched` vs `topics_fetched`
- **Cursor update**: GitHub uses `next_cursor`, Jira/Discourse use `etag`
- **Progress JSON builder**: each has its own `build_progress_json` function

The core loop structure (fetch → count → update cursor → store → progress →
check next) is identical.

**Fix:** Create a unified `fetch_store_loop` in `ingestion_common` with a
`ProgressBuilder` trait that each source provides. See Phase 3 implementation
details.

### 3.4 `IngestionContext` rebuilt unnecessarily in `fetch_batch` and `store_batch`

The `fetch_batch` and `store_batch` functions in `ingestion_common` reconstruct
an `IngestionContext` from scratch every call. The caller already has a
correctly-built `IngestionContext` (from `build_ingestion_context`) but can't
pass it in because the current API takes individual parameters.

**Fix:** Change `fetch_batch` and `store_batch` to accept `&IngestionContext`
directly. This fixes bug 1.3 (lost email/api_username), eliminates redundant
construction, and simplifies the call sites.

### 3.5 Failed-items extraction duplicated 3 times

The pattern for extracting `Vec<FailedItem>` from the final cursor JSON is
identical in all three ingestion handlers:

```rust
let failed_items: Vec<FailedItem> =
    serde_json::from_str::<serde_json::Value>(&final_cursor)
        .ok()
        .and_then(|v| v.get("failed_items").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
```

**Fix:** Extract to `ingestion_common::extract_failed_items(cursor: &str) -> Vec<FailedItem>`.

### 3.6 Outcome decision logic duplicated 3 times

The three-way decision (no failures → complete, all failed → fail, partial →
complete_with_warnings) and the associated summary string formatting is
nearly identical across all three handlers, differing only in the noun
("repo(s)" vs "project(s)" vs "category(s)").

**Fix:** Extract to a shared `finalise_run` function. See Phase 2 implementation
details.

---

## 4. Logging and Observability Redesign

### 4.0 Current state audit

There are **152 log statements** across 24 files in ps-workers. The problems:

**Too much noise at `info` level.** A single GitHub ingestion run with 20 repos
produces ~60+ `info!` lines: "starting ingestion", "plan ready", then per-repo
"fetching PRs from...", "fetched page (N items, M reviews)", "completed repo",
per-batch "stored batch", plus "stored batch with unresolved identities",
"advanced watermark", "triggering metrics", "ingestion complete". The source
adapter layer (`github/source/fetch.rs` alone has 12 `info!` calls) is
especially chatty — it logs every phase transition, every repo start, every
batch result, every member search batch.

**Per-batch `info!` in the handler loop.** Every `store_batch` call emits:
```
info!(source, batch_stored, total_items, "stored batch")
```
A Jira backfill with 15,000 batches produces 15,000 identical log lines that
differ only in the counter values. This is progress information, not a discrete
event — it belongs in the progress JSON (which is already written to the DB),
not in the log stream.

**No structured context propagation.** Log lines include `source = source_name`
as an ad-hoc field, but there's no tracing span, so:
- `run_id` is never in log output — can't correlate to a specific run
- Source adapter logs (`github/source/fetch.rs`, `jira/source/fetch.rs`) don't
  inherit any handler context — they add their own `source = ctx.source_config.name`
- Duration is never recorded

**Inconsistent field naming.** The `source` field is sometimes `source`,
sometimes `source = %source_name`, sometimes `source = source_name`. Error
fields are sometimes `error = %e`, sometimes just `"message: {e}"` in the
format string. The structured field conventions from CLAUDE.md
(`tracing::info!(repo = %name, count = items.len(), "fetched items")`) aren't
consistently followed.

**`warn!` for routine best-effort operations.** Every progress update failure
emits `warn!`, but progress updates are explicitly documented as best-effort.
Logging these as `warn!` creates alert fatigue — they're expected during DB
hiccups and don't indicate anything actionable.

**Mixed levels in source adapters.** `github/source/fetch.rs` uses `info!` for
things like "fetching PRs from repo" and "completed repo" — these are internal
progress steps within a single `fetch_batch` call. They should be `debug!` at
most, since the handler already logs batch progress.

### 4.1 Design: Log level policy

Define what each level means in the context of a worker handler:

| Level | When to use | Audience | Examples |
| --- | --- | --- | --- |
| `error!` | **System integrity at risk.** The handler cannot record its own state, or infrastructure is broken. Requires investigation. | On-call | Run lifecycle failure (can't write complete/fail to DB), DB connection error, health server crash |
| `warn!` | **Degraded but continuing.** Something unexpected happened but the handler is still making progress. May need attention if it persists. | On-call (if repeated) | Rate limit approaching, per-item fetch failure (skipped), budget exceeded, API error on non-critical path |
| `info!` | **Discrete lifecycle events.** One line per handler invocation start, one per completion. Optionally one per major phase transition (not per batch). Should be scannable at a glance. | Operators | Handler started, handler completed (with summary), phase transition |
| `debug!` | **Internal progress and diagnostics.** Per-batch, per-repo, per-page details. Useful for debugging a specific run. | Developers | Batch stored, repo fetched, cursor state, API response details |

**Key principle: at `info` level, a complete successful ingestion run should
produce 2-5 log lines, not 60.** The detailed progress lives in the DB's
`progress_detail` JSONB column and is visible in the admin UI. Logs are for
events, not progress bars.

### 4.2 Design: Tracing spans for context propagation

Instead of passing `source = source_name` as an ad-hoc field on every log call,
use a `tracing::Span` that propagates context automatically. Every log line
within the span inherits its fields.

**Handler-level span** (created after `create_run`):

```rust
let span = tracing::info_span!(
    "handler",
    handler = spec.handler_name,
    source = %source_name,
    run_id = %run_id,
);
```

All code within the handler executes inside this span. Every `info!`, `warn!`,
`error!`, `debug!` automatically includes `handler`, `source`, and `run_id` in
its JSON output without callers needing to add them.

**Source adapter code** (`github/source/fetch.rs`, etc.) doesn't need any span
changes — it already runs within the handler's span via the call stack. Remove
all manual `source = ctx.source_config.name` annotations from source adapter
log calls; the span provides this automatically.

**Non-ingestion handlers** (enrichment, identity resolution, metrics, model
catalogue, team sync) get the same pattern — a span with `handler` and `run_id`
after run creation.

### 4.3 Design: What to log at each level

#### `info!` — lifecycle events only

Each handler should emit **exactly these** `info!` lines (and no others):

**Ingestion handlers (GitHub, Jira, Discourse):**
```
info!("starting ingestion");                     // or "starting backfill"
info!(items = total, duration_secs, "complete"); // one-line summary
```

If the run has warnings or failures, that detail goes in `warn!` (see below).
The `info!` completion line always fires and always has the same shape — makes
it easy to grep/alert on.

**Team sync:**
```
info!("starting team sync");
info!(teams = total, duration_secs, "complete");
```

**Enrichment:**
```
info!("starting enrichment cycle");
info!(processed = total, errors = total, duration_secs, "complete");
```

**Identity resolution:**
```
info!("starting identity resolution");
info!(resolved = total, duration_secs, "complete");
```

**Metrics compute:**
```
info!("starting metrics compute");
info!(snapshots = total, duration_secs, "complete");
```

**Model catalogue:**
```
info!("starting catalogue refresh");
info!(models = total, duration_secs, "complete");
```

That's it. Two `info!` lines per handler invocation. Everything else moves down
to `debug!` or up to `warn!`.

#### `warn!` — actionable degradation

```rust
// Rate limit pressure (already in progress JSON, but also log it)
warn!(remaining, limit, "rate limit pressure");

// Per-item failure that was isolated (run continues)
warn!(item = %key, error = %e, "item skipped");

// Budget exceeded (enrichment paused)
warn!(cap, "daily budget exceeded");

// API error on a specific source during identity resolution
warn!(error = %e, "source resolution failed, continuing");
```

`warn!` should **not** be used for:
- Progress update write failures (change to `debug!` — these are explicitly
  best-effort and expected during transient DB issues)
- "triggering metrics recomputation" / "triggering identity resolution" (change
  to `debug!` — these are routine fire-and-forget calls)
- "no repos to ingest" / "no pending resolutions" (change to `debug!` —
  no-op runs are not warnings)

#### `error!` — system integrity

```rust
// Can't write run completion to DB — system can't track its own state
error!(error = %e, "failed to record run completion");

// Same for run failure
error!(error = %e, "failed to record run failure");
```

Nothing else in the handler layer should be `error!`. API failures, fetch
errors, item-level failures — these are `warn!` (degraded) or handled via the
run's error_message field.

#### `debug!` — everything else

All the current per-batch, per-repo, per-page logging moves here:

```rust
// Per-batch in fetch_store_loop
debug!(batch_stored = stored, total_items, "stored batch");

// Per-repo in GitHub fetch
debug!(repo = %format!("{owner}/{repo}"), items = items.len(), "fetched repo page");

// Per-page in Jira/Discourse
debug!(page, items = items.len(), "fetched page");

// Phase transitions
debug!("transitioning to member search phase");

// Cursor state
debug!(watermark = ?plan.watermark, repos = plan.repos.len(), "plan ready");

// Downstream triggers
debug!("triggering metrics recomputation");
debug!("triggering identity resolution");

// Progress write failures (best-effort)
debug!(error = %e, "failed to update run progress");
```

### 4.4 Design: Periodic progress at `info` level

The concern with only 2 `info!` lines per run is that a long backfill (hours)
gives no signal until completion. Add a **time-based periodic progress** log
at `info!` in the fetch-store loop:

```rust
let mut last_progress_log = Instant::now();

loop {
    // ... fetch, store ...

    if last_progress_log.elapsed() >= Duration::from_secs(60) {
        info!(
            total_items,
            batches,
            "progress"
        );
        last_progress_log = Instant::now();
    }

    // ... rest of loop
}
```

This gives one `info!` line per minute during long runs — enough to confirm
the handler is alive without flooding the logs. The span provides source and
run_id context automatically. The progress JSON in the DB has the detailed
breakdown (repos completed, current repo, rate limit, etc.).

For non-ingestion handlers (enrichment, identity resolution), the same pattern
applies — emit a periodic `info!` if the loop has been running for > 60s.

### 4.5 Design: Duration tracking

Record `Instant::now()` at handler start. Include `duration_secs` in the
completion `info!` line (shown in 4.3 above). This is a structured field so
it's searchable/aggregatable in log systems.

For the `progress_detail` JSON, also include a `"elapsed_secs"` field so the
UI can show running time.

### 4.6 Design: Structured field conventions

Standardise field naming across all handlers. These fields go in the span
(automatic on every line) or in individual log calls:

| Field | Source | Used in |
| --- | --- | --- |
| `handler` | Span | All lines — identifies which handler type |
| `source` | Span | All lines — source config name |
| `run_id` | Span | All lines — UUID for log-to-run correlation |
| `total_items` | Log call | Completion line, periodic progress |
| `duration_secs` | Log call | Completion line |
| `error` | Log call | `warn!`/`error!` — always `error = %e` format |
| `item` | Log call | `warn!` for per-item failures — `item = %key` |
| `remaining` | Log call | Rate limit warnings |

**Never** embed dynamic values in the format string (`"failed: {e}"`) — always
use structured fields (`error = %e, "operation failed"`). This makes log
aggregation and filtering work correctly.

### 4.7 Design: Progress reporting for all handlers

Currently 3 of 8 handlers have no progress reporting (identity resolution,
metrics compute, team sync). Add `update_run_progress_detail` calls to these.

**Identity resolution** — after each person:
```json
{"phase": "resolving", "source": "discourse_ubuntu", "resolved": 12, "pending": 45, "status_message": "Resolved 12/45 identities"}
```

**Metrics compute** — after each period:
```json
{"phase": "quarter", "snapshots": 24, "status_message": "Computed quarter snapshots (24 teams)"}
```

**Team sync** — after each org's teams are synced:
```json
{"phase": "syncing", "org": "canonical", "teams_synced": 8, "status_message": "Synced 8 teams for canonical"}
```

### 4.8 Concrete changes by file

Summary of level changes across all files. **D** = demote to `debug!`,
**K** = keep, **R** = remove, **M** = merge into completion line.

#### Handlers

| File | Current | Action |
| --- | --- | --- |
| `github_ingestion.rs:34` | `info!("starting ingestion run")` | **K** — simplify to `info!("starting ingestion")` |
| `github_ingestion.rs:48` | `info!("starting backfill")` | **K** |
| `github_ingestion.rs:98` | `info!(repos, watermark, "ingestion plan ready")` | **D** |
| `github_ingestion.rs:106` | `info!("no repos to ingest")` | **D** |
| `github_ingestion.rs:181` | `info!("triggering metrics recomputation")` | **D** |
| `github_ingestion.rs:187` | `info!(total_items, "ingestion complete")` | **K** — add `duration_secs` |
| `github_ingestion.rs:230` | `info!(batch_stored, total_items, "stored batch")` | **D** |
| `github_ingestion.rs:254` | `warn!("failed to update run progress")` | **D** |
| `github_ingestion.rs:277` | `warn!("failed to update final progress")` | **D** |
| `jira_ingestion.rs` | Same pattern as GitHub | Same changes |
| `discourse_ingestion.rs` | Same pattern as GitHub | Same changes |
| `discourse_ingestion.rs:200` | `info!("triggering identity resolution")` | **D** |
| `enrichment.rs:100` | `info!("daily budget exceeded")` | Change to **`warn!`** |
| `enrichment.rs:118` | `info!("processing enrichment batch")` | **D** |
| `enrichment.rs:384` | `info!("cleaned up fully enriched")` | **D** |
| `enrichment.rs:335,359` | `warn!("failed to complete/fail run")` | Change to **`error!`** |
| `enrichment.rs:313` | `warn!("failed to log enrichment cost")` | **D** |
| `enrichment.rs:407` | `warn!("failed to update enrichment progress")` | **D** |
| `github_team_sync.rs:226` | `info!("discovered GitHub teams")` | **D** |
| `github_team_sync.rs:240` | `info!("fetched team details")` | **D** |
| `github_team_sync.rs:257` | `info!("removed stale GitHub teams")` | **D** |
| `identity_resolution.rs:29` | `info!("no enabled Discourse sources")` | **D** |
| `identity_resolution.rs:34` | `info!("found Discourse sources")` | **D** |
| `identity_resolution.rs:43` | `info!("resolved identities for source")` | **D** — per-source detail |
| `identity_resolution.rs:206` | `info!("created pending resolution rows")` | **D** |
| `identity_resolution.rs:244` | `info!("resolving pending identities")` | **D** |
| `identity_resolution.rs:574` | `info!("backfilled contribution person_ids")` | **D** |
| `metrics_compute.rs:31` | `info!("recomputed snapshots")` | **D** — per-period detail |
| `model_catalogue.rs:89` | `info!("skipping — no API key")` | **D** |
| `model_catalogue.rs:160` | `info!("model catalogue refreshed")` | **D** — per-provider detail |
| `model_catalogue.rs:171,185` | `warn!("failed to mark/complete run")` | Change to **`error!`** |
| `model_catalogue.rs:202` | `warn!("failed to update progress")` | **D** |
| All `ingestion_common.rs` | `error!("failed to update run status")` | **K** |

#### Source adapters

| File | Lines affected | Action |
| --- | --- | --- |
| `github/source/fetch.rs` | 12 `info!` calls | **D** all — per-repo, per-page, per-phase detail |
| `github/source/store.rs` | 2 `info!` calls | **D** both — per-batch, watermark advance |
| `github/source/plan.rs` | 3 `info!` calls | **D** all |
| `jira/source/fetch.rs` | 1 `info!` call | **D** |
| `jira/source/store.rs` | 2 `info!` calls | **D** both |
| `jira/source/plan.rs` | 2 `info!` calls | **D** both |
| `discourse/source/fetch.rs` | 1 `info!` call | **D** |
| `discourse/source/store.rs` | 2 `info!` calls | **D** both |
| `discourse/source/plan.rs` | 2 `info!` calls | **D** both |
| All source `warn!` calls | Per-item errors, enrichment queue failures | **K** — these are legitimate degradation |

**Remove** all manual `source = ctx.source_config.name` annotations from source
adapter log calls once the handler span is in place.

### 4.9 Expected result

**Before (info level, GitHub run with 20 repos):** ~60+ lines
```
starting ingestion run
ingestion plan ready (repos=20, watermark=2026-03-18)
fetching PRs from canonical/lxd (1/20)
fetched page (15 items, 8 reviews)
completed repo canonical/lxd
stored batch (15 items)
fetching PRs from canonical/juju (2/20)
... (x18 more repos)
triggering metrics recomputation
ingestion complete (total=312)
```

**After (info level, same run):** 2-3 lines
```json
{"handler":"GithubIngestionHandler","source":"github","run_id":"019...","msg":"starting ingestion"}
{"handler":"GithubIngestionHandler","source":"github","run_id":"019...","total_items":312,"duration_secs":47,"msg":"complete"}
```

For a long backfill (>60s), add periodic progress:
```json
{"handler":"GithubIngestionHandler","source":"github","run_id":"019...","total_items":312,"duration_secs":47,"msg":"starting ingestion"}
{"handler":"GithubIngestionHandler","source":"github","run_id":"019...","total_items":1240,"batches":82,"msg":"progress"}
{"handler":"GithubIngestionHandler","source":"github","run_id":"019...","total_items":2891,"batches":194,"msg":"progress"}
{"handler":"GithubIngestionHandler","source":"github","run_id":"019...","total_items":3412,"duration_secs":187,"msg":"complete"}
```

**At `debug` level**, all the per-repo, per-batch, per-page detail is still
available — you just have to ask for it:

```
RUST_LOG=ps_workers=debug
```

Or, for a specific source only:

```
RUST_LOG=info,ps_workers::github=debug
```

---

## 5. Resilience Improvements

### 5.1 Ingestion handlers should catch panics in fetch/store

If a panic occurs inside `source.fetch_batch()` or `source.store_batch()`, the
entire Restate invocation crashes. The run stays in "Running" state with no
error message.

**Fix:** Use `std::panic::AssertUnwindSafe` + `FutureExt::catch_unwind()` or
`tokio::spawn` to isolate panics and convert them to errors that can be
recorded on the run.

### 5.2 Rely on Restate's inactivity timeout

No handler should enforce its own run duration limit. Backfills can legitimately
take hours. Restate's inactivity timeout (configurable per deployment) is the
right mechanism for detecting stuck handlers — it fires when a handler stops
making progress (no `ctx.run()` completions, no `ctx.sleep()` resumptions),
which correctly distinguishes "slow but progressing" from "stuck in an infinite
loop".

**Action:** Ensure the Restate inactivity timeout is configured appropriately
for the deployment (default is fine for now). No code changes needed.

---

## 6. Detailed Implementation

### Phase 1: Bug fixes

Each step is a single commit. Steps are independent unless noted.

#### Step 1a: Fix orphaned runs in GitHub and Jira handlers

**Files:** `github_ingestion.rs`, `jira_ingestion.rs`

**Change:** In `execute_ingestion`, wrap the `fetch_store_loop` call in a match
block to catch errors and record run failure before propagating. Match the
existing Discourse pattern exactly.

```rust
// github_ingestion.rs — replace lines 111-121
let result = self
    .fetch_store_loop(ctx, run_id, source_name, config, source.as_ref(), &plan, ing_ctx.token.as_deref())
    .await;

let (total_items, final_cursor) = match result {
    Ok(v) => v,
    Err(e) => {
        let msg = e.to_string();
        fail_ingestion_run(ctx, &self.state.repos, run_id, source_name, &msg).await;
        return Err(TerminalError::new(format!("ingestion failed: {msg}")));
    }
};
```

Same pattern in `jira_ingestion.rs`.

**Journal safety:** No change to `ctx.run()` call sequence. `fail_ingestion_run`
is only reached on the error path where no further journal entries would have
been written anyway.

**Commit:** `fix: fail ingestion run on fetch_store_loop error in GitHub/Jira handlers`

---

#### Step 1b: Pass `IngestionContext` through `fetch_batch` and `store_batch`

**Files:** `ingestion_common.rs`, `github_ingestion.rs`, `jira_ingestion.rs`,
`discourse_ingestion.rs`

This fixes bug 1.3 and addresses dedup items 3.4 at the same time. The key
constraint is that `store_batch` runs inside `ctx.run()`, so the
`IngestionContext` must be cloneable into the closure.

**Change in `ingestion_common.rs`:**

Replace `fetch_batch` signature:

```rust
// Before:
pub(super) async fn fetch_batch(
    state: &SharedState,
    config: &SourceConfig,
    cursor: &str,
    token: Option<&str>,
) -> Result<SerFetchResult, TerminalError>

// After:
pub(super) async fn fetch_batch(
    ing_ctx: &IngestionContext,
    cursor: &str,
) -> Result<SerFetchResult, TerminalError>
```

Body: use `ing_ctx` directly instead of rebuilding an `IngestionContext`.
`registry::create_source` uses `ing_ctx.source_config.source_type`.

Replace `store_batch` signature:

```rust
// Before:
pub(super) async fn store_batch(
    ctx: &ObjectContext<'_>,
    state: &SharedState,
    config: &SourceConfig,
    items: &[ContributionInput],
    token: Option<&str>,
) -> Result<i32, TerminalError>

// After:
pub(super) async fn store_batch(
    ctx: &ObjectContext<'_>,
    ing_ctx: &IngestionContext,
    items: &[ContributionInput],
) -> Result<i32, TerminalError>
```

Body: clone `ing_ctx` into the `ctx.run()` closure (it's already Clone-able).

**Remove** `build_ingestion_context` — callers construct `IngestionContext`
directly (it's a simple struct).

**Update all three handler call sites** to pass `&ing_ctx` instead of
`(&self.state, config, token)`.

**Also update `advance_watermark`** to take `&IngestionContext` for the same
reason.

**Journal safety:** The `ctx.run()` closures in `store_batch` and
`advance_watermark` execute the same code with the same journal name
(`"store_batch"`, `"advance_watermark"`). The only difference is that `email`
and `api_username` are now populated. Since these are consumed inside the
closure (not serialized to the journal), replay is unaffected.

**Commit:** `fix: pass full IngestionContext to fetch_batch/store_batch`

---

#### Step 1c: Fix model catalogue journaling and error handling

**File:** `model_catalogue.rs`

Three small fixes in one commit:

1. **Use `ctx.run()` for run creation** — change `do_refresh` to accept
   `ctx: &Context<'_>` (remove the `_` prefix). Wrap the `create_run` call:

```rust
let run_id = {
    let repos = self.state.repos.clone();
    ctx.run(|| {
        let repos = repos.clone();
        async move {
            let id = Uuid::now_v7();
            repos.activity.create_run(id, "_model_catalogue", "ModelCatalogueHandler", "refresh_catalogue")
                .await
                .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
            Ok(Json::from(id.to_string()))
        }
    })
    .name("create_run")
    .await?
    .into_inner()
    .parse()
    .map_err(|e| TerminalError::new(format!("invalid run_id: {e}")))?
};
```

2. **Log errors in key decryption** — change `decrypt_provider_key` to log
   before returning None:

```rust
async fn decrypt_provider_key(&self, secret_key_name: &str) -> Option<String> {
    let encrypted = match self.state.repos.config.get_global_secret(secret_key_name).await {
        Ok(Some(enc)) => enc,
        Ok(None) => return None,
        Err(e) => {
            tracing::warn!(key = secret_key_name, error = %e, "failed to read provider secret");
            return None;
        }
    };
    // ... rest unchanged
}
```

3. **Return error when all providers fail** — change the `Ok(())` at the end
   of `do_refresh` to propagate:

```rust
if total_models == 0 && had_error {
    // ... existing fail_run logic ...
    return Err(TerminalError::new(err_msg));
}
```

**Journal safety:** Adds one new `ctx.run()` call at the beginning. In-flight
invocations that were mid-replay will fail (the first journal entry was
previously something else). This is acceptable — model catalogue refreshes are
short-lived and fully idempotent, so the retry will just re-execute the whole
thing.

**Commit:** `fix: model catalogue journaling, error logging, and error propagation`

---

#### Step 1d: Make `ctx.run()` names unique in identity resolution and enrichment

**Files:** `identity_resolution.rs`, `enrichment.rs`

Not a correctness fix (replay is positional), but important for debuggability
when inspecting Restate journals.

**Identity resolution** — add person index to names:

```rust
// In resolve_person, pass person_index from the caller's enumerate()
.name(format!("email_lookup_{person_index}"))
.name(format!("probe_{person_index}_{candidate_index}"))
.name(format!("store_resolution_{person_index}"))
.name(format!("store_unresolved_{person_index}"))
```

Also add person_index to `load_candidates`:
```rust
.name(format!("load_candidates_{person_index}"))
```

This requires threading a `person_index: usize` through `resolve_person`.

**Enrichment** — add batch counter:

```rust
// In the inner loop, track batch_num per type
.name(format!("find_{enrichment_type}_{batch_num}"))
.name(format!("log_cost_{enrichment_type}_{batch_num}"))
.name(format!("cleanup_{cleanup_counter}"))
```

**Journal safety:** Name changes don't affect positional replay. Safe for
in-flight invocations.

**Commit:** `refactor: make ctx.run() names unique for journal debuggability`

---

### Phase 2: Deduplication (safe extractions)

All changes in this phase are pure code extractions — the `ctx.run()` call
sequence within each handler remains identical.

#### Step 2a: Extract shared run lifecycle module

**New file:** `handlers/run_lifecycle.rs`
**Modified files:** `handlers/mod.rs`, all 5 handler files with duplicated
lifecycle wrappers.

The Restate SDK's `ContextSideEffects` trait is blanket-implemented for all
context types via `SealedContext`, so we can write:

```rust
use restate_sdk::prelude::*;
use ps_core::repo::Repos;
use uuid::Uuid;

/// Create a run record inside a journaled `ctx.run()` closure.
pub(super) async fn create_run<'ctx>(
    ctx: &(impl ContextSideEffects<'ctx> + Send + Sync),
    repos: &Repos,
    source: &str,
    handler: &str,
    method: &str,
) -> Result<Uuid, TerminalError> {
    let repos = repos.clone();
    let source = source.to_string();
    let handler = handler.to_string();
    let method = method.to_string();
    ctx.run(|| {
        let repos = repos.clone();
        let source = source.clone();
        let handler = handler.clone();
        let method = method.clone();
        async move {
            let id = Uuid::now_v7();
            repos
                .activity
                .create_run(id, &source, &handler, &method)
                .await
                .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
            Ok(Json::from(id.to_string()))
        }
    })
    .name("create_run")
    .await?
    .into_inner()
    .parse()
    .map_err(|e| TerminalError::new(format!("invalid run_id: {e}")))
}

/// Mark a run as complete inside a journaled `ctx.run()` closure.
pub(super) async fn complete_run<'ctx>(
    ctx: &(impl ContextSideEffects<'ctx> + Send + Sync),
    repos: &Repos,
    run_id: Uuid,
    source_name: &str,
    items_collected: i32,
) {
    let repos = repos.clone();
    let sn = source_name.to_string();
    let result = ctx
        .run(|| {
            let repos = repos.clone();
            let sn = sn.clone();
            async move {
                repos.activity.complete_run(run_id, items_collected).await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                repos.activity.clear_current_invocation_id(&sn).await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                Ok(Json::from(()))
            }
        })
        .name("complete_run")
        .await;

    if let Err(e) = result {
        error!(source = source_name, "failed to update run status: {e}");
    }
}
```

Same pattern for `fail_run` and `complete_run_with_warnings`.

**Important: verify `ContextSideEffects` trait bounds.** Before implementing,
confirm that the generic bound `impl ContextSideEffects<'ctx> + Send + Sync`
compiles. The `SealedContext` trait is private, so the public
`ContextSideEffects` is the right bound. If the trait bound doesn't work due
to lifetime issues, fall back to two concrete functions (one for
`ObjectContext`, one for `Context`) with a shared inner function for the
closure body.

**Migration strategy:** Replace one handler at a time. After each handler is
migrated, run `prek run -av` to verify. Start with `metrics_compute` (simplest,
no `clear_current_invocation_id` complexity) to validate the generic approach.

Order:
1. `metrics_compute.rs` — validate approach
2. `identity_resolution.rs` — validate with `Context` type
3. `enrichment.rs`
4. `github_team_sync.rs`
5. `ingestion_common.rs` — the existing `ObjectContext`-specific versions

**Journal safety:** Each handler's `ctx.run()` call sequence is unchanged. The
code is just moved to a shared function.

**Commit:** `refactor: extract shared run lifecycle functions`

---

#### Step 2b: Extract `extract_failed_items` and `finalise_run`

**File:** `ingestion_common.rs`

Add:

```rust
/// Extract failed items from the final cursor JSON.
pub(super) fn extract_failed_items(cursor: &str) -> Vec<FailedItem> {
    serde_json::from_str::<serde_json::Value>(cursor)
        .ok()
        .and_then(|v| v.get("failed_items").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Finalise an ingestion run based on whether there were failures.
///
/// Three outcomes:
/// - No failures → advance watermark + complete
/// - All items failed (total_items == 0) → fail
/// - Partial failure → complete with warnings (do NOT advance watermark)
pub(super) async fn finalise_run(
    ctx: &ObjectContext<'_>,
    state: &SharedState,
    ing_ctx: &IngestionContext,
    run_id: Uuid,
    source_name: &str,
    total_items: i32,
    failed_items: &[FailedItem],
    item_noun: &str,
    final_cursor: &str,
    watermark_field: &str,
) -> Result<(), TerminalError> {
    if failed_items.is_empty() {
        if total_items > 0 {
            advance_watermark(ctx, ing_ctx, final_cursor, total_items, watermark_field).await?;
        }
        complete_run(ctx, &state.repos, run_id, source_name, total_items).await;
    } else if total_items == 0 {
        let summary = format!(
            "all {} {item_noun}(s) failed: {}",
            failed_items.len(),
            failed_items.iter().map(|f| f.key.as_str()).collect::<Vec<_>>().join(", ")
        );
        fail_run(ctx, &state.repos, run_id, source_name, &summary).await;
    } else {
        let summary = format!(
            "{} {item_noun}(s) failed: {}",
            failed_items.len(),
            failed_items.iter().map(|f| f.key.as_str()).collect::<Vec<_>>().join(", ")
        );
        let metadata = serde_json::json!({ "failed_items": failed_items });
        complete_run_with_warnings(ctx, &state.repos, run_id, source_name, total_items, &summary, metadata).await;
    }
    Ok(())
}
```

**Update all three ingestion handlers** to replace their inline versions with
calls to `extract_failed_items` and `finalise_run`.

**Journal safety:** `finalise_run` calls the same `ctx.run()` wrappers
(`complete_run`, `fail_run`, `complete_run_with_warnings`, `advance_watermark`)
in the same conditional structure. The journal entry that gets written depends
on the outcome, which is the same logic as before.

**Commit:** `refactor: extract shared finalise_run and extract_failed_items`

---

#### Step 2c: Move cursor building into Source trait

**Files:** `jira_ingestion.rs`, `discourse_ingestion.rs`,
`jira/source/mod.rs`, `discourse/source/mod.rs`

Jira and Discourse already have `initial_cursor` on their Source trait impl,
but the handler-side `build_jira_cursor()` / `build_discourse_cursor()` override
them because the Source impl's version doesn't have access to the full config.

**Fix:** The Source trait's `initial_cursor(&self, plan: &IngestionPlan)` method
should be sufficient if the plan contains the right data. Check what each
`build_*_cursor` function reads from `config.settings` that isn't already in
the plan:

- **Jira:** `base_url`, `story_points_field`, `api_mode`, `projects`
- **Discourse:** `base_url`, `categories`, `min_posts`, instance name

The `plan()` method already has access to `IngestionContext` which includes
`source_config`. So `initial_cursor` should also take `&IngestionContext` (or
the plan should be enriched).

**Simplest approach:** Change `Source::initial_cursor` to take
`&IngestionContext` and `&IngestionPlan`:

```rust
fn initial_cursor(&self, ctx: &IngestionContext, plan: &IngestionPlan) -> String;
```

Then the Jira/Discourse Source impls can read `ctx.source_config.settings`
directly, and the handler-side `build_*_cursor` functions become dead code.

**Journal safety:** No `ctx.run()` changes. Cursor construction is pure
computation.

**Commit:** `refactor: move cursor building into Source::initial_cursor`

---

#### Step 2d: Move watermark field name into Source trait

**Files:** `ps-core/src/ingestion.rs` (Source trait), source impls,
`ingestion_common.rs`

Add to Source trait:

```rust
fn watermark_field(&self) -> &str;
```

Implement:
- `GitHubSource` → `"max_updated_at"`
- `JiraSource` → `"max_updated_at"`
- `DiscourseSource` → `"max_bumped_at"`

Update `advance_watermark` to read from source instead of taking a parameter,
or have callers pass `source.watermark_field()`.

**Journal safety:** No `ctx.run()` changes. The watermark field value passed
to the DB is the same.

**Commit:** `refactor: move watermark field name into Source trait`

---

### Phase 3: Unified ingestion loop

This phase is higher risk because it restructures handler code. Each step must
be validated carefully.

**Invariant:** After this phase, each ingestion handler's `run_ingestion` and
`backfill` methods are thin wrappers (~5 lines) that call shared code. The
`ctx.run()` call sequence is:
1. `load_config`
2. `create_run`
3. N × (`store_batch`) — one per batch
4. One of: `complete_run` / `fail_run` / `complete_run_with_warnings` + `advance_watermark`

This sequence is unchanged from the current code.

#### Step 3a: Standardise secret decryption

**Files:** `github_team_sync.rs`, `identity_resolution.rs`,
`ingestion_common.rs`

Replace `github_team_sync::decrypt_token` and
`identity_resolution::decrypt_source_secret_optional` with calls to the
existing `ingestion_common::decrypt_required_secret` /
`decrypt_optional_secret`.

For `github_team_sync`, this means removing the `load_encrypted_token` journaled
step. The encrypted bytes read will move from inside `ctx.run()` to outside.
This is acceptable because the encrypted bytes don't change mid-invocation.

**Journal safety for team sync:** This **removes** one `ctx.run()` call
(`load_encrypted_token`) from `sync_teams`. In-flight team sync invocations
will fail on replay because the journal entry sequence has changed. This is
acceptable because:
- Team sync is a short-lived operation (seconds)
- The retry will succeed with the new code
- Team sync is idempotent (upserts)

**Deploy note:** After deploying this change, any in-flight `sync_teams`
invocations may fail once and retry successfully.

**Commit:** `refactor: standardise secret decryption on ingestion_common`

---

#### Step 3b: Create unified `fetch_store_loop`

**Files:** `ingestion_common.rs`, all three ingestion handlers

Define a `ProgressTracker` trait that sources implement:

```rust
/// Source-specific progress tracking for the fetch-store loop.
pub(super) trait ProgressTracker {
    /// Count an item from a fetched batch (e.g. increment PR/ticket/topic counter).
    fn count_item(&mut self, item: &ContributionInput);

    /// Build a progress JSON object from current counters and cursor state.
    fn build_progress(
        &self,
        cursor: &str,
        rate_limit: Option<&RateLimitInfo>,
    ) -> serde_json::Value;

    /// Build the final "complete" progress JSON.
    fn build_final_progress(&self) -> serde_json::Value;
}
```

Create three implementations (can live in each handler file or in the source
modules):
- `GithubProgressTracker` — tracks `prs_fetched`, `reviews_fetched`, `identities_skipped`
- `JiraProgressTracker` — tracks `tickets_fetched`
- `DiscourseProgressTracker` — tracks `topics_fetched`

Create the unified loop in `ingestion_common`:

```rust
pub(super) async fn fetch_store_loop(
    ctx: &ObjectContext<'_>,
    ing_ctx: &IngestionContext,
    run_id: Uuid,
    source_name: &str,
    initial_cursor: &str,
    tracker: &mut dyn ProgressTracker,
) -> Result<(i32, String), TerminalError> {
    let mut cursor = initial_cursor.to_string();
    let mut total_items = 0i32;

    loop {
        let batch = fetch_batch(ing_ctx, &cursor).await?;

        for item in &batch.items {
            tracker.count_item(item);
        }

        // Update cursor from etag if present (Jira/Discourse pattern),
        // which carries watermark state even when next_cursor is None.
        if let Some(ref latest) = batch.etag {
            cursor = latest.clone();
        }

        if !batch.items.is_empty() {
            let stored = store_batch(ctx, ing_ctx, &batch.items).await?;
            total_items += stored;
            info!(source = source_name, batch_stored = stored, total_items, "stored batch");
        }

        let progress = tracker.build_progress(&cursor, batch.rate_limit.as_ref());
        if let Err(e) = ing_ctx.repos
            .activity
            .update_run_progress_detail(run_id, total_items, &progress)
            .await
        {
            warn!(source = source_name, "failed to update run progress: {e}");
        }

        let Some(next_cursor) = batch.next_cursor else {
            break;
        };
        // For GitHub, cursor comes from next_cursor. For Jira/Discourse,
        // cursor was already updated from etag above. In all cases, if
        // next_cursor is Some, that's the authoritative next position.
        cursor = next_cursor;
    }

    // Final progress
    let final_progress = tracker.build_final_progress();
    if let Err(e) = ing_ctx.repos
        .activity
        .update_run_progress_detail(run_id, total_items, &final_progress)
        .await
    {
        warn!(source = source_name, "failed to update final progress: {e}");
    }

    Ok((total_items, cursor))
}
```

**Important subtlety — cursor vs etag vs next_cursor:** The current GitHub
handler only uses `next_cursor` to advance the cursor (no `etag`). Jira and
Discourse use `etag` to carry state (like `max_updated_at`) even on the
final batch where `next_cursor` is None. The unified loop must handle both:
- `batch.etag` is always applied if present (updates watermark state)
- `batch.next_cursor` determines whether the loop continues

This matches the Jira/Discourse pattern. For GitHub, `batch.etag` is always
None, so the `if let Some` is a no-op. Verify this by checking that
`GitHubSource::fetch_batch` never sets `etag`.

**Journal safety:** The `ctx.run()` calls inside `store_batch` are unchanged.
The loop structure is identical. The only difference is that the counter
tracking is delegated to a trait instead of inline code.

**Migration strategy:** Implement the unified loop and `ProgressTracker`, then
migrate one handler at a time:
1. Jira (simplest counter structure)
2. Discourse (same as Jira)
3. GitHub (most complex progress builder)

After each migration, run `prek run -av`.

**Commit:** `refactor: unified fetch_store_loop with ProgressTracker trait`

---

#### Step 3c: Create unified `execute_ingestion`

**Files:** `ingestion_common.rs`, all three ingestion handlers

After steps 2a-2d and 3a-3b, the remaining handler-specific logic is:
- Which secrets to decrypt (token required vs optional, email, api_username)
- Which downstream handlers to trigger (metrics, identity resolution)

Define:

```rust
pub(super) struct IngestionSpec {
    pub handler_name: &'static str,
    pub token_key: Option<&'static str>,        // "api_token" or None
    pub token_required: bool,
    pub email_key: Option<&'static str>,         // "email" for Jira
    pub api_username_key: Option<&'static str>,  // "api_username" for Discourse
    pub item_noun: &'static str,                 // "repo", "project", "category"
}

pub(super) async fn execute_ingestion(
    ctx: &ObjectContext<'_>,
    state: &SharedState,
    spec: &IngestionSpec,
    override_watermark: Option<String>,
    tracker: &mut dyn ProgressTracker,
    trigger_downstream: impl FnOnce(&ObjectContext<'_>, i32),
) -> Result<(), TerminalError> {
    let source_type_key = ctx.key().to_string();
    let config = load_source_config(ctx, &state.repos, &source_type_key).await?;
    let source_name = config.name.clone();

    let method = if override_watermark.is_some() { "backfill" } else { "run_ingestion" };
    let run_id = create_run(ctx, &state.repos, &source_name, spec.handler_name, method).await?;

    // Decrypt secrets outside ctx.run()
    let token = match (spec.token_key, spec.token_required) {
        (Some(key), true) => Some(decrypt_required_secret(state, config.id, key).await?),
        (Some(key), false) => decrypt_optional_secret(state, config.id, key).await?,
        (None, _) => None,
    };
    let email = match spec.email_key {
        Some(key) => decrypt_optional_secret(state, config.id, key).await?,
        None => None,
    };
    let api_username = match spec.api_username_key {
        Some(key) => decrypt_optional_secret(state, config.id, key).await?,
        None => None,
    };

    let source = registry::create_source(&config.source_type)
        .ok_or_else(|| TerminalError::new(format!("unsupported source type: {}", config.source_type)))?;

    let ing_ctx = IngestionContext {
        repos: state.repos.clone(),
        source_config: config.clone(),
        http_client: state.http_client.clone(),
        token, email, api_username,
    };

    let mut plan = match source.plan(&ing_ctx).await {
        Ok(p) => p,
        Err(e) => {
            fail_run(ctx, &state.repos, run_id, &source_name, &e.to_string()).await;
            return Err(TerminalError::new(format!("plan failed: {e}")));
        }
    };

    if let Some(ref wm) = override_watermark {
        plan.watermark = Some(wm.clone());
    }

    info!(source = %source_name, watermark = ?plan.watermark, "ingestion plan ready");

    let initial_cursor = source.initial_cursor(&ing_ctx, &plan);

    let result = fetch_store_loop(
        ctx, &ing_ctx, run_id, &source_name, &initial_cursor, tracker,
    ).await;

    let (total_items, final_cursor) = match result {
        Ok(v) => v,
        Err(e) => {
            let msg = e.to_string();
            fail_run(ctx, &state.repos, run_id, &source_name, &msg).await;
            return Err(TerminalError::new(format!("ingestion failed: {msg}")));
        }
    };

    let failed_items = extract_failed_items(&final_cursor);
    finalise_run(
        ctx, state, &ing_ctx, run_id, &source_name, total_items,
        &failed_items, spec.item_noun, &final_cursor, source.watermark_field(),
    ).await?;

    if total_items > 0 {
        trigger_downstream(ctx, total_items);
    }

    info!(source = %source_name, total_items, "ingestion complete");
    Ok(())
}
```

Each handler becomes a thin wrapper:

```rust
// github_ingestion.rs
impl GithubIngestionHandler for GithubIngestionHandlerImpl {
    async fn run_ingestion(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let mut tracker = GithubProgressTracker::default();
        execute_ingestion(
            &ctx, &self.state,
            &IngestionSpec {
                handler_name: "GithubIngestionHandler",
                token_key: Some("api_token"),
                token_required: true,
                email_key: None,
                api_username_key: None,
                item_noun: "repo",
            },
            None,
            &mut tracker,
            |ctx, _| {
                ctx.service_client::<MetricsComputeHandlerClient>()
                    .compute_current_periods()
                    .send();
            },
        ).await
    }

    async fn backfill(&self, ctx: ObjectContext<'_>, since_date: String) -> Result<(), TerminalError> {
        let mut tracker = GithubProgressTracker::default();
        execute_ingestion(
            &ctx, &self.state,
            &IngestionSpec { /* same as above */ },
            Some(since_date),
            &mut tracker,
            |ctx, _| {
                ctx.service_client::<MetricsComputeHandlerClient>()
                    .compute_current_periods()
                    .send();
            },
        ).await
    }
}
```

**Journal safety:** The `ctx.run()` sequence is:
1. `load_config` (from `load_source_config`)
2. `create_run` (from `create_run`)
3. N × `store_batch` (from `fetch_store_loop` → `store_batch`)
4. `advance_watermark` and/or `complete_run`/`fail_run`/`complete_run_with_warnings` (from `finalise_run`)

This exactly matches the current sequence in each handler.

**Migration strategy:** Same as 3b — migrate one handler at a time, validate
after each.

**Commit:** `refactor: unified execute_ingestion function`

---

### Phase 4: Logging and observability overhaul

Implements the design from section 4. These are non-functional changes — no
`ctx.run()` modifications, no risk to in-flight invocations.

**Do this phase as a single PR** to avoid an intermediate state where some
handlers have the new logging and some have the old.

#### Step 4a: Add tracing spans and duration tracking to all handlers

**Files:** All 8 handler files, `ingestion_common.rs`

For the unified `execute_ingestion` (after Phase 3):

```rust
pub(super) async fn execute_ingestion(...) -> Result<(), TerminalError> {
    let start = Instant::now();

    // ... load config, create run ...

    let span = tracing::info_span!(
        "handler",
        handler = spec.handler_name,
        source = %source_name,
        run_id = %run_id,
    );
    async {
        info!("starting ingestion");

        // ... entire handler body ...

        info!(total_items, duration_secs = start.elapsed().as_secs(), "complete");
        Ok(())
    }.instrument(span).await
}
```

For non-ingestion handlers, the same pattern in each handler method:

```rust
async fn run_cycle(&self, ctx: Context<'_>) -> Result<(), TerminalError> {
    let start = Instant::now();
    let run_id = self.create_run(&ctx).await?;
    let span = tracing::info_span!("handler", handler = "EnrichmentHandler", run_id = %run_id);
    async {
        info!("starting enrichment cycle");
        // ... body ...
        info!(processed = total, errors = total, duration_secs = start.elapsed().as_secs(), "complete");
        Ok(())
    }.instrument(span).await
}
```

**Commit:** `feat: add tracing spans with run_id and duration to all handlers`

---

#### Step 4b: Demote per-batch/per-item logging to `debug!`

**Files:** All handler files, all source adapter files (see table in 4.8)

Bulk change: every `info!` call that logs per-batch, per-repo, per-page, or
per-item detail becomes `debug!`. This is the largest part of the logging
overhaul by line count (~40 call sites across handlers and source adapters).

Also change:
- `warn!("failed to update run progress")` → `debug!` (best-effort)
- `info!("triggering metrics recomputation")` → `debug!`
- `info!("no repos to ingest")` → `debug!`

Remove manual `source = ctx.source_config.name` annotations from source adapter
log calls — the handler span provides this automatically.

**Commit:** `refactor: demote per-batch and per-item logging to debug level`

---

#### Step 4c: Add periodic progress logging at `info` level

**File:** `ingestion_common.rs` (unified `fetch_store_loop`)

Add a 60-second interval `info!` line so long runs have a heartbeat:

```rust
let mut last_progress_log = Instant::now();
let mut batches = 0u32;

loop {
    let batch = fetch_batch(ing_ctx, &cursor).await?;
    // ... store ...
    batches += 1;

    if last_progress_log.elapsed() >= Duration::from_secs(60) {
        info!(total_items, batches, "progress");
        last_progress_log = Instant::now();
    }

    // ... rest of loop
}
```

For enrichment and identity resolution, same pattern in their inner loops.

**Commit:** `feat: add periodic progress logging at info level`

---

#### Step 4d: Add rate-limit warnings

**File:** `ingestion_common.rs` (unified `fetch_store_loop`)

After each `fetch_batch` call, check rate limit:

```rust
if let Some(ref rl) = batch.rate_limit {
    if rl.remaining < 100 {
        warn!(remaining = rl.remaining, limit = rl.limit, "rate limit pressure");
    }
}
```

No `source` field needed — the span provides it.

**Commit:** `feat: log rate-limit warnings during ingestion`

---

#### Step 4e: Standardise error/warn levels for run lifecycle failures

**Files:** `run_lifecycle.rs` (or wherever the shared functions land),
`enrichment.rs`, `model_catalogue.rs`

All `complete_run`/`fail_run` error paths use `error!`:
```rust
if let Err(e) = result {
    error!(error = %e, "failed to record run completion");
}
```

Change `enrichment.rs` and `model_catalogue.rs` from `warn!` to `error!` for
these specific cases.

Change `info!("daily budget exceeded")` in enrichment to `warn!` (this is
degraded behavior, not routine).

**Commit:** `fix: standardise log levels for run lifecycle and budget warnings`

---

#### Step 4f: Add progress reporting to identity resolution, metrics, team sync

**Files:** `identity_resolution.rs`, `metrics_compute.rs`,
`github_team_sync.rs`

Thread `run_id` through the relevant methods and add
`update_run_progress_detail` calls. Progress write failures use `debug!` (not
`warn!`).

**Identity resolution** — after each person in the resolve loop
**Metrics compute** — after each period type
**Team sync** — after each org

**Commit:** `feat: add progress reporting to identity resolution, metrics, and team sync`

---

### Phase 5: Resilience

#### Step 5a: Panic isolation for fetch/store

**File:** `ingestion_common.rs`

Wrap `source.fetch_batch()` and `source.store_batch()` calls in
`catch_unwind()`:

```rust
use std::panic::AssertUnwindSafe;
use futures::FutureExt;

let result = AssertUnwindSafe(src.fetch_batch(&ic, cursor))
    .catch_unwind()
    .await
    .map_err(|panic| {
        let msg = panic
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        TerminalError::new(format!("fetch panicked: {msg}"))
    })?
    .map_err(|e| TerminalError::new(format!("fetch failed: {e}")))?;
```

This converts panics into `TerminalError`, which the handler's error path will
then record via `fail_ingestion_run`.

**Note:** `catch_unwind` only catches unwinding panics, not `abort`. It also
requires `UnwindSafe` bounds. The `AssertUnwindSafe` wrapper is necessary
because `IngestionContext` contains `PgPool` and `reqwest::Client` which aren't
`UnwindSafe`. This is safe because we don't use the context after a panic — we
just want the error message.

**Commit:** `feat: isolate panics in fetch/store to prevent orphaned runs`

---

## 7. Commit Sequence and Deploy Strategy

### Commit order

```
Phase 1 (bugs):
  1. fix: fail ingestion run on fetch_store_loop error in GitHub/Jira handlers
  2. fix: pass full IngestionContext to fetch_batch/store_batch
  3. fix: model catalogue journaling, error logging, and error propagation
  4. refactor: make ctx.run() names unique for journal debuggability

Phase 2 (dedup):
  5. refactor: extract shared run lifecycle functions
  6. refactor: extract shared finalise_run and extract_failed_items
  7. refactor: move cursor building into Source::initial_cursor
  8. refactor: move watermark field name into Source trait

Phase 3 (unified loop):
  9. refactor: standardise secret decryption on ingestion_common
 10. refactor: unified fetch_store_loop with ProgressTracker trait
 11. refactor: unified execute_ingestion function

Phase 4 (logging overhaul — single PR):
 12. feat: add tracing spans with run_id and duration to all handlers
 13. refactor: demote per-batch and per-item logging to debug level
 14. feat: add periodic progress logging at info level
 15. feat: log rate-limit warnings during ingestion
 16. fix: standardise log levels for run lifecycle and budget warnings
 17. feat: add progress reporting to identity resolution, metrics, and team sync

Phase 5 (resilience):
 18. feat: isolate panics in fetch/store to prevent orphaned runs
```

### Journal-breaking changes

Only two commits alter the `ctx.run()` call sequence and may break in-flight
invocations:

| Commit | Handler affected | Risk | Mitigation |
| --- | --- | --- | --- |
| 3 (model catalogue) | `ModelCatalogueHandler` | Low | Short-lived, idempotent, retry is harmless |
| 9 (secret decryption) | `GithubTeamSyncHandler` | Low | Short-lived, idempotent, retry is harmless |

All other commits are pure code extractions that preserve the journal entry
sequence exactly.

### Deployment recommendation

Deploy in phases. Each phase can go out as a single PR:
- **Phase 1** can ship immediately — all bug fixes, high value
- **Phase 2** after Phase 1 is verified in production
- **Phase 3** after Phase 2 — highest risk, should soak before Phase 4
- **Phases 4-5** can ship independently at any time after Phase 2

---

## 8. Estimated Impact

| Metric | Before | After |
| --- | --- | --- |
| Total handler code | ~3,500 lines | ~2,200 lines (est.) |
| Duplicated run lifecycle wrappers | 5 copies | 1 |
| `execute_ingestion` copies | 3 | 1 |
| `fetch_store_loop` copies | 3 | 1 |
| Orphaned run bugs | 2 handlers | 0 |
| Handlers with progress reporting | 5/8 | 8/8 |
| Handlers with run_id in logs | 0/8 | 8/8 |
| Handlers with duration tracking | 0/8 | 8/8 |
