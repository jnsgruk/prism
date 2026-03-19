# Plan 44 — Unified Item-Level Iteration & Failure Isolation

## Problem Statement

When a single repo (GitHub), project (Jira), or category (Discourse) fails during ingestion, the entire run fails and the watermark does not advance. This has two consequences:

1. **Poison item blocking** — one persistently failing item (permissions revoked, deleted, renamed) blocks all other items in the source from making progress indefinitely.
2. **Redundant re-fetching** — on retry, items that already succeeded are re-fetched from the old watermark.

Additionally, the shared `fetch_batch()` function in `ingestion_common.rs` wraps external API calls inside `ctx.run()`, causing every fetch response to be journaled. For large backfills (e.g. Jira with 750k+ issues), this creates journals with tens of thousands of large entries, eventually OOM-killing Restate. This violates the project's own convention:

> **External API calls outside `ctx.run()`** — responses are large, and re-executing is safe (upserts, idempotent APIs). Never journal API responses.

GitHub already iterates per-repo via `repo_index` in its cursor, but Jira and Discourse use monolithic queries (all projects/categories in one request). This inconsistency makes it impossible to apply a uniform failure-isolation strategy.

## Goals

- **Move `fetch_batch()` outside `ctx.run()`** — stop journaling external API responses; only journal DB writes (store, watermark advance, run lifecycle)
- Unify all three ingestion handlers to iterate per-item (per-repo, per-project, per-category)
- Isolate failures so one bad item doesn't block the rest
- Track failed items in run metadata for admin visibility
- Add a `CompletedWithWarnings` status to distinguish partial-success runs
- Update CLAUDE.md so future ingestion handlers follow the same convention

## Non-Goals

- Per-item watermarks (too complex; source-level watermark is sufficient when items can be skipped)
- Retry queues or new database tables for failure tracking (run metadata is sufficient)
- Changes to the enrichment handler (already has per-item error handling — though it should adopt `CompletedWithWarnings` in a follow-up)

## Watermark Behaviour on Partial Failure

All three sources use a **single global timestamp watermark** (`max_updated_at` / `max_bumped_at`). The watermark advances at the end of a run to the max timestamp seen across all fetched items.

**Problem:** If the watermark advances when some items fail, items in the failed project/category that were updated between the old and new watermark timestamps are **permanently missed**. Example: PROJ1 has items up to Mar 18, PROJ2 fails, PROJ3 has items up to Mar 20. Watermark advances to Mar 20. Next run fetches PROJ2 from Mar 20 — PROJ2 items updated between old watermark and Mar 20 are lost.

**Rule: Do NOT advance the watermark on `CompletedWithWarnings` runs.** The run completes (not `Failed`), items from successful projects are stored via upsert, admin sees which items failed and why, and the next run re-fetches from the old watermark. The redundant re-fetching of already-stored items is harmless because stores are idempotent upserts.

This means failed items are automatically re-attempted on the next scheduled run — no retry queue needed. The only cost is re-fetching items that were already stored, which is a minor overhead compared to data loss.

---

## Phase 0 — Audit Existing `status = 'completed'` Queries

Before adding `CompletedWithWarnings`, audit all code that filters on completion status to determine which queries need updating.

### Needs updating

| Location | Purpose | Fix |
|----------|---------|-----|
| `crates/ps-core/src/repo/activity/status.rs:39` | `get_source_statuses()` lateral join finds last successful run | Change `status = 'completed'` → `status IN ('completed', 'completed_with_warnings')` |
| `frontend/lib/run-status.ts` | `StatusFilter` type and `statusConfig` object | Add `completed_with_warnings` entry |
| `frontend/views/admin/components/handler-runs-table.tsx` | Filter buttons for run status | Add filter option for partial-success runs (or group with "Completed") |

### No change needed

| Location | Reason |
|----------|--------|
| `crates/ps-server/src/services/handlers/restate.rs:145` | Queries Restate's `sys_invocation` table — different status domain entirely |
| `crates/ps-core/src/repo/activity/watermarks.rs` | `upsert_watermark()` doesn't check status — called by handler logic |
| `crates/ps-server/src/services/handlers/mod.rs:112` | `derive_state()` uses `last_successful_run` which comes from the status query above — correct once that query is fixed |
| Proto definitions | `IngestionStatus` is Rust-only (not in any `.proto` file), so no proto changes needed |

---

## Phase 0.5 — Move `fetch_batch()` Outside `ctx.run()`

### Problem

The shared `fetch_batch()` in `crates/ps-workers/src/handlers/ingestion_common.rs` (line 243) wraps the external HTTP call inside `ctx.run("fetch_batch")`. This journals every API response — for a Jira backfill with 750k+ issues at 50/batch, that's ~15,000 journal entries with large JSON payloads. This caused Restate to OOM and crash-loop during a real backfill on 2026-03-18.

The project convention (CLAUDE.md) is clear: external API calls go **outside** `ctx.run()` because responses are large and re-executing is safe (stores are idempotent upserts). Only DB writes (`store_batch`, `advance_watermark`, `complete_run`) should be journaled.

### Step 0.5a: Refactor `fetch_batch()` to execute outside `ctx.run()`

**File: `crates/ps-workers/src/handlers/ingestion_common.rs`**

Change `fetch_batch()` from a journaled side-effect to a plain async function. The fetch is safe to re-execute on replay because:
- Stores use `ON CONFLICT ... DO UPDATE` (idempotent upserts)
- Watermark only advances after all items are stored
- External APIs are read-only from our perspective

