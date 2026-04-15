# Claude Code Guidelines for Prism

## Project Overview

Prism is an engineering insights platform for understanding team and individual performance across multiple platforms (GitHub, Jira, Discourse, Launchpad, Google Drive, mailing lists). Built in Rust (backend) + Vite/React (frontend) with PostgreSQL, gRPC (tonic + Connect), and Restate for ingestion orchestration.

## Build & Test Commands

```bash
mise install                              # Install all dev tools (one-time setup)
mise run install-deps                     # Install native OS packages (one-time setup)
prek install                              # Install git hooks (one-time setup)
prek run -av                              # All lints, tests, formatters — run before finishing any task
mise run fmt                              # Format all files (rustfmt, oxfmt via vp)
mise run check                            # Full CI validation (fmt + clippy + lint + typecheck)
mise run test                             # Run all tests (Rust + frontend)
mise run fix                              # Auto-fix all code (format + clippy fix + lint fix)
mise run generate                         # Generate all derived files (proto + sqlx)
cargo build                               # Build all crates
cargo nextest run                         # Run all Rust tests (unit + integration)
cargo nextest run -p ps-integration       # Run integration tests only
buf lint                                  # Lint proto files
buf generate                              # Generate Rust + TypeScript code from protos
cargo sqlx prepare --workspace            # Update offline query cache (.sqlx/)
sqlx migrate add <name>                   # Create new migration in migrations/
bun install                               # Install frontend dependencies (run from frontend/)
vp dev                                    # Start frontend dev server (run from frontend/)
vp check                                  # Frontend fmt + lint + typecheck (run from frontend/)
vp test run                               # Run frontend tests (run from frontend/)
```

## Local Dev Services (Tilt)

Tilt port-forwards all infrastructure to localhost. Use these for troubleshooting, sqlx query cache updates, and ad-hoc queries:

| Service | Port(s) | Credentials / Connection |
| --- | --- | --- |
| PostgreSQL | `5432` | `DATABASE_URL=postgres://prism:prism-dev-password@localhost:5432/prism` |
| Restate Admin API | `9070` | `curl http://localhost:9070/...` — manage invocations, deployments, state |

For sqlx query cache updates: `mise run generate:sqlx`

Connect to the dev database with psql: `kubectl exec -it postgres-0 -- psql -U prism -d prism`

Useful Restate commands:
- List invocations: `restate invocations list`
- Cancel stuck invocation: `restate invocations cancel <id>`
- Re-register deployment: `restate deployments register http://localhost:9080/ --force --yes`

## Workflow Requirements

**Before finishing any task**, always:

1. Run `prek run -av`
2. Ensure **zero warnings** from `cargo clippy` and `mise run fmt` — lints must be 100% clean before committing
3. Consider if the test coverage needs updating
4. If the change affects architecture, technology choices, or key conventions, update the relevant file in `docs/` to reflect the current state
5. If the change represents a significant decision or reversal of a previous decision, add a dated entry to `docs/08-decision-log.md` with context, decision, and rationale
6. Provide a **draft commit message** using Conventional Commits format

**Commit rules:**

- Use `--no-gpg-sign` when committing autonomously
- Always commit in logical chunks along the way. Don't wait to be prompted.
- **`.sqlx/` changes go in a separate commit** with message `chore: update sqlx query cache` — never mix query cache updates with code changes.
- **`docs/` changes go in a separate commit** with message `docs: <short description>` — never mix documentation updates with code changes.

## Code Structure

Code is organised **feature-first, layer-second**. See `docs/01-architecture.md` for the full strategy, invariants, and worked examples.

- **Frontend:** feature UI lives in `views/<feature>/` with `components/`, `hooks/`, `pages/` subdirs. Routes defined in `app.tsx` with lazy imports. Shared components in `components/`. Shared hooks in `lib/hooks/`.
- **Rust services:** features live under `src/features/<name>/` with handler, service, repository, types files. No layer-first `services/` buckets.
- **Three-tier escalation:** feature-local → service/app-local → shared crate/package. Only lift when a concrete second consumer exists.
- **No `utils/` or `helpers/` directories.** Give utilities a proper home.
- **Tests colocated** with source files. No `__tests__/` directories. Rust uses inline `#[cfg(test)]`.
- **File size limit** — split files exceeding ~500 lines into modules.

## Key Conventions

### sqlx — Type-Safe Queries Only

**Always** use `sqlx::query!`, `sqlx::query_as!`, `sqlx::query_scalar!`. **Never** use the runtime `sqlx::query()` string-based function. Schema changes must be caught at compile time.

### Repository Layer

All `sqlx::query!` calls live in `ps-core/src/repo/` — services and ingestion sources never contain direct SQL. Services receive `Repos`, never a raw `PgPool`. One repo per schema. See `.claude/rules/repository.md` for full layering rules.

### Domain Enums — Strong Typing with TEXT Storage

Use Rust enums stored as `TEXT` in PostgreSQL via `impl_sqlx_text!`. Never use string literals like `"github"` or `"merged"` — always use domain enums (`Platform`, `ContributionType`, `ContributionState`, `IngestionStatus`, `PeriodType`, `Role`). Enums live in `ps-core/src/models/enums.rs`.

