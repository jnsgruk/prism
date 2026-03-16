# Repository Layer Refactoring — DDD-Style Architecture

## Context

All 62 `sqlx::query!` calls across ps-server and ps-ingestion are inline in gRPC service handlers and source adapters. There is no separation between application logic (orchestration, validation, proto mapping) and infrastructure (database access). This makes it hard to see which tables a service touches, leads to duplicated query patterns across crates, and violates DDD layering principles.

**Goal:** Introduce a repository layer in ps-core so that database access is encapsulated behind clean domain-oriented interfaces. Services and source adapters call repository methods instead of embedding SQL directly.

## Architecture

### Layering

```
┌─────────────────────────────────────────────────────┐
│  Presentation: gRPC services (ps-server),           │
│                Restate handlers (ps-ingestion)       │
├─────────────────────────────────────────────────────┤
│  Application: Source trait impls, service orchestration│
├─────────────────────────────────────────────────────┤
│  Domain: Models, traits, error types  (ps-core)     │
├─────────────────────────────────────────────────────┤
│  Infrastructure: Repositories         (ps-core/repo)│
└─────────────────────────────────────────────────────┘
```

### Why repositories live in ps-core

- ps-core already depends on sqlx (for `From<sqlx::Error>` on the `Error` type)
- Both ps-server and ps-ingestion need shared data access (platform_identities, source_configs, contributions, watermarks)
- No benefit to a separate `ps-repo` crate — adds a dependency node for no gain
- Repositories return domain model types already defined in ps-core

### Concrete structs, not traits

We test against real PostgreSQL (per CLAUDE.md), never mock the database. No need for trait indirection — repositories are concrete structs holding `PgPool`. They must be `Clone` (PgPool is `Arc` internally) so Restate closures can clone them.

## Module Layout

```
crates/ps-core/src/
├── repo/
│   ├── mod.rs          # Repos bundle struct, re-exports
│   ├── auth.rs         # AuthRepo: users, sessions (12 queries)
│   ├── config.rs       # ConfigRepo: source_configs, secrets (14 queries)
│   ├── org.rs          # OrgRepo: people, teams, memberships, identities, repositories (18 queries)
│   └── activity.rs     # ActivityRepo: contributions, watermarks, etag_cache, runs (10 queries)
├── models/             # (unchanged — pure value types)
├── ingestion.rs        # IngestionContext: pool → repos
└── ...
```

## Key Design Decisions

### Repos bundle

A single `Repos` struct wraps all four repos and is passed to services/handlers instead of raw `PgPool`:

```rust
#[derive(Clone)]
pub struct Repos {
    pub auth: AuthRepo,
    pub config: ConfigRepo,
    pub org: OrgRepo,
    pub activity: ActivityRepo,
}

impl Repos {
    pub fn new(pool: PgPool) -> Self { ... }
}
```

### Method return types

All methods return `Result<T, ps_core::Error>` — not `sqlx::Error`. This keeps consumers decoupled from sqlx error types and allows domain validation errors.

### Cross-schema joins

The `get_source_statuses` query joins `config.source_configs` with `activity.ingestion_watermarks`. This lives in `ActivityRepo` since the watermark data is the primary value; it returns a dedicated `SourceStatusRow` type.

### Backup counts/exports

The admin service's backup queries (COUNT from multiple tables, SELECT all rows for export) become individual repo calls: `config.count_sources()`, `org.count_people()`, `config.export_all()`, `org.export_people()`, etc.

### IngestionContext change

```rust
// Before:
pub struct IngestionContext {
    pub pool: PgPool,
    pub source_config: SourceConfig,
    pub secret_key: [u8; 32],
    pub http_client: reqwest::Client,
}

// After:
pub struct IngestionContext {
    pub repos: Repos,
    pub source_config: SourceConfig,
    pub secret_key: [u8; 32],
    pub http_client: reqwest::Client,
}
```

Source adapters change `sqlx::query!(...).execute(&ctx.pool)` to `ctx.repos.activity.upsert_contribution(...)`.

### Service constructors

```rust
// Before:
let org_service = OrgServiceImpl::new(pool.clone());

// After:
let repos = Repos::new(pool.clone());
let org_service = OrgServiceImpl::new(repos.clone());
```

### AuthLayer

The Tower interceptor currently takes `PgPool`. It changes to take `AuthRepo` directly since it only needs session validation.

## Repository Method Signatures