```rust
/// Fetch a batch — NOT journaled (external API call, large response, idempotent on replay).
pub(super) async fn fetch_batch(
    state: &SharedState,
    config: &SourceConfig,
    cursor: &str,
    token: Option<&str>,
) -> Result<SerFetchResult, TerminalError> {
    let src = registry::create_source(&config.source_type)
        .ok_or_else(|| TerminalError::new("source unavailable"))?;
    let ic = IngestionContext {
        repos: state.repos.clone(),
        source_config: config.clone(),
        http_client: state.http_client.clone(),
        token: token.map(String::from),
        email: None,
        api_username: None,
    };
    let result = src
        .fetch_batch(&ic, cursor)
        .await
        .map_err(|e| TerminalError::new(format!("fetch failed: {e}")))?;

    Ok(SerFetchResult {
        items: result.items,
        next_cursor: result.next_cursor,
        rate_limit: result.rate_limit,
        etag: result.etag,
    })
}
```

Note: the `ctx: &ObjectContext<'_>` parameter is removed entirely — the function no longer needs Restate context.

### Step 0.5b: Update all handler call sites

All three handlers call `fetch_batch()` identically in their fetch-store loops. Update each to drop the `ctx` argument:

**Files:**
- `crates/ps-workers/src/handlers/github_ingestion.rs` (~line 166)
- `crates/ps-workers/src/handlers/jira_ingestion.rs` (~line 161)
- `crates/ps-workers/src/handlers/discourse_ingestion.rs` (~line 185)

```rust
// Before:
let batch = fetch_batch(ctx, &self.state, config, &cursor, token).await?;

// After:
let batch = fetch_batch(&self.state, config, &cursor, token).await?;
```

### Step 0.5c: Verify `store_batch()` remains inside `ctx.run()`

`store_batch()` (line 294) **must** stay inside `ctx.run("store_batch")` — it performs DB writes that should be idempotent on replay (they already use upserts) and should not be re-executed unnecessarily. No change needed here; this step is a verification checkpoint.

Similarly, `advance_watermark()`, `complete_ingestion_run()`, `fail_ingestion_run()`, and `create_ingestion_run()` all correctly use `ctx.run()` for their DB writes. No changes needed.

### Impact

This single change eliminates ~50% of journal entries (every `fetch_batch` call) and all of the largest payloads (serialized API responses). For the Jira backfill that OOM'd Restate:
- **Before:** ~30,000 journal entries (15,000 fetch + 15,000 store) with large fetch payloads
- **After:** ~15,000 journal entries (store only) with small DB-result payloads

This is safe because on Restate replay, the fetch will re-execute (hitting the external API again), get the same or updated data, and the store upsert will handle it correctly.

### Replay behaviour