### Configuration in DB, Not Files

Source credentials are encrypted in `config.secrets` using AES-256-GCM. Only `PS_SECRET_KEY` comes from environment. All other configuration via admin UI.

### Traceability

Every metric, insight, or AI-generated output **must** be auditable back to source data. Static metrics link to contributing data points. AI enrichments store model, input, and confidence. The UI must provide a "show how this was calculated" affordance.

### Proto & Code Generation

Proto files live in `proto/canonical/prism/v1/`. After changes: `buf lint` → `buf generate` → rebuild both backend and frontend. `buf breaking --against .git#branch=main` catches compatibility issues.

### Security Conventions

- **Fail-closed auth** — missing auth header returns an error. Non-public RPCs without auth are rejected by the interceptor.
- **Admin role enforcement** — privileged operations must call `require_admin()`.
- **Error masking** — log full errors server-side with `tracing`, return generic "internal error" to clients. Never expose DB error details.
- **LIKE pattern escaping** — always escape `%` and `_` in user-supplied search terms.
- **Input validation** — validate external identifiers before interpolation.
- **Secret material** — never decrypt secrets inside Restate `ctx.run()`. The journal persists side-effect results.
- **Auth interceptor allow-list** — all RPCs require auth except: `GetSetupStatus`, `CompleteSetup`, `PreviewBackup`, `RestoreBackup`, `Login`. Update the interceptor when adding new public RPCs.

### Performance Conventions

- **Batch writes with `UNNEST`** — bulk upserts use `UNNEST` arrays, not per-row loops.
- **`tokio::try_join!`** for independent async operations.
- **`futures::stream::buffer_unordered(N)`** for capped concurrent work.
- **Params structs** when a function takes >5 parameters.

### Restate — All Background Work

All long-running work **must** run as Restate handlers — never as synchronous gRPC RPCs. See `.claude/rules/restate-handlers.md` for journaling rules, macros, and handler patterns.

### Frontend

shadcn/ui (built on `@base-ui/react`, not Radix) is the standard component library — never hand-roll with raw Tailwind. See `.claude/rules/frontend-ui.md` for detailed UI conventions. Key rules:

- **No horizontal overflow** — use `min-w-0`, `overflow-hidden`, `overflow-x-auto`
- **DataTable** for all tables — never raw `<Table>` primitives
- **24-hour clock only** — never 12-hour format or AM/PM
- **Sonner** for toasts — fire in mutation `onSuccess`/`onError` callbacks
- **Lucide React** is the only icon library
- **React Query** is the only state management library — no Redux, Jotai, nanostores
- **Zod at boundaries only** — forms, file uploads, localStorage. Not for proto responses or internal args.
- To add new shadcn components: `bunx shadcn@latest add <component-name>`

### Testing

Test against real PostgreSQL, never mock the database. External APIs mocked with `wiremock`. Tests run via `cargo nextest run`. Frontend tests: Vitest + React Testing Library + happy-dom. See `docs/07-development.md` for test contexts and patterns.

## Gotchas

1. **sqlx offline mode** — after changing any `query!` macro or migration, run `cargo sqlx prepare --workspace` and commit `.sqlx/`. CI builds with `SQLX_OFFLINE=true`.
2. **Proto regeneration** — `buf generate` after `.proto` changes. Both Rust and TypeScript need regeneration.
3. **Connect clients** — frontend transport auto-discovers services. New hooks go in `lib/hooks/` (shared) or `views/<feature>/hooks/` (feature-local).
4. **Encrypted secrets** — `GetSource` RPC never returns secret values, only booleans indicating if set.
5. **Restate journal** — never decrypt secrets inside `ctx.run()`. Journal persists results, defeating at-rest encryption.

## Code Style

### Rust

- Prefer `match` over `if/else-if` on the same variable
- Extract closures >10 lines into named functions
- DRY 3+ similar blocks into helpers
- Use `tracing` for logging, never `println!`/`eprintln!`
- Structured tracing fields: `tracing::info!(repo = %name, count = items.len(), "fetched items")`
- Domain enums everywhere, never string literals
- `From`/`Into` for mechanical enum conversions between domain and proto types

### TypeScript

- `const`/`let` only, never `var`
- Arrow functions, never `function` declarations
- Template literals for interpolation
- Absolute imports with `@ps/*` alias, no relative parent imports

## Documentation

When working on specific subsystems, read the relevant doc for full architecture and patterns:

| Working on... | Read |
| --- | --- |
| System architecture, crate roles, code organisation | `docs/01-architecture.md` |
| Database queries, repos, migrations | `docs/02-database.md` |
| Ingestion handlers, Restate, Source trait | `docs/03-ingestion.md` |
| AI enrichment, embeddings, agentic query | `docs/04-ai-reasoning.md` |
| Frontend UI, state, components | `docs/05-frontend.md` |
| Containers, K8s, proto tooling | `docs/06-infrastructure.md` |
| Tests, test contexts, naming conventions | `docs/07-development.md` |
