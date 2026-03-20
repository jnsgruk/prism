# Claude Code Guidelines for Prism

## Project Overview

Prism is an engineering insights platform for understanding team and individual performance across multiple platforms (GitHub, Jira, Discourse, Launchpad, Google Drive, mailing lists). Built in Rust (backend) + Vite/React (frontend) with PostgreSQL, gRPC (tonic + Connect), and Restate for ingestion orchestration.

## Build & Test Commands

```bash
prek run -av                              # All lints, tests, formatters — run before finishing any task
cargo build                               # Build all crates
cargo test                                # Run all Rust tests
cargo clippy --allow-dirty --fix          # Lint + auto-fix
nix fmt                                   # Format all files (treefmt: rustfmt, nixfmt, deadnix, oxfmt, shfmt)
buf lint                                  # Lint proto files
buf generate                              # Generate Rust + TypeScript code from protos
cargo sqlx prepare --workspace            # Update offline query cache (.sqlx/)
sqlx migrate add <name>                   # Create new migration in migrations/
bun install                               # Install frontend dependencies (run from frontend/)
bun dev                                   # Start frontend dev server (run from frontend/)
bun test                                  # Run frontend tests via vitest (run from frontend/)
```

## Workflow Requirements

**Before finishing any task**, always:

1. Run `prek run -av`
2. Ensure **zero warnings** from `cargo clippy` and `nix fmt` — lints must be 100% clean before committing
3. Consider if the test coverage needs updating
4. Update the **Implementation Progress** checklist in `README.md` if the task completes (or partially completes) a workstream
5. Provide a **draft commit message** using Conventional Commits format

**Commit rules:**

- Use `--no-gpg-sign` when committing autonomously
- Always commit in logical chunks along the way. Don't wait to be prompted.
- **`.sqlx/` changes go in a separate commit** with message `chore: update sqlx query cache` — never mix query cache updates with code changes.

## Code Structure

Code is organised **feature-first, layer-second**. See `plans/18-code-structure.md` for the full strategy, invariants, and worked examples.

### Key rules

- **Frontend:** feature UI lives in `views/<feature>/` with `components/`, `hooks/`, `pages/` subdirs. Routes are defined in `app.tsx` with lazy imports. Shared components stay in `components/`. Shared hooks stay in `lib/hooks/`. The signal to lift is a concrete second consumer.
- **Rust services:** new features go in `src/features/<name>/` with handler, service, repository, types files. `ps-core` remains the shared domain layer (models, repo, auth, crypto).
- **Three-tier escalation:** feature-local → service/app-local → shared crate/package. Only lift when a concrete second consumer exists.
- **No `utils/` or `helpers/` directories.** Give utilities a proper home.
- **Tests colocated** with source files. No `__tests__/` directories. Rust uses inline `#[cfg(test)]`.

### Frontend structure

```
frontend/
├── app.tsx           # Router — lazy imports from views/, route definitions
├── main.tsx          # React root — BrowserRouter, Providers, render
├── index.html        # SPA entry point
├── globals.css       # Tailwind + shadcn theme variables
├── views/            # Feature modules
│   ├── admin/        #   components/, hooks/, lib/, pages/
│   ├── dashboard/    #   pages/
│   ├── ingestion/    #   components/, hooks/, pages/
│   ├── teams/        #   components/, hooks/, pages/
│   ├── login/        #   pages/
│   └── setup/        #   pages/
├── components/       # Service-level: app-shell, page-header, data-table/, ui/ (shadcn)
└── lib/              # Service plumbing: api/, hooks/ (shared), session, providers
```

### Crate structure

```
crates/
├── ps-core/          # Domain types, traits, error types, shared logic
│   └── src/
│       ├── repo/     # Repository layer — ALL database access lives here
│       ├── models/   # Domain models: config, contribution, enums, ingestion, person, team
│       ├── auth/     # Password hashing, token generation, session management
│       ├── crypto.rs # AES-256-GCM encryption for source credentials
│       └── backup.rs # Export/import logic
├── ps-proto/         # Generated Rust code from proto definitions (pedantic lints disabled)
├── ps-server/        # API server binary (tonic + tonic-web), services, auth interceptor
├── ps-workers/       # Restate worker binary — ingestion, team sync, metrics compute handlers
├── ps-metrics/       # Metric computation logic (DORA, flow, etc.)
├── ps-migrate/       # Migration binary for k8s init container
└── psctl/            # Lightweight CLI client (depends only on ps-proto)
```