On replay after a crash, Restate will:
1. Skip all completed `ctx.run()` entries (stores, watermark advances) using journaled results
2. Re-execute any un-journaled fetch calls (they'll hit the external API again)
3. The re-fetched data feeds into the next `store_batch` `ctx.run()`, which is either already journaled (skip) or needs to execute (idempotent upsert)

This matches GitHub's GraphQL pattern and the enrichment handler's AI API call pattern — both already execute external calls outside `ctx.run()`.

---

## Phase 1 — Unify Iteration Pattern

### Step 1.1: Add `CompletedWithWarnings` ingestion status

A run that completes with some items skipped due to errors should not be marked `Completed` (misleading) or `Failed` (the run did useful work). Add a new status variant.

**File: `crates/ps-core/src/models/enums.rs`**

Add variant to `IngestionStatus`:

```rust
pub enum IngestionStatus {
    Running,
    Completed,
    CompletedWithWarnings,  // NEW
    Failed,
    Cancelled,
}
```

Update `as_str()`, `FromStr`, and `Display`:

```rust
impl IngestionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::CompletedWithWarnings => "completed_with_warnings",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl FromStr for IngestionStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "completed_with_warnings" => Ok(Self::CompletedWithWarnings),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("invalid IngestionStatus: {s}")),
        }
    }
}
```

**File: `crates/ps-core/src/repo/activity/runs.rs`**

Add `complete_run_with_warnings()`:

```rust
pub async fn complete_run_with_warnings(
    &self,
    id: Uuid,
    items_collected: i32,
    error_message: &str,
) -> Result<(), Error> {
    sqlx::query!(
        r#"UPDATE activity.ingestion_runs
           SET completed_at = now(),
               status = 'completed_with_warnings',
               items_collected = $2,
               error_message = $3
           WHERE id = $1"#,
        id,
        items_collected,
        error_message,
    )
    .execute(&*self.pool)
    .await?;
    Ok(())
}
```

**File: `crates/ps-workers/src/handlers/ingestion_common.rs`**

Add `complete_ingestion_run_with_warnings()` following the same `ctx.run()` pattern as `complete_ingestion_run()`:

```rust
pub(super) async fn complete_ingestion_run_with_warnings(
    ctx: &ObjectContext<'_>,
    state: &SharedState,
    source_name: &str,
    run_id: Uuid,
    total_items: i32,
    error_summary: &str,
) -> Result<(), TerminalError> {
    let repos = state.repos.clone();
    let name = source_name.to_string();
    let id = run_id;
    let items = total_items;
    let err_msg = error_summary.to_string();
    ctx.run(|| {
        let repos = repos.clone();
        let name = name.clone();
        let err_msg = err_msg.clone();
        async move {
            repos.activity.complete_run_with_warnings(id, items, &err_msg).await
                .map_err(|e| TerminalError::new(format!("complete_with_warnings failed: {e}")))?;
            repos.activity.clear_current_invocation_id(&name).await
                .map_err(|e| TerminalError::new(format!("clear invocation failed: {e}")))?;
            Ok(Json::from(()))
        }
    })
    .name("complete_run_with_warnings")
    .await?;
    Ok(())
}
```

**Frontend changes:**

Update the Run History badge mapping in the ingestion views to handle the new status:

**File: `frontend/views/ingestion/`** (wherever run status badges are rendered)

```typescript
// Add to status → badge variant mapping
case "completed_with_warnings":
  return { variant: "outline", label: "Partial", icon: AlertTriangle };
```

---

### Step 1.2: Add `IngestionPlan.items` field

The current `IngestionPlan.repos` field is GitHub-specific (`Vec<RepoTarget>`). We need a generic items list. Rather than breaking the existing field (which GitHub uses extensively), add a new `items` field for the generic iteration target.

**File: `crates/ps-core/src/ingestion.rs`**

```rust
pub struct IngestionPlan {
    pub source_name: String,
    pub watermark: Option<String>,
    pub repos: Vec<RepoTarget>,       // GitHub-specific (kept for backward compat)
    pub items: Vec<String>,           // NEW: generic iteration targets (project keys, category IDs)
}
```

This avoids changing GitHub's plan at all — it continues to use `repos`. Jira sets `items` to project keys (`["PROJ1", "PROJ2"]`), Discourse sets `items` to category ID strings (`["5", "12", "30"]`).

---

### Step 1.3: Refactor Jira to per-project iteration

The Jira cursor currently uses `projects: Vec<String>` and builds one big `project IN (...)` JQL query. Refactor to iterate one project at a time via `project_index`.

#### 1.3a: Update the Cursor

**File: `crates/ps-workers/src/jira/source/mod.rs`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Cursor {
    pub(crate) watermark: Option<String>,
    pub(crate) projects: Vec<String>,
    #[serde(default)]
    pub(crate) project_index: usize,           // NEW: which project we're fetching
    pub(crate) next_page_token: Option<String>,
    pub(crate) max_updated_at: Option<String>,
    pub(crate) base_url: String,
    pub(crate) story_points_field: Option<String>,
    pub(crate) api_mode: String,
    #[serde(default)]
    pub(crate) failed_items: Vec<FailedItem>,  // NEW: items that errored
}
```

Add the `FailedItem` struct (shared across all sources — define in `ps-core`):

**File: `crates/ps-core/src/ingestion.rs`**

```rust
/// An item (repo, project, category) that failed during an ingestion run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedItem {
    /// Human-readable identifier (e.g. "canonical/lxd", "PROJ-1", "category:5")
    pub key: String,
    /// Error message from the failed fetch
    pub error: String,
}
```

#### 1.3b: Update initial_cursor to set project_index = 0

**File: `crates/ps-workers/src/jira/source/mod.rs`** — `initial_cursor()`:

```rust
fn initial_cursor(&self, plan: &IngestionPlan) -> String {
    // ... existing field extraction from plan ...
    let cursor = Cursor {
        watermark: plan.watermark.clone(),
        projects: plan.items.clone(),   // Use plan.items instead of extracting from settings
        project_index: 0,               // NEW
        next_page_token: None,
        max_updated_at: plan.watermark.clone(),
        base_url,
        story_points_field,
        api_mode,
        failed_items: vec![],           // NEW
    };
    serde_json::to_string(&cursor).unwrap_or_default()
}
```

#### 1.3c: Refactor fetch to iterate per-project

**File: `crates/ps-workers/src/jira/source/fetch.rs`**

Replace the monolithic JQL construction with per-project iteration:

```rust
pub(super) async fn fetch_batch_impl(
    ctx: &IngestionContext,
    cursor: &str,
) -> Result<FetchResult, ps_core::Error> {
    let mut cur: Cursor = serde_json::from_str(cursor)
        .map_err(|e| ps_core::Error::Internal(format!("invalid cursor: {e}")))?;

    // Check if all projects are exhausted
    let current_project = if cur.projects.is_empty() {
        // No project filter — fetch everything (existing behavior for unconfigured sources)
        None
    } else {
        let Some(proj) = cur.projects.get(cur.project_index) else {
            // All projects exhausted — done
            let final_cursor = serialise_cursor(&cur)?;
            return Ok(FetchResult {
                items: vec![],
                next_cursor: None,
                rate_limit: None,
                etag: Some(final_cursor),
            });
        };
        Some(proj.clone())
    };

    // Build single-project JQL
    let project_filter = current_project
        .as_ref()
        .map(|p| format!("project = \"{}\"", p.replace('"', "\\\"")))
        .unwrap_or_default();

    let jql = match (project_filter.is_empty(), &cur.watermark) {
        (false, Some(wm)) => {
            let jira_date = format_watermark_for_jql(wm);
            format!("{project_filter} AND updated >= \"{jira_date}\" ORDER BY updated ASC")
        }
        (false, None) => format!("{project_filter} ORDER BY updated ASC"),
        (true, Some(wm)) => {
            let jira_date = format_watermark_for_jql(wm);
            format!("updated >= \"{jira_date}\" ORDER BY updated ASC")
        }
        (true, None) => "ORDER BY updated ASC".to_string(),
    };

    // ... existing client construction and search call ...

    let (response, rate_limit) = client
        .search(&jql, MAX_RESULTS_PER_PAGE, fields, expand,
                cur.next_page_token.as_deref())
        .await?;

    // ... existing item conversion and max_updated_at tracking ...

    // Determine next cursor
    let has_more = response
        .is_last
        .map_or(response.next_page_token.is_some(), |last| !last);

    let final_cursor = serialise_cursor(&cur)?;

    let next_cursor = if has_more {
        // More pages for current project
        cur.next_page_token = response.next_page_token;
        Some(serialise_cursor(&cur)?)
    } else if !cur.projects.is_empty() && cur.project_index + 1 < cur.projects.len() {
        // Move to next project
        cur.project_index += 1;
        cur.next_page_token = None;
        Some(serialise_cursor(&cur)?)
    } else {
        // All projects exhausted
        None
    };

    Ok(FetchResult {
        items,
        next_cursor,
        rate_limit,
        etag: Some(final_cursor),
    })
}
```

#### 1.3d: Update plan to populate `items`

**File: `crates/ps-workers/src/jira/source/plan.rs`**

```rust
pub(super) async fn plan_impl(ctx: &IngestionContext) -> Result<IngestionPlan, ps_core::Error> {
    let projects: Vec<String> = ctx.source_config.settings
        .get("projects")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // ... existing watermark logic ...

    Ok(IngestionPlan {
        source_name: ctx.source_config.name.clone(),
        watermark: effective_watermark,
        repos: vec![],
        items: projects,  // NEW: populate generic items
    })
}
```

#### 1.3e: Remove `build_jira_cursor()` from handler

**File: `crates/ps-workers/src/handlers/jira_ingestion.rs`**

The handler currently builds the cursor in `build_jira_cursor()`. After this change, cursor construction moves into `Source::initial_cursor()` (which reads from the plan). Remove `build_jira_cursor()` and replace:

```rust
// Before:
let initial_cursor = build_jira_cursor(config, &plan);

