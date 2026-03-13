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
├── views/            # Feature modules (sources, teams, dashboard, login, etc.)
│   ├── sources/      #   components/, hooks/, lib/, pages/
│   └── teams/        #   components/, hooks/, pages/
├── components/       # Service-level: app-shell, page-header, ui/ (shadcn)
└── lib/              # Service plumbing: api/, hooks/ (shared), session, providers
```

### Crate structure

```
crates/
├── ps-core/          # Domain types, traits, error types, shared logic
│   └── src/
│       ├── repo/     # Repository layer — ALL database access lives here
│       ├── auth/     # Password hashing, token generation, session management
│       ├── crypto.rs # AES-256-GCM encryption for source credentials
│       └── backup.rs # Export/import logic
├── ps-proto/         # Generated Rust code from proto definitions (pedantic lints disabled)
├── ps-server/        # API server binary (tonic + tonic-web), services, auth interceptor
├── ps-ingestion/     # Ingestion service binary + source modules
├── ps-metrics/       # Metric computation logic (DORA, flow, etc.)
├── ps-migrate/       # Migration binary for k8s init container
└── psctl/            # Lightweight CLI client (depends only on ps-proto)
```

**Dependency flow:** `psctl → ps-proto` | `ps-server → ps-core, ps-proto, ps-metrics` | `ps-ingestion → ps-core, ps-proto` | `ps-metrics → ps-core`

### Repository Layer (`ps-core/src/repo/`)

All database access is centralized in the repository layer. Each repo maps to one database schema (bounded context):

| Repo           | Schema     | Responsibility                                         |
| -------------- | ---------- | ------------------------------------------------------ |
| `AuthRepo`     | `auth`     | Users, sessions, API tokens                            |
| `ConfigRepo`   | `config`   | Source configs, encrypted secrets                      |
| `OrgRepo`      | `org`      | People, teams, platform identities, repositories       |
| `ActivityRepo` | `activity` | Contributions, watermarks, ingestion runs, ETag cache  |

The `Repos` struct bundles all four repos and is constructed once from a `PgPool`, then cloned into each service and the ingestion handler.

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

Vite + React Router SPA + React + shadcn/ui (built on `@base-ui/react` primitives) + TypeScript (strict mode, type-checked with typescript-go). Bun as runtime/package manager. Connect clients generated from proto definitions. React Query for server state. Tremor for charts. Production container serves static files via Caddy.

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

### Ingestion

Sources implement a common `Source` trait. Orchestrated by Restate virtual objects (one per source). Each step (plan, fetch, store, advance) is a named `ctx.run()` side effect. Rate limit backoff uses durable `ctx.sleep()`. Watermarks stored in PostgreSQL.

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

## Code Style

### Rust

- Prefer `match` over `if/else-if` on the same variable
- Extract closures >10 lines into named functions
- DRY 3+ similar blocks into helpers
- Use `tracing` for logging, never `println!`/`eprintln!`

### TypeScript

- `const`/`let` only, never `var`
- Arrow functions, never `function` declarations
- Template literals for interpolation
- Absolute imports with `@ps/*` alias, no relative parent imports