**Dependency flow:** `psctl → ps-proto` | `ps-server → ps-core, ps-proto, ps-metrics` | `ps-workers → ps-core, ps-proto, ps-metrics` | `ps-metrics → ps-core`

### Repository Layer (`ps-core/src/repo/`)

All database access is centralized in the repository layer. Each repo maps to one database schema (bounded context):

| Repo           | Schema     | Responsibility                                         |
| -------------- | ---------- | ------------------------------------------------------ |
| `AuthRepo`     | `auth`     | Users, sessions, API tokens                            |
| `ConfigRepo`   | `config`   | Source configs, encrypted secrets                      |
| `OrgRepo`      | `org`      | People, teams, platform identities, repositories       |
| `ActivityRepo` | `activity` | Contributions, watermarks, ingestion runs, ETag cache  |
| `MetricsRepo`  | `metrics`  | Pre-computed team/individual snapshots, contribution queries |

The `Repos` struct bundles all repos and is constructed once from a `PgPool`, then cloned into each service and the ingestion handler.

**Layering rules:**

1. **All `sqlx::query!` calls must live in `ps-core/src/repo/`** — services, ingestion sources, and other crates must never contain direct SQL. They access data exclusively through repo methods.
2. **Services are thin gRPC adapters** — they receive `Repos`, delegate to repo methods, and map between domain types and proto types. Business logic that doesn't need proto types belongs in `ps-core`.
3. **One repo per schema** — each repo owns queries for its schema. Cross-schema joins are permitted only as read-only queries within the repo that is the primary consumer of the result (e.g., `ActivityRepo::get_source_statuses` joins `config` + `activity`).
4. **No `PgPool` in services or sources** — services and ingestion sources receive `Repos`, never a raw pool. Only `main.rs` (server/ingestion binaries) and the repo layer itself should touch `PgPool`.

### Database Schemas (Bounded Contexts)

| Schema      | Purpose                                                            |
| ----------- | ------------------------------------------------------------------ |
| `config`    | Source configs, encrypted secrets, global settings                 |
| `org`       | People, teams, platform identities, team memberships, repositories |
| `activity`  | Contributions, ingestion watermarks, ingestion runs, ETag cache    |
| `metrics`   | Pre-computed team/individual snapshots                             |
| `auth`      | Users, sessions                                                    |
| `reasoning` | AI enrichments, embeddings, insights (Phase 3+)                    |

### Frontend

Vite + React Router SPA + React + shadcn/ui (built on `@base-ui/react` primitives) + TypeScript (strict mode, type-checked with typescript-go). Bun as runtime/package manager. Connect clients generated from proto definitions. React Query for server state. Recharts for charts. Production container serves static files via Caddy.

**No horizontal overflow.** All page content must stay within the viewport width — no horizontal scrollbars on the page. Use `min-w-0` on flex children, `overflow-hidden` on content wrappers, and `overflow-x-auto` on wide elements like tables so they scroll internally rather than pushing the page wider. The `SidebarInset` component already applies `min-w-0 overflow-hidden`; individual pages must ensure their content respects this constraint.

**shadcn/ui is the standard UI component library.** Always use components from `@/components/ui/` (Dialog, Button, Card, Input, Label, Select, Tabs, Badge, Table, Alert, Separator, DropdownMenu) rather than hand-rolling UI with raw Tailwind. The underlying primitives come from `@base-ui/react`, not Radix. To add new shadcn components: `bunx shadcn@latest add <component-name>`. Components use `@ps/cn` for the `cn` helper.

## Frontend State & Validation

### State Management — React Query + Local State

**React Query** is the only state management library. It handles all server data via custom hooks (`useAuth`, `useConfig`, `useOrg`) with hierarchical query keys. Do not add nanostores, Redux, Jotai, or other global state libraries.

**When to use what:**

| State type | Tool | Example |
| --- | --- | --- |
| Server data (queries, mutations) | React Query | Auth status, source configs, team lists |
| Component-local UI | `useState` | Dialog open/close, form inputs, drag state |
| Shared UI state within a subtree | React Context | Sidebar collapse (already exists) |
| Persisted client preference | Cookie / `localStorage` | Sidebar state (cookie), session token (localStorage) |