// After:
let source = JiraSource;
let initial_cursor = source.initial_cursor(&plan);
```

#### 1.3f: Update progress reporting

**File: `crates/ps-workers/src/handlers/jira_ingestion.rs`** — `build_progress_json()`

Add project iteration tracking to match GitHub's repo tracking:

```rust
fn build_progress_json(cursor_json: &str, tickets_fetched: u32, rate_limit: ...) -> serde_json::Value {
    let cur: serde_json::Value = serde_json::from_str(cursor_json).unwrap_or_default();
    let projects_total = cur.get("projects")
        .and_then(|v| v.as_array())
        .map_or(0, Vec::len);
    let project_index = cur.get("project_index")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let current_project = cur.get("projects")
        .and_then(|v| v.as_array())
        .and_then(|ps| ps.get(project_index as usize))
        .and_then(serde_json::Value::as_str);
    let failed_count = cur.get("failed_items")
        .and_then(|v| v.as_array())
        .map_or(0, Vec::len);

    serde_json::json!({
        "phase": current_project.map_or("complete".to_string(),
            |p| format!("project:{p}")),
        "tickets_fetched": tickets_fetched,
        "projects_total": projects_total,
        "projects_completed": project_index,
        "current_project": current_project,
        "failed_items": failed_count,
        // ... rate limit fields ...
    })
}
```

---

### Step 1.4: Refactor Discourse to per-category iteration

The Discourse handler currently fetches `/latest.json` (all categories) and filters client-side. Refactor to iterate per-category using the Discourse `/c/{slug}/{id}/l/latest.json` endpoint.

#### 1.4a: Add per-category `latest` method to the client

**File: `crates/ps-workers/src/discourse/client.rs`**

```rust
/// Fetch the latest topics page for a specific category.
///
/// Uses the `/c/{category_id}/l/latest.json` endpoint (Discourse 2.7+).
/// The slug-less form avoids needing to slugify category names. `page` is 0-indexed.
pub async fn latest_for_category(
    &self,
    category_id: i64,
    page: u32,
) -> Result<LatestResponse, ps_core::Error> {
    let url = format!("{}/c/{}/l/latest.json", self.base_url, category_id);

    debug!(category_id, page, "discourse category latest request");

    let req = self
        .http
        .get(&url)
        .query(&[("page", page.to_string()), ("order", "activity".into())])
        .timeout(std::time::Duration::from_secs(30));

    let resp = self.auth(req).send().await.map_err(|e| {
        ps_core::Error::Internal(format!("discourse category latest request failed: {e}"))
    })?;

    Self::handle_rate_limit(&resp)?;
    Self::require_success(&resp)?;

    resp.json()
        .await
        .map_err(|e| ps_core::Error::Internal(format!("discourse category latest parse: {e}")))
}
```

#### 1.4b: Update the Cursor

**File: `crates/ps-workers/src/discourse/source/mod.rs`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Cursor {
    pub(crate) watermark: Option<String>,
    pub(crate) category_ids: Vec<i64>,
    #[serde(default)]
    pub(crate) category_index: usize,                               // NEW
    pub(crate) page: u32,
    pub(crate) min_posts: i32,
    pub(crate) base_url: String,
    pub(crate) instance: String,
    pub(crate) max_bumped_at: Option<String>,
    pub(crate) has_more: bool,
    pub(crate) category_map: std::collections::HashMap<i64, String>,
    #[serde(default)]
    pub(crate) failed_items: Vec<ps_core::ingestion::FailedItem>,   // NEW
}
```

Update `initial_cursor()`:

