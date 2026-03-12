# Claude Code Guidelines for Prism

## Project Overview

Prism is an engineering insights platform for understanding team and individual performance across multiple platforms (GitHub, Jira, Discourse, Launchpad, Google Drive, mailing lists). Built in Rust (backend) + Next.js/React (frontend) with PostgreSQL, gRPC (tonic + Connect), and Restate for ingestion orchestration.

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

## Architecture

### Crate Structure

```
crates/
├── ps-core/          # Domain types, traits, error types, shared logic
│   └── src/
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

Next.js App Router + React + shadcn/ui (Base UI primitives) + TypeScript (strict mode, type-checked with typescript-go). Bun as runtime/package manager. Connect clients generated from proto definitions. nanostores for client state. React Query for server state. Tremor for charts.

**shadcn/ui is the standard UI component library.** Always use components from `@/components/ui/` (Dialog, Button, Card, Input, Label, Select, Tabs, Badge, Table, Alert, Separator, DropdownMenu) rather than hand-rolling UI with raw Tailwind. To add new shadcn components: `bunx shadcn@latest add <component-name>`. Components use `@ps/utils` for the `cn` helper.

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

Vitest + React Testing Library + happy-dom. API mocking via `createRouterTransport` (Connect, in-memory, type-safe). Fresh `QueryClient` per test with `retry: false`. `cleanStores()` in `afterEach` for nanostores.

Test custom hooks, nanostore logic, data transformations, interactive components. Don't test shadcn/ui primitives, chart SVG output, Next.js routing, or CSS.

## Gotchas

1. **sqlx offline mode** — after changing any `query!` macro or migration, run `cargo sqlx prepare --workspace` and commit the `.sqlx/` directory. CI builds with `SQLX_OFFLINE=true`.
2. **Proto regeneration** — after changing `.proto` files, run `buf generate`. Both Rust and TypeScript clients need regeneration. `buf breaking --against .git#branch=main` catches compatibility issues.
3. **Connect client changes** — frontend `TransportProvider` auto-discovers services, but new services need their hooks added to `frontend/lib/hooks/`.
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