### AuthRepo (12 queries → ~10 methods)

Source files: `ps-server/services/auth.rs`, `ps-server/services/admin.rs`, `ps-server/interceptor.rs`

- `validate_session(token_hash: &str) -> Result<Option<SessionWithUser>, Error>`
- `touch_session(session_id: Uuid)`
- `any_users_exist() -> Result<bool, Error>`
- `create_user(id, username, display_name, password_hash, role) -> Result<(), Error>`
- `find_user_by_username(username: &str) -> Result<Option<UserCredentials>, Error>`
- `create_session(id, user_id, token_hash, session_type, expires_at, token_name) -> Result<(), Error>`
- `delete_session(session_id: Uuid) -> Result<(), Error>`
- `list_api_tokens(user_id: Uuid) -> Result<Vec<ApiTokenRow>, Error>`
- `delete_api_token(token_id: Uuid, user_id: Uuid) -> Result<bool, Error>`
- `count_users() -> Result<i64, Error>`
- `export_users() -> Result<Vec<serde_json::Value>, Error>`

### ConfigRepo (14 queries → ~13 methods)

Source files: `ps-server/services/config.rs`, `ps-ingestion/github/source.rs`

- `list_sources() -> Result<Vec<SourceConfig>, Error>`
- `get_source(id: Uuid) -> Result<Option<SourceConfig>, Error>`
- `get_enabled_source_by_name(name: &str) -> Result<Option<SourceConfig>, Error>`
- `create_source(id, source_type, name, settings, schedule_cron) -> Result<SourceConfig, Error>`
- `update_source_enabled(id: Uuid, enabled: bool) -> Result<(), Error>`
- `update_source_settings(id: Uuid, settings: &serde_json::Value) -> Result<(), Error>`
- `update_source_schedule(id: Uuid, cron: &str) -> Result<(), Error>`
- `delete_source(id: Uuid) -> Result<bool, Error>`
- `list_secret_keys(source_id: Uuid) -> Result<Vec<String>, Error>`
- `get_encrypted_secret(source_id: Uuid, key: &str) -> Result<Option<Vec<u8>>, Error>`
- `upsert_secret(id, source_id, key, encrypted) -> Result<(), Error>`
- `count_sources() -> Result<i64, Error>`
- `export_sources() -> Result<Vec<serde_json::Value>, Error>`

### OrgRepo (18 queries → ~16 methods)

Source files: `ps-server/services/org.rs`, `ps-ingestion/github/identity.rs`, `ps-ingestion/github/repos.rs`

- `list_teams(parent_filter: Option<Uuid>) -> Result<Vec<TeamWithCount>, Error>`
- `get_team(id: Uuid) -> Result<Option<TeamWithCount>, Error>`
- `find_team_by_name(name: &str, org_name: &str) -> Result<Option<Uuid>, Error>`
- `create_team(id, name, org_name) -> Result<(), Error>`
- `list_people() -> Result<Vec<Person>, Error>`
- `get_team_members(team_id: Uuid) -> Result<Vec<Person>, Error>`
- `find_person_by_directory_id(dir_id: &str) -> Result<Option<Uuid>, Error>`
- `create_person(...) -> Result<Uuid, Error>`
- `update_person(...) -> Result<(), Error>`
- `get_identities_for_people(person_ids: &[Uuid]) -> Result<Vec<PlatformIdentity>, Error>`
- `batch_resolve_person_ids(platform: &str, usernames: &[String]) -> Result<HashMap<String, Uuid>, Error>`
- `upsert_identity(...) -> Result<(), Error>`
- `has_active_membership(person_id, team_id) -> Result<bool, Error>`
- `create_membership(...) -> Result<(), Error>`
- `upsert_repository(...) -> Result<(), Error>`
- `count_people/count_teams/export_people/export_teams()` for backup

### ActivityRepo (10 queries → ~11 methods)

Source files: `ps-ingestion/github/source.rs`, `ps-ingestion/github/etag.rs`, `ps-ingestion/handler.rs`, `ps-server/services/ingestion.rs`