```rust
fn initial_cursor(&self, plan: &IngestionPlan) -> String {
    // ... existing field extraction ...
    let category_ids: Vec<i64> = plan.items.iter()
        .filter_map(|s| s.parse::<i64>().ok())
        .collect();

    let cursor = Cursor {
        watermark: plan.watermark.clone(),
        category_ids,
        category_index: 0,           // NEW
        page: 0,
        min_posts,
        base_url,
        instance,
        max_bumped_at: plan.watermark.clone(),
        has_more: true,
        category_map: HashMap::new(),
        failed_items: vec![],        // NEW
    };
    serde_json::to_string(&cursor).unwrap_or_default()
}
```

#### 1.4c: Refactor fetch to iterate per-category

**File: `crates/ps-workers/src/discourse/source/fetch.rs`**

Key change: when `category_ids` is non-empty, fetch per-category instead of global `/latest.json`.

```rust
pub(super) async fn fetch_batch_impl(
    ctx: &IngestionContext,
    cursor: &str,
) -> Result<FetchResult, ps_core::Error> {
    let mut cur: Cursor = serde_json::from_str(cursor)
        .map_err(|e| ps_core::Error::Internal(format!("invalid cursor: {e}")))?;

    // ... existing client construction, category_map loading ...

    // Determine which API to call
    let response = if cur.category_ids.is_empty() {
        // No category filter — fetch global latest (existing behavior)
        client.latest(cur.page).await
    } else {
        // Per-category iteration
        let Some(&cat_id) = cur.category_ids.get(cur.category_index) else {
            let final_cursor = serialise_cursor(&cur)?;
            return Ok(FetchResult {
                items: vec![],
                next_cursor: None,
                rate_limit: None,
                etag: Some(final_cursor),
            });
        };
        client.latest_for_category(cat_id, cur.page).await
    };

    // Handle rate limit gracefully (existing pattern)
    let response = match response {
        Ok(r) => r,
        Err(ps_core::Error::RateLimit { retry_after_secs }) => {
            // ... existing rate limit handling ...
        }
        Err(e) => return Err(e),
    };

    // ... existing topic processing, detail fetching, like fetching ...

    // Determine next cursor
    let stop = reached_watermark || !has_more_pages || cur.page >= MAX_PAGES_PER_RUN;

    let final_cursor = serialise_cursor(&cur)?;

    let next_cursor = if stop {
        if !cur.category_ids.is_empty() && cur.category_index + 1 < cur.category_ids.len() {
            // Move to next category
            cur.category_index += 1;
            cur.page = 0;
            Some(serialise_cursor(&cur)?)
        } else {
            // All categories exhausted (or no category filter)
            None
        }
    } else {
        cur.page += 1;
        cur.has_more = has_more_pages;
        Some(serialise_cursor(&cur)?)
    };

    Ok(FetchResult {
        items,
        next_cursor,
        rate_limit: None,
        etag: Some(final_cursor),
    })
}
```

**Important:** With per-category fetching, the `filter_topics()` function no longer needs the category filter check (the API returns only topics from that category). The `min_posts` and watermark filters remain. Simplify `filter_topics()` to remove the category filter branch when iterating per-category:

```rust
// In filter_topics(), the category filter block becomes:
if !cur.category_ids.is_empty() && cur.category_index < cur.category_ids.len() {
    // Per-category fetch — server already filtered by category, no client-side check needed.
    // The min_posts and watermark filters still apply.
}
// When category_ids is empty, no category filtering at all (global fetch, existing behaviour).
```

#### 1.4d: Update plan to populate items

Currently the Discourse `plan.rs` doesn't extract categories — that happens in `build_discourse_cursor()` in the handler (reading `config.settings["categories"]`). Move the extraction into `plan_impl()`:

**File: `crates/ps-workers/src/discourse/source/plan.rs`**

```rust
pub(super) async fn plan_impl(ctx: &IngestionContext) -> Result<IngestionPlan, ps_core::Error> {
    // ... existing watermark logic ...

    let categories: Vec<i64> = ctx.source_config.settings
        .get("categories")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    Ok(IngestionPlan {
        source_name: ctx.source_config.name.clone(),
        watermark: effective_watermark,
        repos: vec![],
        items: categories.iter().map(|id| id.to_string()).collect(),  // NEW
    })
}
```

#### 1.4e: Remove `build_discourse_cursor()` from handler

Same pattern as Jira — move cursor construction into `Source::initial_cursor()`.

---

### Step 1.5: Add `failed_items` to GitHub cursor

GitHub already iterates per-repo. Add the `failed_items` field to its cursor for consistency.

**File: `crates/ps-workers/src/github/source/mod.rs`**

```rust
pub(super) struct Cursor {
    // ... existing fields ...
    pub(super) last_rate_limit_remaining: Option<i32>,
    #[serde(default)]
    pub(super) failed_items: Vec<ps_core::ingestion::FailedItem>,  // NEW
}
```

---

### Step 1.6: Add per-item error handling to fetch loops

This is the key change. Each handler's `fetch_store_loop()` currently propagates errors from `fetch_batch()` immediately via `?`. Instead, catch per-item fetch errors, record the failure, and advance to the next item.

The cleanest way to do this is in the `fetch_batch()` **source implementation** (not the handler), because that's where the item-level iteration happens.

#### GitHub: `crates/ps-workers/src/github/source/fetch.rs`

Wrap the per-repo GraphQL call:

```rust
pub(super) async fn fetch_team_repos(
    ctx: &IngestionContext,
    cur: &mut Cursor,
) -> Result<FetchResult, ps_core::Error> {
    let Some(repo_target) = cur.repos.get(cur.repo_index) else {
        return transition_to_member_search(ctx, cur).await;
    };

    let (owner, repo) = (&repo_target.owner, &repo_target.repo);

    // ... existing query construction ...

    let page = match client
        .search_pull_requests(&query, cur.graphql_cursor.as_deref())
        .await
    {
        Ok(page) => page,
        Err(e) => {
            // Record failure, skip to next repo
            warn!(
                source = ctx.source_config.name,
                repo = %format!("{owner}/{repo}"),
                error = %e,
                "skipping repo due to fetch error"
            );
            cur.failed_items.push(FailedItem {
                key: format!("{owner}/{repo}"),
                error: e.to_string(),
            });
            cur.repo_index += 1;
            cur.graphql_cursor = None;
            return Ok(FetchResult {
                items: vec![],
                next_cursor: Some(serialise_cursor(cur)?),
                rate_limit: None,
                etag: None,
            });
        }
    };

    // ... rest of existing logic unchanged ...
}
```

#### Jira: `crates/ps-workers/src/jira/source/fetch.rs`

Same pattern — wrap the search call:

```rust
let (response, rate_limit) = match client
    .search(&jql, MAX_RESULTS_PER_PAGE, fields, expand,
            cur.next_page_token.as_deref())
    .await
{
    Ok(result) => result,
    Err(e) => {
        if let Some(ref proj) = current_project {
            warn!(
                source = ctx.source_config.name,
                project = proj,
                error = %e,
                "skipping project due to fetch error"
            );
            cur.failed_items.push(FailedItem {
                key: proj.clone(),
                error: e.to_string(),
            });
            // Advance to next project
            cur.project_index += 1;
            cur.next_page_token = None;
            let next_cursor = if cur.project_index < cur.projects.len() {
                Some(serialise_cursor(&cur)?)
            } else {
                None
            };
            // etag carries cursor state back to the handler (not an HTTP ETag)
            let final_cursor = serialise_cursor(&cur)?;
            return Ok(FetchResult {
                items: vec![],
                next_cursor,
                rate_limit: None,
                etag: Some(final_cursor),
            });
        }
        return Err(e); // No project filter → can't skip
    }
};
```

#### Discourse: `crates/ps-workers/src/discourse/source/fetch.rs`

Same pattern — wrap the per-category fetch call and skip on error.

---

### Step 1.7: Use `CompletedWithWarnings` in handlers

Each handler's `execute_ingestion()` checks for failures after the fetch-store loop and chooses the appropriate completion status.

**Pattern (same for all three handlers):**

```rust
// After fetch_store_loop returns (total_items, final_cursor):

// Extract failed_items from final cursor
let failed_items: Vec<FailedItem> = serde_json::from_str::<serde_json::Value>(&final_cursor)
    .ok()
    .and_then(|v| v.get("failed_items").cloned())
    .and_then(|v| serde_json::from_value(v).ok())
    .unwrap_or_default();

if failed_items.is_empty() {
    // Fully successful — advance watermark and complete
    if total_items > 0 {
        advance_watermark(ctx, &self.state, config, &final_cursor, total_items, ...).await?;
    }
    complete_ingestion_run(ctx, &self.state, source_name, run_id, total_items).await?;
} else if total_items == 0 {
    // Every item failed — nothing was ingested, treat as a full failure.
    let summary = format!(
        "all {} item(s) failed: {}",
        failed_items.len(),
        failed_items.iter().map(|f| f.key.as_str()).collect::<Vec<_>>().join(", ")
    );
    fail_ingestion_run(ctx, &self.state, source_name, run_id, &summary).await?;
} else {
    // Partial failure — do NOT advance watermark (see "Watermark Behaviour on Partial Failure").
    // Items from successful projects are already stored via upsert. The next run will
    // re-fetch from the old watermark, re-storing them idempotently, and retry the
    // failed items.
    let summary = format!(
        "{} item(s) failed: {}",
        failed_items.len(),
        failed_items.iter().map(|f| f.key.as_str()).collect::<Vec<_>>().join(", ")
    );
    complete_ingestion_run_with_warnings(
        ctx, &self.state, source_name, run_id, total_items, &summary,
    ).await?;
}
```

---

## Phase 2 — Admin Visibility

### Step 2.1: Store failed items in run metadata

When completing a run with warnings, store the `failed_items` vec in the run's `metadata` JSONB column. Note: `ingestion_runs` has both `metadata` (final run results/context) and `progress` (live progress updated during the run). Failed items belong in `metadata` — they're a final result, not transient progress.

**File: `crates/ps-core/src/repo/activity/runs.rs`**

Update `complete_run_with_warnings()` to accept metadata:

```rust
pub async fn complete_run_with_warnings(
    &self,
    id: Uuid,
    items_collected: i32,
    error_message: &str,
    metadata: serde_json::Value,
) -> Result<(), Error> {
    sqlx::query!(
        r#"UPDATE activity.ingestion_runs
           SET completed_at = now(),
               status = 'completed_with_warnings',
               items_collected = $2,
               error_message = $3,
               metadata = $4
           WHERE id = $1"#,
        id,
        items_collected,
        error_message,
        metadata,
    )
    .execute(&*self.pool)
    .await?;
    Ok(())
}
```

Metadata JSON shape:

```json
{
  "failed_items": [
    { "key": "canonical/lxd", "error": "404 Not Found" },
    { "key": "canonical/juju", "error": "403 Forbidden" }
  ]
}
```

