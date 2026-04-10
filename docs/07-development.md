# Development

## Testing Strategy

### Rust — Integration Tests Are Primary

Test against real PostgreSQL, never mock the database. External APIs (GitHub, Jira, Discourse) mocked with wiremock.

Tests must be run via `cargo nextest run`. The integration test crate requires the nextest setup script to provision a database.

**Database provisioning** is automatic:
1. Setup script starts a `pgvector/pgvector:pg17` Docker container (or reuses an existing one)
2. Creates a pre-migrated template database (`ps_template`)
3. Each test creates an isolated database from the template via `CREATE DATABASE ... TEMPLATE` (near-instant filesystem copy)
4. Watchdog process stops the container when nextest exits

Set `DATABASE_URL` externally to skip the container and use an existing Postgres (useful in CI).

### Three Test Contexts

Tests are plain `#[tokio::test]` async functions using one of three context types:

| Context | Provides | Use for |
| --- | --- | --- |
| `ApiTestContext` | Real gRPC server (`server.channel`, `server.pool`, `server.addr`) | API-layer tests |
| `RepoTestContext` | `repos`, `pool` (no server) | Repository and metrics tests |
| `SourceTestContext` | `mock_server`, `repos`, `pool`, `build_ingestion_ctx()` | Source adapter tests |

```rust
#[tokio::test]
async fn my_test() {
    let ctx = RepoTestContext::new().await;
    // ... test using ctx.repos, ctx.pool ...
    ctx.teardown().await;
}
```

Test infrastructure lives in `tests/integration/src/`:
- `common/db.rs` — TestDb, RepoTestContext
- `common/server.rs` — TestServer, ApiTestContext
- `common/wiremock_helpers.rs` — SourceTestContext + mock response builders
- `common/fixtures.rs` — `create_admin_user()` and other data builders

### Frontend Tests

Vitest + React Testing Library + happy-dom. API mocking via `createRouterTransport` (Connect, in-memory, type-safe). Fresh `QueryClient` per test with `retry: false`.

Test custom hooks, data transformations, interactive components. Don't test shadcn/ui primitives, chart SVG output, React Router config, or CSS.

## Naming Conventions

### Rust

| Element | Convention | Example |
| --- | --- | --- |
| Files/modules | snake_case | `repository.rs` |
| Module entry | `mod.rs` | |
| Doc comments | `//!` at top of handler files | `//! Sources — gRPC CRUD` |

### TypeScript

| Element | Convention | Example |
| --- | --- | --- |
| All files | kebab-case | `source-card.tsx` |
| Hooks | `use-` prefix, `.ts` | `use-sources.ts` |
| Pages | `-page` suffix, `.tsx` | `sources-page.tsx` |
| Tests | `.test.ts` / `.test.tsx` | `parser.test.ts` |
| Zod schemas | `.schemas.ts` suffix | `source-config.schemas.ts` |
| Type files | `.types.ts` suffix | `source.types.ts` |

### Imports

TypeScript uses absolute paths — no relative parent (`../`) imports. `@ps/...` for shared lib, `@/...` for app-level paths. Same-directory relative (`./sibling`) is permitted.

## Test Placement

Tests are colocated with source in both languages. No separate test directories.

- **Rust:** inline `#[cfg(test)] mod tests` at the bottom of the file. Integration tests in `tests/integration/`.
- **TypeScript:** `*.test.ts` alongside the file under test. No `__tests__/` directories.

## Workflow

Before finishing any task:

1. Run `prek run -av` (all lints, tests, formatters)
2. Ensure zero warnings from `cargo clippy` and `mise run fmt`
3. Consider if test coverage needs updating
4. If the change affects architecture or conventions, update the relevant `docs/` file
5. If the change represents a significant decision, add a dated entry to `docs/08-decision-log.md`

### Commit Conventions

- Conventional Commits format
- Use `--no-gpg-sign` when committing autonomously
- Commit in logical chunks — don't wait to be prompted
- `.sqlx/` changes go in a separate commit with message `chore: update sqlx query cache`

## Code Style

See [CLAUDE.md](../CLAUDE.md) for detailed code style rules. Key principles:

**Rust:** prefer `match` over `if/else-if`, extract large closures into named functions, use `tracing` (never `println!`), structured tracing fields, domain enums (never string literals), `From`/`Into` for proto conversions.

**TypeScript:** `const`/`let` only, arrow functions, template literals, absolute imports with `@ps/*` alias.

**Both:** DRY 3+ similar blocks, split files over ~500 lines into modules, use params structs for > 5 parameters.