If a future feature genuinely needs **cross-component client state** that isn't server data (e.g., complex multi-step wizard state, global notification queue, coordinated filter state across unrelated components), prefer **Zustand** — it's lightweight, React-idiomatic, and avoids the prop-drilling that Context solves poorly at scale. Do not reach for nanostores (framework-agnostic overhead we don't need in a Vite SPA).

### Zod — Validate at System Boundaries

Zod is installed. Use it for **runtime validation at system boundaries** — places where data enters the app from outside TypeScript's compile-time guarantees:

- **Form validation** — define Zod schemas for non-trivial forms (multi-field, cross-field rules, format constraints). Pair with shadcn/ui `<Form>` + `react-hook-form` when forms outgrow simple `required` attributes.
- **File uploads** — validate structure/format of imported files (JSON shape, CSV headers) before processing.
- **localStorage / cookies** — validate shape when reading persisted data that could be stale or corrupted.

**Do not use Zod for:**

- **Proto responses** — Connect + `@bufbuild/protobuf` already handles serialization. Adding Zod on top is redundant.
- **Simple required-field checks** — HTML5 `required` attribute is sufficient for basic presence checks.
- **Internal function arguments** — TypeScript types are enough within the app boundary.

## Frontend UI Conventions

### Tables — DataTable Component

All tables use the shared `DataTable` component (`components/data-table/`) built on TanStack React Table v8. Always use it rather than building tables from raw `<Table>` primitives.

- **Manual sorting** (`manualSorting: true`) — sorting is server-driven via gRPC `sort_field`/`sort_ascending` parameters
- **Pagination** via `DataTablePagination` — shows "1–10 of 47" (en-dash), page size selector (10/25/50/100), chevron navigation
- **Empty state** — "No results." centered across full table width
- **Overflow** — wrap in `<div className="overflow-x-auto rounded-md border">` so wide tables scroll internally
- **Sortable column headers** — use `ArrowUpDown` icon button; active sort shows directional arrow
- Filters reset page index to 0 when changed

### Date & Time Formatting

- **24-hour clock only** — never use 12-hour format or AM/PM
- **Short format** for timestamps: `toLocaleDateString(undefined, { month: "short", day: "numeric" })` + `toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false })` → "Mar 16 14:30"
- **Relative time** for recent events: "5m ago", "2h ago", "1d ago" — fall back to full date for older items
- **ISO 8601** (`YYYY-MM-DD`) for period selectors and API values

### Number Formatting

- **Whole numbers**: `String(n)` or `.toLocaleString()` for large values (comma separators)
- **Decimals with unit**: `.toFixed(1)` + suffix — e.g., `"2.5h"`, `"1.2d"`
- **Percentages**: `Math.round(percent)` + `%`
- **Tabular alignment**: `className="tabular-nums"` on numeric columns
- **No data**: em-dash `"—"` (not "N/A" or "0")

### Icons — Lucide React

Lucide React is the only icon library. Sizing conventions:

| Context | Class | Size |
| --- | --- | --- |
| Buttons, table cells | `size-4` | 16px |
| Small badges, inline | `size-3` or `size-3.5` | 12–14px |
| Section headings | `size-6` | 24px |
| Empty state illustrations | `size-10` | 40px |
| Spinner | `Loader2` + `animate-spin` | context-dependent |

Secondary icons use `text-muted-foreground`, primary use `text-foreground`.

### Empty States

Centered layout with dashed border for empty lists/pages:

```
<div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
  <Icon className="size-10 text-muted-foreground" />
  <p className="mb-1 font-medium">Title</p>
  <p className="text-sm text-muted-foreground">Description</p>
</div>
```

### Loading States

- **Full-page**: centered `<Loader2 className="size-6 animate-spin text-muted-foreground" />`
- **Skeletons**: `<Skeleton className="h-10 w-full" />` for list items
- **Buttons**: inline `<Loader2 className="mr-1.5 size-3.5 animate-spin" />` with "Saving..." text, button disabled

### Toasts — Sonner

All user-facing success/error feedback uses Sonner: `toast.success("Created")`, `toast.error("Failed")`. Fire in mutation `onSuccess`/`onError` callbacks. Extract error messages with `err instanceof Error ? err.message : "Default"`.

### Badge Conventions

Status badges map to shadcn variants — no custom Tailwind colors:

| State | Variant |
| --- | --- |
| Active, merged, approved | `default` |
| Counts, secondary info | `secondary` |
| Error, closed, inactive | `destructive` |
| Neutral, open | `outline` |

State text is `text-[10px] uppercase`. Include icon with `className="gap-1"` when badge has an icon.

### Search & Filter Patterns

- **Search input**: `<Input>` with `Search` icon (size-3.5) absolutely positioned left, `pl-8` padding. Debounced 300ms via `useRef` + `setTimeout`.
- **Filter toggles**: `Button` group — `variant="default"` for active, `variant="outline"` for inactive. Grouped in `flex items-center gap-1`.
- **Select dropdowns** for categorical filters (type, state).

### Dialogs & Forms

- **Dialog structure**: `DialogHeader` (title + description) → form body → `DialogFooter` (Cancel + primary action)
- **Multi-step dialogs**: step state in parent, `ArrowLeft` back button in header, separate header per step
- **Scrollable content**: `max-h-[60vh] overflow-y-auto` for long form bodies
- **Field layout**: `space-y-4` between fields, `space-y-2` between label and input
- **Validation**: HTML5 `required` for simple presence; Zod + react-hook-form for complex rules
- **Submit buttons**: `type="submit"`, `disabled={isPending}`, loading text "Saving..."/"Creating..."
- **Errors**: `Alert variant="destructive"` above the footer

### Page Layout

- **PageHeader**: fixed `h-14` bar with `SidebarTrigger | Separator | Title + Description | Actions`
- **Content area**: `<div className="min-w-0 flex-1 space-y-6 overflow-y-auto p-6">`
- **Metric grids**: `grid grid-cols-2 lg:grid-cols-4 gap-4` for stat cards
- **Section spacing**: `space-y-6` between top-level sections, `space-y-3` or `space-y-4` within cards

### Collapsible Sections

Use shadcn `Collapsible` for expandable content within cards. Card header is the clickable trigger with `ChevronDown`/`ChevronRight` icon. Show count badge next to the section title: `<Badge variant="secondary" className="ml-1">{count}</Badge>`.

### Links & Navigation

- **Internal**: React Router `<Link>` component, wrapped in Button via `render={<Link to="/path" />}`
- **External**: `<a href={url} target="_blank" rel="noopener noreferrer">` with `ExternalLink` icon (size-3)
- **URL state**: `useSearchParams()` for filter/pagination state that should survive navigation
- **Back**: `ArrowLeft` icon button with `onClick={() => history.back()}`

### Charts — Recharts

- **Responsive**: `<ResponsiveContainer width="100%" height={300}>`
- **Grid**: `<CartesianGrid strokeDasharray="3 3" className="stroke-border" />`
- **Axes**: `tick={{ fontSize: 12 }} className="fill-muted-foreground"`
- **Bars**: `radius={[4, 4, 0, 0]}` for rounded tops
- **Colors**: use HSL CSS variables — `hsl(var(--primary))`, `hsl(var(--popover))` for tooltip background

## Key Conventions

### sqlx — Type-Safe Queries Only

**Always** use `sqlx::query!`, `sqlx::query_as!`, `sqlx::query_scalar!`. **Never** use the runtime `sqlx::query()` string-based function. Schema changes must be caught at compile time.

The app binary **never runs migrations** — migrations are handled by the `ps-migrate` k8s init container.

### Configuration in DB, Not Files

Source credentials (API tokens) are stored encrypted in `config.secrets` using AES-256-GCM (`aes-gcm` crate). Only `PS_SECRET_KEY` (256-bit, base64-encoded) comes from environment. All other configuration is managed through the admin UI via gRPC.

### Traceability

Every metric, insight, or AI-generated output **must** be auditable back to source data. Static metrics link to contributing data points. AI enrichments store model, input, and confidence. The UI must always provide a "show how this was calculated" affordance.

### Proto & Code Generation

Proto files live in `proto/prism/v1/`. After changes:

1. `buf lint`
2. `buf generate` (produces Rust types in `crates/ps-proto/src/gen/`, TypeScript clients in `frontend/lib/api/gen/`)
3. Rebuild both backend and frontend

### Domain Enums — Strong Typing with TEXT Storage

Domain concepts (platform, contribution type, state, ingestion status, period type, role) use Rust enums stored as `TEXT` in PostgreSQL. The `impl_sqlx_text!` macro bridges sqlx encode/decode. No Postgres custom type migrations needed — the Rust compiler enforces valid values.

- Use domain enums (`Platform`, `ContributionType`, `ContributionState`, `IngestionStatus`, `PeriodType`, `Role`) everywhere — never string literals like `"github"` or `"merged"`
- Implement `FromStr` / `Display` via the macro, use `.parse::<Platform>()` idiomatically
- Enums live in `ps-core/src/models/enums.rs`

### Security Conventions

- **Fail-closed auth** — missing auth header must return an error, never silently forward. Non-public RPCs without auth are rejected by the interceptor.
- **Admin role enforcement** — privileged operations (reset, backup, token management) must call `require_admin()`.
- **Error masking** — log full database errors server-side with `tracing`, return generic "internal error" to gRPC clients. Never expose DB error details.
- **LIKE pattern escaping** — always escape `%` and `_` in user-supplied search terms before passing to SQL `LIKE`/`ILIKE`.
- **Input validation** — validate external identifiers (Restate SQL identifiers: `^[a-zA-Z0-9_-]+$`, GitHub usernames, URLs) before interpolation.
- **Secret material** — never decrypt secrets inside Restate `ctx.run()` (the journal persists side-effect results, defeating at-rest encryption). Decrypt outside, pass through context. Use `Zeroizing` wrapper for key material.

### Performance Conventions

- **Batch writes with `UNNEST`** — for bulk upserts (people, contributions, identities, teams), use `UNNEST` arrays in a single query instead of per-row INSERT loops.
- **`tokio::try_join!`** — use for independent async operations (e.g., count + data queries, parallel team computations).
- **`futures::stream::buffer_unordered(N)`** — for capped concurrent work over collections.
- **File size limit** — split files exceeding ~500 lines into modules. God-files hurt readability and review.
- **Params structs** — when a function takes >5 parameters, bundle into a struct instead of suppressing `clippy::too_many_arguments`.

### Restate Handler Architecture

All long-running background work **must** run as Restate handlers — never as synchronous gRPC RPCs. This ensures durability, cancellation, progress tracking, and journal visibility.

#### Handler Types

| Handler | Restate Type | Key | Purpose |
| --- | --- | --- | --- |
| `GithubIngestionHandler` | Object | source name | GitHub PR/review ingestion |
| `JiraIngestionHandler` | Object | source name | Jira issue ingestion |
| `DiscourseIngestionHandler` | Object | source name | Discourse topic ingestion |
| `GithubTeamSyncHandler` | Object | source name | GitHub team/member/repo sync |
| `MetricsComputeHandler` | Service | — | Metric snapshot computation |
| `EnrichmentHandler` | Service | — | AI enrichment pipeline |
| `IdentityResolutionHandler` | Service | — | Discourse identity resolution |
| `ModelCatalogueHandler` | Service | — | AI model catalogue refresh |

**Objects** are per-source (keyed by source type name, e.g. `"github"`). **Services** are singletons.

#### SharedState

All handlers receive `SharedState` (constructed once in `main.rs`, cloned into each handler):

```rust
pub struct SharedState {
    pub repos: Repos,                      // Database access (all repos)
    pub secret_key: Zeroizing<[u8; 32]>,  // AES-256-GCM encryption key
    pub http_client: reqwest::Client,     // Shared HTTP client (60s timeout)
}
```

Handlers never touch `PgPool` directly — always go through `state.repos`.

#### Journaling Rules

| What | Inside `ctx.run()`? | Why |
| --- | --- | --- |
| DB writes (store, watermark, run lifecycle) | Yes | Must be idempotent on replay |
| External API calls (GitHub, Jira, AI) | **No** | Responses are large; re-executing is safe (upserts) |
| Secret decryption | **No** | Journal persists results — plaintext must never be inside |
| Progress updates | **No** | Best-effort, doesn't affect replay correctness |
| Budget/rate-limit reads | **No** | Re-checking on replay is correct |

All `ctx.run()` closures must have `.name("step_name")` labels for journal debugging.

#### Run Lifecycle

Managed by macros in `handlers/run_lifecycle.rs`:

- **`create_run!`** — inside `ctx.run()`, generates `Uuid::now_v7()` inside the closure so retries reuse the journaled ID (no duplicate runs)
- **`complete_run!`** — inside `ctx.run()`, marks complete + clears `current_invocation_id`
- **`complete_run_with_warnings!`** — partial failure: records failed items in metadata
- **`fail_run!`** — inside `ctx.run()`, marks failed + clears `current_invocation_id`

All log errors rather than propagating — run lifecycle failure should not abort the handler.

#### Frontend Dispatch

- Use `TriggerHandler` RPC (fire-and-forget to Restate), never synchronous RPCs for long operations.
- `trigger_handler()` guards against duplicate runs (checks for active runs before dispatching).
- UI shows Run/Cancel toggle with polling for status updates.

#### Journal Compatibility

Changing the sequence of `ctx.run()` calls in a handler **breaks in-flight invocations**. Restate replays the journal positionally — if the code now calls different steps at the same indices, you get error 570 ("mismatch between code paths"). After refactoring handler code:

1. Cancel all in-flight invocations for affected handlers
2. If the CLI can't find them: wipe Restate's journal storage (`/restate-data/`) and restart the pod
3. Re-register the deployment: `restate deployments register http://ps-workers:9081/ --force --yes`

### Ingestion Handler Pattern

The three ingestion handlers (GitHub, Jira, Discourse) share unified orchestration via `execute_ingestion()` in `handlers/ingestion_common.rs`. Platform-specific logic is abstracted behind the `Source` trait.

#### Source Trait (`ps-core/src/ingestion.rs`)

```rust
pub trait Source: Send + Sync {
    fn name(&self) -> &'static str;
    async fn plan(&self, ctx: &IngestionContext) -> Result<IngestionPlan, Error>;
    async fn fetch_batch(&self, ctx: &IngestionContext, cursor: &str) -> Result<FetchResult, Error>;
    async fn store_batch(&self, ctx: &IngestionContext, items: &[ContributionInput]) -> Result<usize, Error>;
    async fn advance_watermark(&self, ctx: &IngestionContext, new_watermark: &str, items: i32) -> Result<(), Error>;
    fn initial_cursor(&self, ctx: &IngestionContext, plan: &IngestionPlan) -> String;
    fn watermark_field(&self) -> &'static str { "max_updated_at" }  // Discourse overrides to "max_bumped_at"
}
```

Sources are registered in `registry.rs` and instantiated by `create_source(platform)`.

#### IngestionContext

Pre-constructed once per run with **all secrets pre-decrypted** (outside `ctx.run()`):

```rust
pub struct IngestionContext {
    pub repos: Repos,
    pub source_config: SourceConfig,
    pub http_client: reqwest::Client,
    pub token: Option<String>,        // Pre-decrypted API token
    pub email: Option<String>,        // Pre-decrypted email (Jira)
    pub api_username: Option<String>, // Pre-decrypted username (Discourse)
}
```

#### execute_ingestion() Flow

1. **Create source adapter** — `registry::create_source(source_type)`
2. **Create run record** (journaled) — `Uuid::now_v7()` inside `ctx.run()` for idempotent retries
3. **Decrypt secrets** (outside `ctx.run()`) — via `decrypt_required_secret()` / `decrypt_optional_secret()`
4. **Build IngestionContext** — combine state + config + decrypted secrets
5. **Plan** (not journaled) — determine repos/projects/categories to fetch, load watermark
6. **Override watermark** if backfilling — replace plan's watermark with user-provided date
7. **fetch_store_loop()** — batched fetch→store→advance cycle (see below)
8. **Finalise run** — three outcomes based on failed items
9. **Trigger downstream** — fire-and-forget to MetricsComputeHandler, etc.

#### IngestionSpec

Each handler defines a static spec describing its secrets and error nouns:

```rust
const GITHUB_SPEC: IngestionSpec = IngestionSpec {
    handler_name: "GithubIngestionHandler",
    token_key: Some("api_token"),
    token_required: true,
    email_key: None,
    api_username_key: None,
    item_noun: "repo",  // For error summaries like "2 repo(s) failed"
};
```

#### fetch_store_loop() — Core Batch Loop

```
loop {
    1. fetch_batch()          — NOT journaled (external API, idempotent on replay)
       └─ wrapped in catch_unwind() to isolate panics
    2. Update cursor from etag (Jira/Discourse pattern)
    3. Rate limit warning if remaining < 100
    4. store_batch()          — journaled inside ctx.run()
    5. advance_watermark()    — journaled inside ctx.run() (incremental, after each batch)
    6. Update progress        — NOT journaled (best-effort)
    7. Break if next_cursor is None
}
```

**Incremental watermark advancement**: After each successful `store_batch()`, the watermark is advanced immediately. On retry, only the last incomplete batch needs re-fetching — not the entire run.

#### Transient Error Retry

All external API calls inside `fetch_batch()` must be wrapped with `retry_transient()` from `crates/ps-workers/src/retry.rs`. This retries up to 3 times with exponential backoff (1s, 2s, 4s) for transient errors (5xx, timeouts, connection resets), while non-transient errors short-circuit immediately.

- **Error classification** — HTTP clients (Jira, Discourse) must use `ps_core::Error::HttpStatus { status, message }` instead of `Error::Internal` for non-2xx responses, so `is_transient()` can inspect the status code. GitHub uses its own error types with their own `is_transient()` methods.
- **Call-site visibility** — retry wrapping is explicit at each fetch call site, not hidden inside the client. The `is_transient` classifier is passed as a function pointer.
- **Journal safety** — all retry sites are inside `fetch_batch()` which runs outside `ctx.run()`. Never introduce `ctx.run()` inside a retry loop.
- **Rate limits are not transient** — 429 responses are handled separately via `Error::RateLimit` and durable sleep, not via retry.

#### ProgressTracker Trait

Each handler implements source-specific progress reporting:

```rust
pub trait ProgressTracker {
    fn count_batch(&mut self, items: &[ContributionInput], stored: i32);
    fn build_progress(&self, cursor: &str, rate_limit: Option<&RateLimitInfo>) -> serde_json::Value;
    fn build_final_progress(&self) -> serde_json::Value;
}
```

Progress is stored in the run's `progress` JSONB column and displayed in the UI. Not journaled.

#### Cursor Design

Each source defines its own cursor struct (serialized to JSON). Cursors are **opaque to the orchestration layer** — only `initial_cursor()` and `fetch_batch()` interpret them.

- **GitHub**: Multi-phase (`TeamRepos` → `MemberSearch`), tracks `repo_index`, `graphql_cursor`, `max_updated_at`, `failed_items`
- **Jira**: Iterates projects, tracks `project_index`, `next_page_token`, `max_updated_at`, `failed_items`
- **Discourse**: Iterates categories, tracks `category_index`, `page`, `max_bumped_at`

Use `#[serde(default)]` on cursor fields for forward compatibility when evolving structure.

#### Watermark & Finalisation

Three outcomes in `finalise_run()`:

| Outcome | Watermark | Run status |
| --- | --- | --- |
| No failures, items > 0 | Advanced (final) | `completed` |
| All items failed (total = 0) | **Not** advanced | `failed` |
| Partial failure (some repos/projects failed) | **Not** advanced | `completed_with_warnings` |

Failed items are tracked via `Vec<FailedItem>` in the cursor and extracted at finalisation.

#### Downstream Triggers

After successful ingestion, handlers fire-and-forget to downstream handlers:

```rust
|ctx| {
    ctx.service_client::<MetricsComputeHandlerClient>()
        .compute_current_periods()
        .send();
}
```

Discourse also triggers `IdentityResolutionHandler`. Triggers are **not awaited**.

### GitHub Two-Phase Ingestion

1. **Team repos phase** — fetch PRs/reviews for repos discovered via team sync data. Uses GraphQL for PRs + reviews inline.
2. **Member search phase** — discover cross-repo contributions by team members via GraphQL search API. Falls back to full org discovery when no teams are configured.

**GraphQL over REST** for N+1-prone queries (PRs + reviews inline, member search). REST for infrequent operations like team sync.

**Scheduling:** Recurring ingestion uses Restate's durable delayed self-invocation (`ctx.object_client().method().send_with_delay()`), not external cron daemons. Cron expressions stored per-source, evaluated in UTC.

### Adding a New Ingestion Handler

1. **Source module** — `crates/ps-workers/src/newplatform/source/{mod.rs, plan.rs, fetch.rs, store.rs}`. Implement `Source` trait. Define cursor struct.
2. **Registry** — add `Platform::NewPlatform => Some(Box::new(NewPlatformSource))` in `registry.rs`
3. **Handler** — `crates/ps-workers/src/handlers/newplatform_ingestion.rs`. Define `IngestionSpec`, implement `ProgressTracker`, create `#[restate_sdk::object]` trait with `run_ingestion()` and `backfill()`. Call `execute_ingestion()`.
4. **Export** — add `pub mod newplatform_ingestion` in `handlers/mod.rs`
5. **Wire up** — instantiate in `main.rs`, bind to Restate endpoint
6. **Platform enum** — add variant to `Platform` in `ps-core/src/models/enums.rs` if new platform type

### Adding a New System Handler

1. **Handler** — `crates/ps-workers/src/handlers/newhandler.rs`. Use `#[restate_sdk::service]` (singleton) or `#[restate_sdk::object]` (per-key). Follow journaling rules.
2. **Export** — add `pub mod newhandler` in `handlers/mod.rs`
3. **Wire up** — instantiate in `main.rs`, bind to Restate endpoint

## Testing Strategy

### Rust — Integration Tests Are Primary

Test against real PostgreSQL (sqlx test fixtures), never mock the database. External APIs (GitHub, Jira) mocked with `wiremock`.

```
tests/
├── integration/
│   ├── main.rs            # Test binary entry point
│   ├── common/            # Shared fixtures, helpers, macros
│   ├── api/               # gRPC API tests
│   ├── ingestion/         # Source adapter tests
│   ├── metrics/           # Metrics computation tests
│   └── domain/            # Cross-cutting domain logic tests
```

Key macros: `define_api_test!`, `define_source_test!`, `define_metric_test!`

### Frontend — Lightweight, Custom Logic Only

Vitest + React Testing Library + happy-dom. API mocking via `createRouterTransport` (Connect, in-memory, type-safe). Fresh `QueryClient` per test with `retry: false`.

Test custom hooks, data transformations, interactive components. Don't test shadcn/ui primitives, chart SVG output, React Router config, or CSS.

## Gotchas

1. **sqlx offline mode** — after changing any `query!` macro or migration, run `cargo sqlx prepare --workspace` and commit the `.sqlx/` directory. CI builds with `SQLX_OFFLINE=true`.
2. **Proto regeneration** — after changing `.proto` files, run `buf generate`. Both Rust and TypeScript clients need regeneration. `buf breaking --against .git#branch=main` catches compatibility issues.
3. **Connect client changes** — frontend transport auto-discovers services. New service hooks go in `lib/hooks/` if shared, or in `views/<feature>/hooks/` if feature-local.
4. **Auth interceptor** — all RPCs require authentication except: `GetSetupStatus`, `CompleteSetup`, `PreviewBackup`, `RestoreBackup`, `Login`. Adding new public RPCs requires updating the interceptor allow-list.
5. **Encrypted secrets** — `config.secrets` values are encrypted at rest. The `GetSource` RPC never returns secret values — only a boolean indicating whether each secret is set.
6. **Restate journal and secrets** — never decrypt secret material inside a `ctx.run()` closure. Restate journals side-effect results for replay, so decrypted tokens would be persisted in plaintext in the Restate journal.

## Code Style

### Rust

- Prefer `match` over `if/else-if` on the same variable
- Extract closures >10 lines into named functions
- DRY 3+ similar blocks into helpers
- Use `tracing` for logging, never `println!`/`eprintln!`
- Use structured tracing fields: `tracing::info!(repo = %name, count = items.len(), "fetched items")` — not bare string interpolation
- Use domain enums, never string literals for platform/status/type values
- Implement `From`/`Into` for mechanical enum conversions between domain and proto types

### TypeScript

- `const`/`let` only, never `var`
- Arrow functions, never `function` declarations
- Template literals for interpolation
- Absolute imports with `@ps/*` alias, no relative parent imports