### Step 2.2: Frontend — Distinguish partial-success runs

**File: `frontend/lib/run-status.ts`** — this is the single source of truth for status display config.

Add `completed_with_warnings` to the `StatusFilter` type and `statusConfig`:

```typescript
export type StatusFilter = "all" | "completed" | "completed_with_warnings" | "failed" | "cancelled" | "running";

// Add to statusConfig:
completed_with_warnings: {
  label: "Partial",
  variant: "outline",
  icon: createElement(AlertTriangle, { className: "size-3" }),
},
```

**File: `frontend/views/admin/components/handler-runs-table.tsx`** — add filter button or group `completed_with_warnings` with "Completed" filter.

### Step 2.3: Frontend — Show failed items in run detail

When a run has `status = "completed_with_warnings"`, show an expandable section listing the failed items and their errors.

```tsx
{run.status === "completed_with_warnings" && run.metadata?.failed_items && (
  <Alert variant="destructive" className="mt-4">
    <AlertTriangle className="size-4" />
    <AlertTitle>
      {run.metadata.failed_items.length} item(s) skipped due to errors
    </AlertTitle>
    <AlertDescription>
      <ul className="mt-2 space-y-1 text-sm">
        {run.metadata.failed_items.map((item: { key: string; error: string }) => (
          <li key={item.key}>
            <span className="font-mono">{item.key}</span>
            <span className="text-muted-foreground"> — {item.error}</span>
          </li>
        ))}
      </ul>
    </AlertDescription>
  </Alert>
)}
```

### Step 2.4: Recurring failure detection (optional, low priority)

A lightweight query to surface items that fail across multiple consecutive runs:

```sql
-- Items that failed in the last N runs for a source
SELECT
    f.value->>'key' AS item_key,
    COUNT(*) AS failure_count,
    MAX(r.started_at) AS last_failure
FROM activity.ingestion_runs r,
     jsonb_array_elements(r.metadata->'failed_items') AS f
WHERE r.source_name = $1
  AND r.status = 'completed_with_warnings'
  AND r.started_at > now() - INTERVAL '7 days'
GROUP BY f.value->>'key'
HAVING COUNT(*) >= 3
ORDER BY failure_count DESC;
```

This could be exposed as a repo method and shown as a "Persistent Issues" section on the source detail page. Not blocking for Phase 1.

### Step 2.5: Enrichment handler follow-up (separate PR)

The enrichment handler already has per-item error handling but currently marks runs `Failed` only when `total_errors > 0 && total_processed == 0`, and `Completed` otherwise. It should adopt `CompletedWithWarnings` for partial-success runs (some items enriched, some failed). This is a small change once the status and repo methods from Phase 1 exist, but is out of scope for this plan to keep the diff focused on ingestion.

---

## Files Changed Summary

### Phase 0 — Status Query Audit

| File | Change |
|------|--------|
| `crates/ps-core/src/repo/activity/status.rs` | Update `get_source_statuses()` to include `completed_with_warnings` |

### Phase 0.5 — Move Fetches Outside `ctx.run()`

| File | Change |
|------|--------|
| `crates/ps-workers/src/handlers/ingestion_common.rs` | Remove `ctx.run()` wrapper from `fetch_batch()`; remove `ctx` parameter |
| `crates/ps-workers/src/handlers/github_ingestion.rs` | Update `fetch_batch()` call site (drop `ctx` arg) |
| `crates/ps-workers/src/handlers/jira_ingestion.rs` | Update `fetch_batch()` call site (drop `ctx` arg) |
| `crates/ps-workers/src/handlers/discourse_ingestion.rs` | Update `fetch_batch()` call site (drop `ctx` arg) |

### Phase 1 — Core Changes

| File | Change |
|------|--------|
| `crates/ps-core/src/models/enums.rs` | Add `CompletedWithWarnings` to `IngestionStatus` |
| `crates/ps-core/src/ingestion.rs` | Add `FailedItem` struct; add `items: Vec<String>` to `IngestionPlan` |
| `crates/ps-core/src/repo/activity/runs.rs` | Add `complete_run_with_warnings()` |
| `crates/ps-workers/src/handlers/ingestion_common.rs` | Add `complete_ingestion_run_with_warnings()` |
| `crates/ps-workers/src/jira/source/mod.rs` | Add `project_index`, `failed_items` to Cursor; update `initial_cursor()` |
| `crates/ps-workers/src/jira/source/fetch.rs` | Rewrite JQL to single-project; add per-project iteration and error handling |
| `crates/ps-workers/src/jira/source/plan.rs` | Populate `plan.items` with project keys |
| `crates/ps-workers/src/handlers/jira_ingestion.rs` | Remove `build_jira_cursor()`; use `source.initial_cursor()`; add warnings handling |
| `crates/ps-workers/src/discourse/client.rs` | Add `latest_for_category()` method |
| `crates/ps-workers/src/discourse/source/mod.rs` | Add `category_index`, `failed_items` to Cursor; update `initial_cursor()` |
| `crates/ps-workers/src/discourse/source/fetch.rs` | Add per-category iteration; simplify `filter_topics()`; add error handling |
| `crates/ps-workers/src/discourse/source/plan.rs` | Populate `plan.items` with category ID strings |
| `crates/ps-workers/src/handlers/discourse_ingestion.rs` | Remove `build_discourse_cursor()`; add warnings handling |
| `crates/ps-workers/src/github/source/mod.rs` | Add `failed_items` to Cursor |
| `crates/ps-workers/src/github/source/fetch.rs` | Wrap per-repo fetch in error handling |
| `crates/ps-workers/src/handlers/github_ingestion.rs` | Add warnings handling |