- `upsert_contribution(id, person_id, item: &ContributionInput) -> Result<(), Error>`
- `get_watermark(source_name: &str) -> Result<Option<String>, Error>`
- `upsert_watermark(source_name, value, items) -> Result<(), Error>`
- `get_cached_etag(source_name, endpoint) -> Result<Option<String>, Error>`
- `set_cached_etag(source_name, endpoint, etag) -> Result<(), Error>`
- `create_run(id, source_name) -> Result<(), Error>`
- `complete_run(id, items_collected) -> Result<(), Error>`
- `fail_run(id, error_message) -> Result<(), Error>`
- `list_runs(source_name: Option<&str>) -> Result<Vec<IngestionRun>, Error>`
- `get_source_statuses() -> Result<Vec<SourceStatusRow>, Error>` (cross-schema join)

## Implementation Tasks

### Task 1: Repo module skeleton + Repos bundle

**Create:** `crates/ps-core/src/repo/mod.rs`, add `pub mod repo;` to `lib.rs`
- Define `Repos` struct with four repo fields
- Define row types needed by repos (e.g. `SessionWithUser`, `UserCredentials`, `TeamWithCount`, `SourceStatusRow`, `ApiTokenRow`)
- Place row types in the relevant repo module

**Modify:** `crates/ps-core/src/lib.rs`

### Task 2: ActivityRepo + IngestionContext migration

The most cross-crate repo. Migrate all activity schema queries.

**Create:** `crates/ps-core/src/repo/activity.rs`
**Modify:**
- `crates/ps-core/src/ingestion.rs` — `IngestionContext.pool` → `IngestionContext.repos`
- `crates/ps-ingestion/src/github/source.rs` — `store_batch_impl`, `advance_watermark_impl`, `plan_impl` (watermark read)
- `crates/ps-ingestion/src/github/etag.rs` — replace module with repo calls (module may become trivial or removed)
- `crates/ps-ingestion/src/handler.rs` — run create/complete/fail, context construction
- `crates/ps-server/src/services/ingestion.rs` — `get_status`, `list_runs`

### Task 3: ConfigRepo

**Create:** `crates/ps-core/src/repo/config.rs`
**Modify:**
- `crates/ps-server/src/services/config.rs` — all CRUD + secret operations
- `crates/ps-ingestion/src/github/source.rs` — `decrypt_token` (get_encrypted_secret)
- `crates/ps-ingestion/src/handler.rs` — load source config
- `crates/ps-server/src/services/admin.rs` — backup count + export queries for source_configs

### Task 4: OrgRepo

Largest service. The `import_directory` transaction logic moves into `OrgRepo`.

**Create:** `crates/ps-core/src/repo/org.rs`
**Modify:**
- `crates/ps-server/src/services/org.rs` — all people/team/identity queries
- `crates/ps-ingestion/src/github/identity.rs` — `batch_resolve_person_ids` (may become a thin wrapper or removed)
- `crates/ps-ingestion/src/github/repos.rs` — `upsert_repository`
- `crates/ps-server/src/services/admin.rs` — backup count + export for people/teams

### Task 5: AuthRepo

**Create:** `crates/ps-core/src/repo/auth.rs`
**Modify:**
- `crates/ps-server/src/services/auth.rs` — user creation, login, session management
- `crates/ps-server/src/services/admin.rs` — API token CRUD, backup user export
- `crates/ps-server/src/interceptor.rs` — session validation, touch. `AuthLayer::new` takes `AuthRepo` instead of `PgPool`

### Task 6: Wire up Repos in main.rs files + test server

**Modify:**
- `crates/ps-server/src/main.rs` — construct `Repos`, pass to all services
- `crates/ps-ingestion/src/main.rs` — construct `Repos`, pass to handler
- `tests/integration/src/common/server.rs` — construct `Repos`, pass to test services
- All service `impl` blocks — `self.pool` → `self.repos.{context}`

### Task 7: Cleanup + verification

- Remove now-unused `PgPool` fields from service structs
- Remove thin wrapper modules that are now just forwarding to repos (e.g. `github/identity.rs`, `github/etag.rs` may shrink to nothing)
- `cargo sqlx prepare --workspace` (queries moved to new file paths)
- `prek run -av` — zero warnings
- `nix fmt`

## Verification

1. `cargo build` — all crates compile
2. `cargo clippy` — zero warnings
3. `cargo test` — all unit + integration tests pass unchanged
4. `prek run -av` — full check suite green
5. No functional changes — pure structural refactoring, all existing behaviour preserved

## Migration Strategy

Tasks 2-5 are independent bounded contexts and can be done in any order. Recommended: T1 → T2 (activity + IngestionContext, most impactful) → T3 (config) → T4 (org, largest) → T5 (auth) → T6 → T7. Each task is a self-contained commit.