### Phase 2 — Frontend Changes

| File | Change |
|------|--------|
| `frontend/lib/run-status.ts` | Add `completed_with_warnings` to `StatusFilter` and `statusConfig` |
| `frontend/views/admin/components/handler-runs-table.tsx` | Add or group filter button for partial-success status |
| `frontend/views/ingestion/` | Failed items detail section in run view |

### sqlx Cache

After all query changes, run `cargo sqlx prepare --workspace` and commit `.sqlx/` separately.

---

## Migration

No database migration needed. The `status` column is `TEXT` — the new `completed_with_warnings` value is handled by the Rust enum's `FromStr`/`Display` implementation. The `metadata` JSONB column already exists on `ingestion_runs`.

---

## Testing Strategy

1. **Unit tests** for `FailedItem` serialization/deserialization round-trip
2. **Integration tests** per source:
   - Configure wiremock to return 403 for one repo/project/category and 200 for others
   - Verify the run completes with `CompletedWithWarnings` status
   - Verify the watermark does NOT advance (stays at old value)
   - Verify `failed_items` appears in run metadata
3. **Watermark regression**: verify that a fully successful run advances the watermark as before
4. **Regression test**: verify that a run with zero failures still completes as `Completed` (not `CompletedWithWarnings`)
5. **All items fail**: configure wiremock to return errors for every repo/project/category — verify the run is marked `Failed` (not `CompletedWithWarnings`) with 0 items collected
6. **Empty items list**: verify that a source with no configured projects/categories falls back to unfiltered fetch (existing behaviour preserved)
7. **Frontend**: verify badge rendering for all five status values

---

## CLAUDE.md Update

After implementation, add the following to the **Ingestion** section of `CLAUDE.md`:

```markdown
### Per-Item Iteration Convention

All ingestion handlers MUST iterate per-item (per-repo, per-project, per-category) using an index field in the cursor (e.g., `repo_index`, `project_index`, `category_index`). The plan populates `IngestionPlan.items` with the list of targets; the cursor caches this list and tracks progress through it.

**Error isolation:** Per-item fetch calls must be wrapped in error handling. On failure, the handler logs a warning, records a `FailedItem` in the cursor's `failed_items` vec, advances the index, and continues to the next item. The run completes as `CompletedWithWarnings` if any items were skipped.

**Watermark rule:** Do NOT advance the watermark on `CompletedWithWarnings` runs. The global timestamp watermark would skip over failed items' data windows. Instead, keep the old watermark so the next run re-fetches everything (idempotent via upserts) and retries the failed items.

**Do not** build monolithic queries that fetch all items at once (e.g., `project IN (...)` JQL, unfiltered `/latest.json`). Each item must be fetchable independently so failures can be isolated.
```

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Discourse `/c/{id}/l/latest.json` returns different topic ordering than `/latest.json` | Low | Medium | Both use `order=activity`; verify in integration test |
| Per-project JQL is slower than `IN(...)` for many projects | Low | Low | Jira paginates identically; slightly more HTTP requests but same total data |
| Existing cursor JSON in Restate journal lacks new fields | Medium | Low | `#[serde(default)]` on all new cursor fields (applied in Steps 1.3a, 1.4b, 1.5) |
| `CompletedWithWarnings` confuses existing queries that check `status = 'completed'` | Medium | Medium | **Resolved in Phase 0** — audited all queries; `get_source_statuses()` updated |
| Advancing watermark on partial failure causes data loss for failed items | — | High | **Resolved** — watermark only advances on fully successful runs (Step 1.7) |
| Sources with no configured items (empty projects/categories list) break per-item iteration | Low | Medium | Fetch code falls back to unfiltered query when items list is empty (Steps 1.3c, 1.4c) |

### Backward Compatibility

New cursor fields (`project_index`, `category_index`, `failed_items`) must use `#[serde(default)]` so that in-flight Restate invocations with old cursor JSON can deserialize without error:

```rust
#[serde(default)]
pub(crate) project_index: usize,       // defaults to 0
#[serde(default)]
pub(crate) failed_items: Vec<FailedItem>,  // defaults to []
```

This is critical — Restate may replay a journaled cursor from before the deployment. The `default` attribute ensures it parses as "start from item 0 with no failures", which is correct behavior (worst case: one redundant re-fetch of already-stored items).

---

## Commit Sequence

1. **`fix: move fetch_batch outside ctx.run to stop journaling API responses`** — Phase 0.5 only. Can ship independently; fixes the Restate OOM.
2. **`feat: add CompletedWithWarnings ingestion status`** — Phase 0 audit + Phase 1.1 (enum, repo method, status query fix, frontend status config).
3. **`refactor: per-project iteration for Jira ingestion`** — Steps 1.2 (IngestionPlan.items + FailedItem), 1.3 (Jira cursor, fetch, plan, handler).
4. **`refactor: per-category iteration for Discourse ingestion`** — Steps 1.4 (Discourse client, cursor, fetch, plan, handler).
5. **`feat: failure isolation with per-item error handling`** — Steps 1.5 (GitHub failed_items), 1.6 (error handling all sources), 1.7 (CompletedWithWarnings in handlers).
6. **`feat: show failed items in run history UI`** — Phase 2 frontend changes.
7. **`chore: update sqlx query cache`** — Separate commit per project convention.
