# Phase 1: Foundation — Detailed Implementation Plan

Stand up the core platform and prove the end-to-end data pipeline with a single source (GitHub). By the end of this phase, a deployed system ingests GitHub data for configured teams and displays team-level PR metrics in the UI.

**References:**
[Architecture Overview](./01-architecture-overview.md) |
[Domain Model](./02-domain-model.md) |
[Data Ingestion Strategy](./03-data-ingestion-strategy.md) |
[Database Design](./04-database-design.md) |
[Frontend Strategy](./05-frontend-strategy.md) |
[Authentication](./07-authentication.md) |
[Open Questions](./08-open-questions.md) |
[Restate vs Temporal Spike](./09-spike-restate-vs-temporal.md)

---

## 1. Workstreams

Phase 1 is organised into eight workstreams. W0 establishes project tooling before anything else. Three workstreams (W1, W2, W3) can start once W0 is complete; the others have dependencies described in section 3.

| #   | Workstream                     | Summary                                                                          |
| --- | ------------------------------ | -------------------------------------------------------------------------------- |
| W0  | Project Tooling & Standards    | Nix flake, devshell, treefmt, prek, clippy config, CLAUDE.md, direnv             |
| W1  | Restate vs Temporal Spike      | Resolve the orchestration question before committing to ingestion implementation |
| W2  | Backend Scaffolding            | Rust workspace, crate structure, proto definitions, database migrations, CI      |
| W3  | Frontend Scaffolding           | Next.js project, Connect client generation, layout, component library setup      |
| W4  | Org Context, Directory Import & Source Configuration | People, teams, platform identities, and data source configuration — via API and UI |
| W5  | GitHub Ingestion               | Source adapter, Restate workflow, watermark tracking, identity resolution        |
| W6a | Ingestion Status UI            | Ingestion status page, manual trigger/backfill buttons, run history              |
| W6b | Metrics Computation & Team UI  | PR throughput, review turnaround, team comparison view                           |

---

## 2. Deliverables Per Workstream

### W0 — Project Tooling & Standards

Establish the development environment, build tooling, and project conventions before any application code is written. All tooling and libraries come from the Nix flake — nothing is installed globally or via other package managers at dev time.

**Reference:** The [brewlog](~/code/brewlog) project's `flake.nix` and `CLAUDE.md` serve as the template for this setup.

**Deliverables:**

1. **Nix flake** (`flake.nix`):

   Inputs:
   - `nixpkgs` (nixos-unstable)
   - `rust-overlay` (oxalica) for pinned Rust toolchain
   - `flake-parts` for per-system structure
   - `crane` for Rust build caching and Nix packaging
   - `treefmt-nix` for unified formatting
   - `git-hooks.nix` for pre-commit hooks via prek

   Rust toolchain via rust-overlay with extensions: `rust-src`, `clippy`, `rust-analyzer`, `rustfmt`.

   Dev shell packages (all from Nix):
   - Rust toolchain (clippy, rustfmt, rust-analyzer, rust-src)
   - `clang` + `mold` (linker) for fast compilation
   - `pkg-config`, `openssl` (for reqwest/tonic TLS)
   - `protobuf`, `buf` (protobuf tooling)
   - `sqlx-cli` (migrations, `cargo sqlx prepare`)
   - `postgresql` (client tools for local dev)
   - `bun` (frontend runtime and package manager)
   - `typescript-go` (type-checking)
   - `nil`, `nixfmt` (Nix LSP and formatting)
   - `cargo-watch` (live reload during dev)

2. **Fast compilation setup** (`.cargo/config.toml`):

   Use clang as the C compiler and mold as the linker for fastest possible iteration:

   ```toml
   [target.x86_64-unknown-linux-gnu]
   linker = "clang"
   rustflags = ["-C", "link-arg=-fuse-ld=mold"]

   [target.aarch64-unknown-linux-gnu]
   linker = "clang"
   rustflags = ["-C", "link-arg=-fuse-ld=mold"]
   ```

   Additionally, set `RUSTFLAGS` environment variable in the devshell if needed for dynamic linking during dev builds.

3. **treefmt configuration** (in `flake.nix`):

   | Formatter | Scope                                       |
   | --------- | ------------------------------------------- |
   | `rustfmt` | `*.rs`                                      |
   | `nixfmt`  | `*.nix`                                     |
   | `deadnix` | `*.nix` (dead code)                         |
   | `oxfmt`   | `*.ts`, `*.tsx`, `*.json`, `*.yaml`, `*.md` |
   | `shfmt`   | `*.sh`                                      |

   Both `oxfmt` and `oxlint` are available in nixpkgs. No need for prettier — oxfmt handles all JS/TS/JSON/YAML/MD formatting.

4. **prek (pre-commit) hooks** (in `flake.nix` via `git-hooks.nix`):

   | Hook         | Stage      | Behaviour                                            |
   | ------------ | ---------- | ---------------------------------------------------- |
   | `treefmt`    | pre-commit | Runs all formatters, fails fast, runs before clippy  |
   | `clippy`     | pre-commit | `--allow-dirty --fix`, fails fast, runs before tests |
   | `cargo-test` | pre-commit | Triggered on `.rs` and `.toml` changes               |
   | `vitest`     | pre-commit | `bun vitest run --reporter=dot` on `.ts`/`.tsx` changes  |
   | `oxlint`     | pre-commit | Runs on `.ts`/`.tsx` files                           |
   | `shellcheck` | pre-commit | Shell script linting                                 |
   | `buf-lint`   | pre-commit | Proto file linting                                   |

   The devshell's `shellHook` activates prek automatically.

5. **direnv integration** (`.envrc`):

   ```bash
   use flake
   ```

   Entering the project directory automatically loads the Nix devshell. All developers get identical tooling without manual setup. Add `.direnv/` to `.gitignore`.

6. **Clippy configuration** (`[workspace.lints.clippy]` in root `Cargo.toml`):

   Pedantic baseline with targeted allows for tonic/sqlx/serde compatibility:

   ```toml
   [workspace.lints.clippy]
   # Pedantic baseline
   pedantic = { level = "warn", priority = -1 }

   # Pedantic allows — noisy or incompatible with our stack
   missing_errors_doc = "allow"
   missing_panics_doc = "allow"
   module_name_repetitions = "allow"   # common in multi-crate workspaces
   must_use_candidate = "allow"
   return_self_not_must_use = "allow"
   needless_pass_by_value = "allow"    # tonic handlers, serde visitors require owned types
   cast_possible_truncation = "allow"  # proto/DB column mappings
   cast_sign_loss = "allow"
   similar_names = "allow"             # proto-generated field names
   unused_async = "allow"              # tonic trait methods must be async even without awaits
   struct_excessive_bools = "allow"    # serde config structs
   unnecessary_wraps = "allow"         # tonic/trait impls force Result returns
   wildcard_imports = "allow"          # idiomatic for proto re-exports
   used_underscore_binding = "allow"   # false positives with sqlx/serde macros
   too_many_lines = "allow"            # use clippy.toml threshold instead
   cast_lossless = "allow"             # proto i32→i64 conversions

   # Restriction lints — production guardrails
   dbg_macro = "deny"
   unimplemented = "deny"
   todo = "warn"
   unwrap_used = "warn"
   expect_used = "warn"
   print_stdout = "warn"              # use tracing, not println
   print_stderr = "warn"
   panic = "warn"
   indexing_slicing = "warn"          # prefer .get() to avoid panics
   ```

   Each member crate inherits via `[lints] workspace = true`. The `ps-proto` crate (generated code) overrides with `pedantic = "allow"`.

   Additional `clippy.toml` at workspace root:

   ```toml
   too-many-lines-threshold = 150
   cognitive-complexity-threshold = 30
   ```

7. **CLAUDE.md** (project root):

   Based on the brewlog CLAUDE.md structure, adapted for this project:
   - **Project overview** — what Prism is, the stack
   - **Build & test commands** — `prek run -av`, `cargo build`, `cargo test`, `cargo clippy --allow-dirty --fix`, `nix fmt`, `buf lint`, `buf generate`, `sqlx migrate add`, `bun dev`
   - **Workflow requirements** — always run `prek run -av` before finishing, consider test coverage, Conventional Commits, never add "Co-Authored-By" trailers, use `--no-gpg-sign` when committing autonomously
   - **Architecture** — workspace crate structure, bounded contexts, dependency flow
   - **Key conventions** — sqlx type-safe macros only (never `query()`), configuration in DB not files, traceability on every metric/insight, source credentials encrypted in DB (AES-256-GCM, key from `PS_SECRET_KEY` env var)
   - **Testing strategy** — see deliverable 8 below
   - **Gotchas** — sqlx offline mode workflow, proto code generation, Connect client regeneration after proto changes
   - **Code style** — Rust (match over if/else-if, extract closures >10 lines, DRY 3+ blocks), TypeScript (const/let, arrow functions, template literals)

8. **Testing strategy:**

   Testing is a first-class concern from day one, not an afterthought. Every workstream must include tests as part of its deliverables.

   **Unit tests** — minimal, inline `#[cfg(test)]` modules in Rust source files. Use these only for pure logic that benefits from co-location: parsing, transformation, validation, domain invariants. Don't unit-test things that are better covered by integration tests.

   **Integration tests** — the primary test surface. A separate `[[test]]` binary target (like brewlog's `tests/server/main.rs` and `tests/cli/main.rs`) that exercises the real stack:

   ```
   tests/
   ├── integration/
   │   ├── main.rs            # Test binary entry point
   │   ├── common/            # Shared fixtures, helpers, macros
   │   │   ├── mod.rs
   │   │   ├── fixtures.rs    # Test data factories (people, teams, contributions)
   │   │   └── server.rs      # Test server setup (real DB, real gRPC)
   │   ├── api/               # gRPC API tests (org, ingestion, metrics services)
   │   ├── ingestion/         # Source adapter tests (GitHub, with wiremock)
   │   ├── metrics/           # Metrics computation tests
   │   └── domain/            # Cross-cutting domain logic tests
   ```

   **What integration tests must cover:**
   - gRPC API endpoints — request/response, error cases, pagination, filtering
   - Ingestion pipeline — source adapters against wiremock, watermark advancement, identity resolution, upsert behaviour
   - Metrics computation — correct aggregation from known contribution data, period boundaries, team rollup
   - Directory import — parsing, upsert, team hierarchy, membership temporal handling
   - Database interactions — via real PostgreSQL (sqlx test fixtures), not mocks

   **Test macros** — use macros to eliminate duplication across similar test cases, following brewlog's pattern (`define_crud_tests!`, `define_datastar_entity_tests!`). For example:
   - `define_api_test!` — sets up a test server with DB, runs a closure, tears down
   - `define_source_test!` — sets up wiremock with fixture responses, runs ingestion, asserts contributions stored
   - `define_metric_test!` — seeds contributions, runs computation, asserts snapshot values

   **Principles:**
   - Test against real PostgreSQL, never mock the database (see feedback_sqlx memory)
   - External APIs (GitHub, Jira, Discourse) mocked with `wiremock`
   - Focus on testing what matters — business logic, data correctness, API contracts — not framework plumbing
   - Every new feature or source adapter must ship with integration tests; this is not optional
   - Tests run in prek pre-commit hooks and CI

   **Frontend tests** — lightweight, focused on custom logic and interactions:

   _Stack:_
   - **Vitest** as test runner (fast, native ESM/TS, runs under Bun)
   - **React Testing Library** + **happy-dom** for component tests (faster than jsdom; swap to jsdom per-file if Radix portals cause issues)
   - **`createRouterTransport`** from `@connectrpc/connect` for API mocking — built-in, in-memory, type-safe, exercises real serialization. No MSW or fetch mocking needed.
   - Fresh `QueryClient` per test with `retry: false` (TanStack testing guidance)
   - `cleanStores()` in `afterEach` for nanostores cleanup

   _Dev dependencies (7 total):_
   `vitest`, `@vitejs/plugin-react`, `vite-tsconfig-paths`, `@testing-library/react`, `@testing-library/dom`, `@testing-library/user-event`, `happy-dom`

   _What to test:_
   - Custom hooks wrapping Connect-Query calls (correct request params, loading/error states)
   - Nanostore logic (computed stores, derived state actions)
   - Data transformation utilities (metric formatting, date ranges, comparison logic)
   - Interactive components (filter panels, period selectors, team selection) — test user interactions, not implementation
   - Page-level smoke tests for each main view: render with mock transport, assert key content appears

   _What NOT to test:_
   - ShadCN/Radix primitives (tested upstream)
   - Tremor chart SVG output (test that correct data is passed, not rendering)
   - Next.js routing/layout wiring (framework responsibility)
   - CSS / Tailwind class presence
   - No snapshot/visual testing — internal app with small user base, not worth the noise

   _Shared test helper:_
   A `renderWithProviders(component, { transport?, queryClient? })` wrapper that provides `TransportProvider` + `QueryClientProvider` with sensible defaults. Used by all component and page tests.

   _prek integration:_
   Add `bun vitest run --reporter=dot` to pre-commit hooks alongside `cargo-test`. Use `--reporter=dot` for minimal output. If the suite grows beyond ~10s, switch to `bun vitest related --changed` to only run affected tests.

**Duration:** ~2-3 days.

**Exit criteria:** `direnv allow` drops into a working shell with all tools available. `prek run -av` passes (on an empty workspace with a stub crate). `nix fmt` works. `.cargo/config.toml` configured with clang + mold.

---

### W1 — Restate vs Temporal Spike — COMPLETE

This spike has been completed. See [evaluation.md](~/code/canonical/temporal-restate-spike/evaluation.md) for full results.

**Outcome: Restate confirmed** (scored 3.9 vs Temporal's 3.1 across six dimensions).

**Key findings:**

- Restate Rust SDK (0.9) is clean and first-class; Temporal Rust SDK (0.1.0-alpha.1) is alpha-quality with numerous ergonomic issues
- Restate: single binary, 3 containers total. Temporal: 4-5 containers, needs its own PostgreSQL schemas
- Temporal's UI for observability is superior, mitigated by our own ingestion status page + structured logging
- Both produced identical results ingesting 184 PRs from `canonical/chisel`

**Patterns to carry forward into `ps-ingestion`:**

1. `IngestionJob` trait — orchestrator-agnostic business logic
2. Virtual objects keyed by source name — per-source concurrency control
3. `ctx.run()` for named, retriable side effects (plan, fetch, store, advance)
4. `ctx.sleep()` for durable rate limit backoff
5. Watermarks in PostgreSQL, not Restate KV state

---

### W2 — Backend Scaffolding

Set up the Rust workspace, proto definitions, database migrations, and CI pipeline so all other workstreams have a foundation to build on.

**Deliverables:**

1. **Cargo workspace** with the crate structure from [01-architecture-overview.md](./01-architecture-overview.md):
   - `ps-core` — domain types, traits, error types, shared utilities (includes `auth` module for password hashing + session management, `backup` module for export/import logic)
   - `ps-server` — API server binary (tonic + tonic-web), `AuthService` implementation, `AdminService` implementation, auth interceptor
   - `ps-ingestion` — ingestion service binary + source modules
   - `ps-metrics` — metric computation logic
   - `ps-proto` — generated Rust code from proto definitions
   - `ps-migrate` — migration binary for the init container
   - `psctl` — lightweight CLI client over the gRPC API (depends only on `ps-proto`, not on `ps-core` or `ps-server`). See [section 7](#psctl--lightweight-cli-client) for full design.

2. **Proto definitions** (see section 5 for specifics):
   - `buf.yaml` and `buf.gen.yaml` at the workspace root
   - Initial `.proto` files in `proto/` for the services needed in Phase 1
   - `buf lint` passing, `buf generate` producing Rust and TypeScript outputs

3. **Database migrations** (run by `ps-migrate`):
   - Migrations for the Phase 1 subset of tables (see section 4)
   - `ps-migrate` binary that runs `sqlx::migrate!()` and seeds default config
   - Offline query cache (`.sqlx/`) checked in for CI

4. **Tiltfile** (local Kubernetes dev environment):

   Modelled on `~/code/cortexlabsai/connect/main/Tiltfile`. Tilt provides a live-reload Kubernetes development loop — code changes trigger rebuilds and hot-deploy into the local cluster without manual image pushes.

   **Reference patterns from cortexlabsai/connect:**
   - `allow_k8s_contexts` scoped to `docker-desktop` (prevent accidental prod deploy)
   - Rust services: `cargo watch` compiles on the host, Tilt syncs the binary into a lightweight dev container via `docker_build` — avoids full image rebuild per change
   - Next.js: `live_update` syncs source files into the container, HMR handles the rest
   - `resource_deps` enforces startup order: database → migrations → Restate → services
   - Envoy Gateway routes all HTTP traffic through `localhost` (no port juggling for the UI)
   - Direct port-forwards for non-HTTP services (PostgreSQL, Restate admin)
   - Build args for git commit hash and timestamp embedded in binaries

   **Scope for Prism (amd64/Linux only — no cross-compile or macOS splits):**

   | Resource       | Local compile          | Container                              | Access                                               |
   | -------------- | ---------------------- | -------------------------------------- | ---------------------------------------------------- |
   | Envoy Gateway  | —                      | `envoyproxy/gateway` (Helm)            | `localhost:80` — all HTTP traffic routes through here |
   | PostgreSQL     | —                      | `pgvector/pgvector:pg18` StatefulSet   | `localhost:15432` (port-forward)                     |
   | `ps-migrate`   | `cargo watch` → binary | K8s Job (runs before app, TTL cleanup) | —                                                    |
   | Restate        | —                      | `restatedev/restate:1.3`               | `localhost:9070` (admin, port-forward)               |
   | `ps-server`    | `cargo watch` → binary | Dev container (binary sync)            | `localhost/api/*` (via Envoy)                        |
   | `ps-ingestion` | `cargo watch` → binary | Dev container (binary sync)            | —                                                    |
   | Frontend       | —                      | `bun dev` (Next.js)                    | `localhost/*` (via Envoy, catch-all)                 |

   **Dockerfiles** — unified multi-stage pattern (modelled on `~/code/cortexlabsai/connect/main/crates/Dockerfile.rust`):

   A single `Dockerfile.rust` with `--target` selectors for each service. Shared stages avoid duplication:

   ```
   Stage 1: chisel        — ubuntu/go base, installs chisel, cuts minimal rootfs slices
                             (base-files, ca-certificates, libc6, libgcc-s1)
   Stage 2: chef          — rust base + clang + mold + protobuf + cargo-chef (pre-built binary, not compiled)
   Stage 3: planner       — cargo chef prepare (generates recipe.json from Cargo.lock)
   Stage 4: builder       — cargo chef cook (caches deps) → cargo build --release all binaries
                             SQLX_OFFLINE=true, BUILD_GIT_HASH/BUILD_TIMESTAMP as build args
   Stage 5+: per-service  — FROM scratch, COPY chisel rootfs + single binary, USER nobody
   ```

   Final targets: `ps-server`, `ps-ingestion`, `ps-migrate`. Build with `docker build --target ps-server -f Dockerfile.rust .`

   **Dev Dockerfile** (`Dockerfile.rust.dev`) — same chisel base but with additional slices for debugging (e.g. `coreutils_bins`, `bash_bins`). Receives pre-compiled binaries via Tilt sync rather than building inside the container.

5. **Envoy Gateway** (Kubernetes Gateway API):

   Bring across the Envoy Gateway configuration from `~/code/cortexlabsai/connect/main/k8s/gateway/`. This is validated and working with Docker Desktop K8s + Tilt in the connect repo.

   **Setup (from connect repo pattern):**
   - `k8s/gateway/kustomization.yaml` — deploys the Envoy Gateway Helm chart (`oci://docker.io/envoyproxy/gateway`) and GatewayClass
   - `k8s/gateway/namespace.yaml` — creates `envoy-gateway-system` namespace
   - `k8s/gateway/gatewayclass.yaml` — defines the `eg` GatewayClass with `gateway.envoyproxy.io/gatewayclass-controller`
   - Gateway CRDs applied before other resources in the Tiltfile (same pattern as connect: `kubectl apply --server-side` for CRDs, then `k8s_yaml(kustomize(...))`)

   **Gateway resource** — listener on HTTP port 80, hostname `localhost`:

   **Routes:**

   | Path prefix  | Backend          | Rewrite | Notes                                                          |
   | ------------ | ---------------- | ------- | -------------------------------------------------------------- |
   | `/api/*`     | `ps-server:8080` | `/api` → `/` | gRPC-Web / Connect protocol. Auth enforced by tonic interceptor (AuthService RPCs are public) |
   | `/restate/*` | `restate:8080`   | `/restate` → `/` | Restate HTTP ingress (for manual triggers)                     |
   | `/*`         | `frontend:3000`  | — | SPA catch-all                                                  |

   **Key configuration:**
   - Disable request timeouts for streaming endpoints (ingestion status SSE)
   - Single `Gateway` resource with `HTTPRoute` per service
   - Browse to `http://localhost` for the full UI — Envoy routes to frontend by default, API calls via `/api/*` prefix

6. **Kubernetes manifests** (`k8s/` directory, Kustomize-based):
   - PostgreSQL StatefulSet with PVC for data persistence
   - Restate StatefulSet with PVC for RocksDB state
   - `ps-migrate` Job with `resource_deps` on database (TTL cleanup after completion)
   - `ps-server` Deployment with health probes (`/health`)
   - `ps-ingestion` Deployment
   - All application container images Ubuntu-based, slimmed with Chisel
   - Resource requests/limits starting minimal for Docker Desktop dev
   - **Dev Secret** (`k8s/secrets.yaml`) — checked into the repo, dev-only:
     - `PS_SECRET_KEY` — a static 256-bit key for encrypting source credentials in the DB (e.g. a fixed base64-encoded value). Used by `ps-server` and `ps-ingestion` to encrypt/decrypt tokens stored in `config.secrets`.
     - `GITHUB_TOKEN` — placeholder value (empty or a dev PAT). Replaced by a real token via the admin UI's secret management after first deploy.
     - These manifests are dev-only — production deployments must supply real secrets via their own mechanism (sealed secrets, external secrets operator, etc.)

7. **CI pipeline:**
   - `SQLX_OFFLINE=true cargo build` for compile-time query checking without a live DB
   - `buf lint` and `buf breaking` checks on proto files
   - `cargo test` for unit and integration tests (integration tests run against a real PostgreSQL instance in CI)
   - `cargo clippy` and `cargo fmt --check`

8. **Shared domain types in `ps-core`:**
   - `Person`, `Team`, `Organisation`, `PlatformIdentity`, `TeamMembership` structs
   - `Contribution` with the typed `ContributionData` enum (initially just `PullRequest` and `CodeReview` variants)
   - `SourceConfig`, `Watermark`, `RateLimitInfo` types
   - Error types using `thiserror`
   - **Encryption module** (`ps-core/src/crypto.rs`): AES-256-GCM envelope encryption via the `aes-gcm` crate (RustCrypto). Encrypts source credentials (API tokens) before DB storage and decrypts on retrieval. Key loaded from `PS_SECRET_KEY` environment variable (base64-encoded 256-bit key). Each encrypted value stored with a unique random nonce. Used by `ConfigService` (write path) and ingestion workers (read path).

9. **Authentication layer** (see [07-authentication.md](./07-authentication.md) for full design):

   **`ps-core/src/auth/` module:**
   - Password hashing via `password-auth` crate (wraps RustCrypto `argon2`, Argon2id)
   - Session token generation: 256-bit random tokens (`rand`), base64url-encoded
   - Token hashing: SHA-256 of raw token for DB storage (raw token never persisted)
   - `AuthContext` struct (user_id, role) attached to requests by the interceptor

   **`AuthService` in `ps-server`** (proto: `proto/performance_studio/v1/auth.proto`):
   - `GetSetupStatus` — returns whether any admin user exists (public, no auth required)
   - `CompleteSetup` — creates initial admin user + session; only callable when no users exist (public)
   - `PreviewBackup` — streams a `.ps-backup` file upload, returns manifest summary (table counts, watermarks, schema version); only callable when no users exist (public)
   - `RestoreBackup` — streams a `.ps-backup` file upload, restores all state, returns session token for the restored admin user; only callable when no users exist (public)
   - `Login` — verifies credentials, creates session, returns token (public)
   - `Logout` — deletes session row (authenticated)
   - `GetCurrentUser` — returns current user info (authenticated)

   **`AdminService` in `ps-server`** (proto: `proto/performance_studio/v1/admin.proto`):
   - `CreateBackup` — assembles and streams a `.ps-backup` file (gzipped tar: manifest JSON + JSONL per table). Includes: `config.*`, `org.*` (Phase 1 tables), `activity.contributions`, `activity.ingestion_watermarks`, `activity.etag_cache`, `metrics.*`, `auth.users`. Excludes: `auth.sessions`, `activity.ingestion_runs`. Encrypted secrets exported as raw bytes (assumes same `PS_SECRET_KEY`).

   **Backup module** (`ps-core/src/backup.rs`):
   - Shared export/import logic used by both `AdminService.CreateBackup` and `AuthService.RestoreBackup`
   - Serializes/deserializes rows through the application's serde types (version-aware)
   - Manifest includes: schema version (migration number), export timestamp, table row counts, app version
   - Format: gzipped tar with manifest as first entry, then one `.jsonl` file per table
   - Restore validates manifest schema version against current migrations, then upserts rows using existing `(platform, platform_id)` and other unique constraints (idempotent)

   **Auth interceptor** (async, via `tonic-middleware`):
   - Extracts `Authorization: Bearer <token>` from request metadata
   - Validates token hash against `auth.sessions` (joined with `auth.users` for role + active status)
   - Rejects expired sessions and inactive users with `Status::unauthenticated()`
   - Attaches `AuthContext` to request extensions for downstream handlers
   - Allows public RPCs (`GetSetupStatus`, `CompleteSetup`, `PreviewBackup`, `RestoreBackup`, `Login`) without a token
   - Touches `last_active_at` on the session (fire-and-forget, non-blocking)

10. **Integration test scaffold** (see W0 deliverable 8 for full strategy):
   - `tests/integration/` directory with `[[test]]` binary target in workspace `Cargo.toml`
   - `tests/integration/common/` with test server setup (real PostgreSQL via sqlx test fixtures), test data factories, and shared helper macros
   - `define_api_test!` macro established — spins up a test server with a fresh DB, runs a closure, tears down
   - `wiremock` added as a dev-dependency for mocking external APIs
   - At minimum, a smoke test proving the test infrastructure works end-to-end before other workstreams build on it
   - **Auth integration tests:**
     - `CompleteSetup` creates admin user and returns valid session token; second call fails
     - `GetSetupStatus` returns `false` before setup, `true` after
     - `Login` with correct credentials returns token; wrong password returns `UNAUTHENTICATED`
     - Authenticated RPC with valid token succeeds; expired/invalid token returns `UNAUTHENTICATED`
     - Unauthenticated RPC (no token) to a protected endpoint returns `UNAUTHENTICATED`
     - `Logout` invalidates the session
   - **Backup/restore integration tests:**
     - `CreateBackup` produces a valid `.ps-backup` file (gzipped tar with manifest + JSONL entries)
     - `PreviewBackup` returns correct table counts and watermark info; fails if users already exist
     - `RestoreBackup` into a fresh DB restores all data and returns a valid session token; subsequent `GetSetupStatus` returns `true`
     - Round-trip: seed data → `CreateBackup` → wipe DB → run migrations → `RestoreBackup` → verify all rows restored
     - `RestoreBackup` fails if users already exist (same gate as `CompleteSetup`)

---

### W3 — Frontend Scaffolding

Set up the Next.js application, tooling, generated API clients, and component library so UI workstreams can build pages.

**Deliverables:**

1. **Next.js App Router project** in `frontend/`:
   - TypeScript strict mode (`noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`), type-checked with typescript-go
   - Bun as runtime and package manager (uses `bun.lock`, not `pnpm-lock.yaml`)
   - Tailwind CSS configured

2. **Tooling:**
   - oxlint and oxfmt configs already seeded in the repo (`.oxlintrc.json`, `.oxfmtrc.json`) — adapt the `@ctx/*` import alias references to `@ps/*` or whatever alias this project uses
   - oxlint: correctness + suspicious categories as errors, strict TypeScript rules (`no-explicit-any`, `explicit-function-return-type`, `no-unsafe-type-assertion`), no barrel files, max 1000 lines/file
   - oxfmt: 120 char print width, sorted imports with internal pattern grouping
   - Pre-commit hooks or CI checks enforcing both

3. **Component library foundation:**
   - ShadCN/ui initialised — core components added: Button, Card, Table, Badge, Dialog, Select, Tabs
   - Tremor installed for charting (used in W6)
   - Zod installed for runtime validation at system boundaries — form inputs, URL search params, environment config, sessionStorage reads. Proto-generated types cover API responses at compile time; Zod covers everything else at runtime.
   - Layout component with navigation sidebar matching the page structure from [05-frontend-strategy.md](./05-frontend-strategy.md)

4. **Connect client generation:**
   - `@connectrpc/connect-web` configured
   - `buf generate` producing TypeScript clients into `frontend/lib/api/gen/`
   - A `TransportProvider` wrapping `createConnectTransport` for the app
   - **Auth interceptor on the transport** — attaches `Authorization: Bearer <token>` from `session.ts` on every RPC; handles `UNAUTHENTICATED` responses by clearing the token and redirecting to `/login`

5. **Session and auth state management:**
   - `lib/session.ts` — token storage (in-memory variable + `sessionStorage` fallback), `setToken`/`getToken`/`clearToken` functions
   - nanostores set up for client state (selected period, active filters, auth state)
   - React Query (TanStack Query) configured with a `QueryClientProvider`
   - Pattern established: React Query hooks in `frontend/lib/hooks/` wrapping Connect client calls

6. **Auth pages** (see [07-authentication.md](./07-authentication.md)):
   - `/setup` — first-run wizard with two paths. Only accessible when `GetSetupStatus` returns `setup_complete = false`:
     - **Create admin account** — username, display name, password + confirm. Calls `AuthService.CompleteSetup`, stores returned token, redirects to dashboard.
     - **Restore from backup** — file upload for `.ps-backup` file. Streams to `AuthService.PreviewBackup` to show summary (table counts, watermarks, export date). On confirm, streams to `AuthService.RestoreBackup`, stores returned session token, redirects to dashboard with all previous state intact.
   - `/login` — username + password form. Calls `AuthService.Login`, stores token, redirects to dashboard.
   - Root layout calls `AuthService.GetSetupStatus` on load to determine routing: setup → `/setup`, no token → `/login`, valid token → app.

7. **Stub pages** for the Phase 1 views:
   - `/teams` — team comparison (implemented in W6b)
   - `/ingestion` — ingestion status (implemented in W6a)
   - `/admin` — configuration / directory import (implemented in W4)

8. **Test infrastructure** (see W0 deliverable 8 for full strategy):
   - Vitest configured with `happy-dom`, `@vitejs/plugin-react`, `vite-tsconfig-paths`
   - `@testing-library/react`, `@testing-library/dom`, `@testing-library/user-event` installed
   - `renderWithProviders` helper: wraps components with `TransportProvider` (using `createRouterTransport`) and `QueryClientProvider` (fresh client, `retry: false`)
   - Smoke test proving the test infrastructure works (render layout with mock transport, assert navigation renders)
   - `bun test` wired to `bun vitest run`; integrated into prek hooks

---

### W4 — Org Context, Directory Import & Source Configuration

Implement the organisation model (people, teams, platform identities, team memberships) and data source configuration. All operations are driven through the gRPC API and admin UI — no CLI. All RPCs require authentication (enforced by the auth interceptor from W2).

**Deliverables:**

1. **Directory file parser** in `ps-core` or `ps-server`:
   - Define a `DirectorySource` trait that abstracts over the file format — the parser produces a common `DirectoryImport` struct (people, teams, memberships, identities) regardless of the input format. This ensures the format can be swapped without touching import logic.
   - Initial implementation: parse the contristat directory file format (reference: `~/code/canonical/contristat/src/infrastructure/directory.rs`)
   - Extract people with their names, emails, levels, platform identities, and team assignments
   - Validate and report errors clearly (missing fields, duplicate identities)

2. **gRPC API for org management** (see section 5 for proto details):
   - `OrgService.ListTeams` — list teams with optional parent filter
   - `OrgService.GetTeam` — team detail with members
   - `OrgService.ListPeople` — list people with identity and team info
   - `OrgService.ImportDirectory` — trigger directory import from the admin UI (file upload)
   - Upserts people/teams/memberships/identities into the `org` schema
   - Handles temporal membership: sets `end_date` on old memberships when a person moves teams
   - Returns import summary: N people imported, N teams created, N identities mapped, N warnings

3. **gRPC API for source configuration** (see section 5 for proto details):
   - `ConfigService.ListSources` — list all configured data sources with their status (enabled/disabled)
   - `ConfigService.GetSource` — get a single source config by ID (never returns secret values — only whether a secret is set)
   - `ConfigService.CreateSource` — create a new source config (source_type, name, settings, schedule_cron)
   - `ConfigService.UpdateSource` — update an existing source config (settings, enabled, schedule)
   - `ConfigService.DeleteSource` — soft-delete or disable a source (also deletes associated secrets from `config.secrets`)
   - `ConfigService.SetSecret` — set or update an encrypted secret for a source (e.g. `github_token`). Encrypts with AES-256-GCM via `ps-core/src/crypto.rs` before storing in `config.secrets`. Accepts plaintext over the wire (gRPC is TLS-terminated at the gateway); never logs or returns the plaintext value.
   - `ConfigService.TestConnection` — validate credentials and connectivity before saving (decrypts the stored token, calls e.g. `GET /user` on GitHub API, returns success/failure + details like org name and rate limit info)
   - Validates `settings` JSONB against source-type-specific schemas before persisting

4. **GitHub source `settings` JSONB schema:**
   ```json
   {
     "orgs": ["canonical"],
     "base_url": "https://api.github.com",
     "api_mode": "rest+graphql",
     "exclude_archived": true,
     "exclude_repos": ["canonical/some-archived-repo"]
   }
   ```
   - `orgs` (required): list of GitHub organisations to ingest — repos are discovered automatically via the GitHub API (`GET /orgs/{org}/repos`), no manual repo enumeration needed
   - `base_url` (optional, default `https://api.github.com`): supports GitHub Enterprise instances
   - `api_mode` (optional, default `rest+graphql`): controls which GitHub API is used — `rest`, `graphql`, or `rest+graphql` (dual strategy as decided)
   - `exclude_archived` (optional, default `true`): skip archived repositories during discovery
   - `exclude_repos` (optional): explicit list of `org/repo` slugs to skip (e.g. forks, mirrors, irrelevant repos)
   - **Secrets** for this source type are stored encrypted in `config.secrets` (not in the `settings` JSONB). For GitHub, the required secret key is `github_token`. Set via `ConfigService.SetSecret`, encrypted at rest with AES-256-GCM, decrypted by the ingestion worker at runtime. The only env var needed is `PS_SECRET_KEY` (the encryption key itself).

5. **Admin UI page** (`/admin`) — the primary interface for org and source management:
   - **Directory import tab:** file upload (calls `OrgService.ImportDirectory`)
   - **Teams tab:** team list view showing hierarchy (org -> teams -> squads)
   - **People tab:** people list with their platform identities; unresolved identity warnings (platform usernames seen in ingestion that don't map to any person)
   - **Data Sources tab:** list configured sources with status; add/edit/disable sources; form for GitHub source with org list, schedule override, exclude patterns; password-style input for API token (calls `ConfigService.SetSecret` — shows "Token set" / "No token" indicator, never displays the stored value); "Test Connection" button to validate credentials before saving
   - **System tab:** "Download Backup" button that calls `AdminService.CreateBackup` and streams the `.ps-backup` file as a browser download. Shows current instance state summary (row counts, watermark positions, export date) before downloading. Also displays instance info: schema version, app version, uptime. **API Tokens** section: create new tokens (shows raw token once in a copy-able field with "won't be shown again" warning), list active tokens (name, created_at, last_used_at), revoke tokens. Calls `AdminService.CreateApiToken`, `ListApiTokens`, `RevokeApiToken`.

6. **Tests:**
   - Unit tests for directory file parser (inline `#[cfg(test)]` — pure parsing logic)
   - Integration tests for `OrgService` API: list/get teams, list people, import directory, team hierarchy, membership temporal handling
   - Integration tests for directory import via API: end-to-end import from fixture data, upsert idempotency, error reporting
   - Integration tests for `ConfigService` API: create/update/list/get/delete sources, settings validation, set/overwrite secrets, test connection with wiremock, verify `GetSource` never leaks secret values
   - Unit tests for encryption module: round-trip encrypt/decrypt, different nonces per encryption, wrong key fails
   - Use `define_api_test!` macro for all API tests

---

### W5 — GitHub Ingestion

Implement the GitHub source adapter and the orchestrated ingestion pipeline. This is the first real data source and proves the full ingestion architecture.

**Deliverables:**

1. **GitHub source adapter** in `ps-ingestion/src/sources/github.rs`:
   - Implements the `Source` trait from [03-data-ingestion-strategy.md](./03-data-ingestion-strategy.md)
   - Fetches pull requests and code reviews for configured repositories
   - **Dual API strategy — GraphQL primary, REST where needed:**
     - **GraphQL** for bulk data fetching: PRs with reviews, comments, and review metadata can be fetched in a single query per repository, significantly reducing call count. Use cursor-based pagination (`after` parameter) and the `since` filter on `search` or `pullRequests` connection for incremental collection.
     - **REST** for endpoints without GraphQL equivalents: Events API (change radar), and as a fallback if GraphQL rate limits (point-based, 5000 points/hour) are exhausted before REST limits (5000 requests/hour for App tokens).
     - Both APIs share the same rate limit pool for authenticated requests but are tracked separately — monitor both via `X-RateLimit-*` (REST) and `x-ratelimit-*` response headers (GraphQL).
   - Respects rate limit headers with adaptive throttling. Plan for hitting limits early — use GitHub App tokens (not PATs) for higher limits from day one.
   - **ETag conditional requests (REST):** sends `If-None-Match` on REST API calls using cached ETags from `activity.etag_cache`. On `304 Not Modified`, skips processing (and doesn't consume rate limit). On `200`, processes normally and updates the cached ETag. For paginated endpoints, a `304` on page 1 means the entire collection is unchanged — skip all subsequent pages.
   - **Change radar via Events API (REST):** at the start of each cycle, fetches `/orgs/{org}/events` (filtered to `PullRequestEvent`, `PullRequestReviewEvent`) to identify repos with recent activity. Active repos get full incremental fetches; inactive repos get ETag-only checks (expect `304`). If the events fetch fails, falls back to full incremental fetch for all repos. See [03-data-ingestion-strategy.md — Change Radar](./03-data-ingestion-strategy.md#change-radar-events-api) for details.
   - Maps GitHub data to `Contribution` records (types: `pull_request`, `code_review`)
   - Resolves GitHub usernames to `Person` via `org.platform_identities` lookup
   - Upserts contributions on `(platform, platform_id)`
   - Stores key metrics inline: `lines_added`, `lines_removed`, `time_to_merge_hours`, `reviewer_count`, `review_comment_count`
   - Reference: `~/code/canonical/contristat/src/infrastructure/sources/github/` for patterns and API usage

2. **Restate virtual object** (pattern proven in spike):
   - Virtual object keyed by source name — per-source concurrency control
   - Orchestrates the GitHub source on a configurable schedule (default: every 6 hours) via `send_after()` delayed self-invocation
   - Each step (plan, fetch_batch, store_batch, advance_watermark) as a named `ctx.run()` side effect
   - `ctx.sleep()` for durable rate limit backoff
   - Supports manual trigger and backfill via Restate HTTP ingress
   - Business logic behind `IngestionJob` trait, independent of Restate

3. **Ingestion status tracking:**
   - Writes to `activity.ingestion_watermarks` and `activity.ingestion_runs` after each run
   - Records per-run ETag hit rate (304s vs 200s) and radar summary (active vs inactive repos) in `activity.ingestion_runs` metadata
   - Reports current state, progress, and rate limit status via gRPC (consumed by the ingestion status page)

4. **Source configuration:**
   - GitHub source configured via `ConfigService` (W4) — stored in `config.source_configs` with `settings` JSONB containing target orgs, API mode, and exclusion patterns (see W4 deliverable 4 for full schema)
   - GitHub token read from `config.secrets` at runtime — decrypted using `ps-core/src/crypto.rs` with the `PS_SECRET_KEY` env var
   - Configurable per-source schedule override
   - Repo discovery: the ingestion worker reads `settings.orgs` and discovers repos automatically via `GET /orgs/{org}/repos`, filtered by `exclude_archived` and `exclude_repos`

5. **gRPC API for ingestion** (see section 5):
   - `IngestionService.GetStatus` — current state per source
   - `IngestionService.ListRuns` — historical run log
   - `IngestionService.TriggerRun` — manually trigger a source run
   - `IngestionService.TriggerBackfill` — start a backfill from a given date

6. **Tests:**
   - Integration tests for the GitHub source adapter using `wiremock` — mock GitHub API responses (PRs, reviews, pagination, rate limit headers), assert correct `Contribution` records stored in DB
   - Use `define_source_test!` macro: sets up wiremock fixtures, runs ingestion, verifies contributions and watermark state
   - Test incremental collection: second run with advanced watermark should only fetch new data
   - Test identity resolution: contributions linked to known people, `person_id = NULL` for unknown usernames
   - Test upsert behaviour: re-ingesting the same PR updates rather than duplicates
   - Test ETag caching: first fetch returns `200` with `ETag` header → ETag stored in `activity.etag_cache`; second fetch sends `If-None-Match` → wiremock returns `304` → no contributions re-processed, no rate limit consumed
   - Test ETag cache miss: wiremock returns `200` (content changed) → contributions updated normally, ETag refreshed
   - Test change radar: mock `/orgs/{org}/events` returning `PullRequestEvent` for repo A → repo A gets full fetch, repo B gets ETag-only check
   - Test radar fallback: events endpoint returns error → all repos fall back to full incremental fetch with ETags
   - Integration tests for `IngestionService` API: trigger run, get status, list runs

---

### W6a — Ingestion Status UI

Build the ingestion status page so that W5's pipeline can be monitored, triggered, and debugged through the UI. This workstream runs **in parallel with W5** — it depends only on the `IngestionService` gRPC API (defined in W5) and the frontend scaffolding (W3). It does **not** depend on ingested data existing, since it reads from `activity.ingestion_runs` and `activity.ingestion_watermarks` which are populated as soon as the first run executes.

**Why this is separate from W6b:** Without this page, the only way to trigger an ingestion run or see its status after W5 is complete would be via the Restate HTTP ingress directly — which contradicts the "no CLI, all through the UI" decision. Splitting it out ensures W5 is manually testable through the UI the moment the pipeline works.

**Deliverables:**

1. **Ingestion status page** (`/ingestion`):
   - Per-source status cards: source name, last run time, next scheduled run, current state (idle/collecting/waiting/error), items collected
   - Rate limit visibility: if waiting, show remaining wait time. ETag hit rate for last run (e.g. "82% of requests returned 304").
   - Historical run log table: start time, duration, items collected, status, error message, ETag hit rate, radar active/inactive repo counts
   - Manual trigger buttons: "Run Now" and "Backfill" per source
   - Auto-refresh via React Query polling (short interval when a run is active)

2. **Tests:**
   - Frontend tests: render ingestion status page with mock transport, assert status cards render, trigger buttons call correct RPCs, polling behaviour works
   - Integration tests for `IngestionService` API (may overlap with W5 tests): get status, list runs, trigger run

---

### W6b — Metrics Computation & Team UI

Compute team-level PR metrics from ingested GitHub data and display them in the frontend. This workstream depends on W5 having ingested data.

**Deliverables:**

1. **Metrics computation** in `ps-metrics`:
   - **PR throughput** — number of PRs merged per team per period (week/month/quarter)
   - **Review turnaround** — average time from PR opened to first review, per team per period
   - Computed from `activity.contributions` joined with `org.team_memberships` and `org.platform_identities`
   - Results written to `metrics.team_snapshots` with `period_type`, `period_start`, `period_end`
   - Computation triggered after each ingestion run completes (via Restate workflow step or separate scheduled task)
   - Supports re-computation for historical periods

2. **gRPC API for metrics** (see section 5):
   - `MetricsService.GetTeamMetrics` — metrics for a single team and period
   - `MetricsService.CompareTeams` — metrics for multiple teams side by side
   - `MetricsService.ListPeriods` — available periods with data

3. **Team comparison page** (`/teams`):
   - Table view of teams with columns: team name, PR throughput, avg review turnaround, member count
   - Period selector (week, month, quarter) using nanostores for state
   - Sortable columns
   - Drill-down link to team detail (stub page — full detail view is Phase 2)
   - Tremor bar chart comparing selected metric across teams
   - Data fetched via React Query hooks wrapping Connect client

4. **Tests:**
   - Integration tests for metrics computation using `define_metric_test!` macro: seed known contributions, run computation, assert snapshot values for throughput and review turnaround
   - Test period boundaries: contributions at week/month edges attributed correctly
   - Test team rollup: contributions from squad members roll up to parent team metrics
   - Integration tests for `MetricsService` API: get team metrics, compare teams, list periods
   - Test empty state: no contributions produces zero/null metrics, not errors

---

## 3. Dependencies Between Workstreams

```
W0 (Tooling) ──┬── W1 (Spike) ✅ COMPLETE
               │
               ├── W2 (Backend Scaffolding) ──┬──────────┐
               │                              │          │
               │                              │  W4 (Org)┤
               │                              │          │
               │                              │  W5 (GitHub) ──── W6b (Metrics + Team UI)
               │                              │          │
               └── W3 (Frontend Scaffolding) ─┘          │
                                              │          │
                                              ├─ W4 (Admin UI)
                                              ├─ W6a (Ingestion Status UI) ← parallel with W5
                                              └─ W6b (Team comparison UI)
```

| Dependency                                                                                  | Detail                                                                                                                            |
| ------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| W0 must complete before W2, W3 start                                                        | All workstreams depend on the Nix devshell, clippy config, and build tooling being in place.                                      |
| ~~W1 must complete before W5 starts~~                                                       | **RESOLVED** — spike complete, Restate confirmed. W5 can proceed immediately using proven patterns.                               |
| W2 must reach "proto + migrations + crate skeleton" before W4, W5, W6a, W6b                 | Other workstreams need the database schema, generated proto code, and shared domain types.                                        |
| W3 must reach "Connect client + layout + component lib" before W4 admin UI, W6a, and W6b UI | Frontend workstreams need generated clients and the component library.                                                            |
| W4 must have people/teams imported before W5 can resolve identities                         | GitHub ingestion maps usernames to people via `org.platform_identities`. Without org data, contributions have `person_id = NULL`. |
| W4 source config must exist before W5 can run                                               | The ingestion worker reads `config.source_configs` to know which GitHub orgs to scrape. At minimum, the `ConfigService` API and a configured source must be in place before W5's pipeline can execute end-to-end. |
| W5 `IngestionService` proto must exist before W6a can build the UI                          | W6a consumes the `IngestionService` gRPC API. The proto and backend RPCs must be defined (can stub responses initially). W6a does **not** need ingested data — it works as soon as the API exists. |
| W5 must have ingested data before W6b can compute metrics                                   | Metrics are derived from `activity.contributions`.                                                                                |

### Parallel work opportunities

- **W0** is the very first task — short (2-3 days) and unblocks everything else.
- **W2 + W3** can start once W0 is complete. They have no mutual dependency for their initial scaffolding.
- **W1 is complete** — Restate confirmed. Its patterns feed directly into W5.
- **W4** can start as soon as W2 has the `org` schema migrations and `ps-core` domain types. W4's directory parser doesn't need proto generation.
- **W5** can begin immediately once W2 has the crate skeleton — the spike is done and Restate patterns are proven. No orchestrator uncertainty remains.
- **W6a** runs in parallel with W5 — it only needs the `IngestionService` proto and frontend scaffolding. This ensures the ingestion pipeline is manually testable through the UI as soon as W5's pipeline works, rather than waiting for W6b.
- **W6b metrics computation** can begin once W5 has produced sample data. **W6b team comparison UI** can begin once W3 has the layout and component library, using mock data while waiting for the real API.

---

## 4. Database Schemas Needed in Phase 1

Phase 1 requires a subset of the tables from [04-database-design.md](./04-database-design.md). The `reasoning` schema is not needed until Phase 3. The `auth` schema is needed from day one.

### `config` schema — full

| Table                    | Needed for                                        |
| ------------------------ | ------------------------------------------------- |
| `config.source_configs`  | GitHub source configuration                       |
| `config.secrets`         | Encrypted API tokens per source (AES-256-GCM)     |
| `config.global_settings` | Default schedule, system-wide settings             |

### `org` schema — partial

| Table                       | Needed for                           | Notes                          |
| --------------------------- | ------------------------------------ | ------------------------------ |
| `org.people`                | Directory import                     | Full table as designed         |
| `org.platform_identities`   | Identity resolution during ingestion | Full table as designed         |
| `org.teams`                 | Team hierarchy                       | Full table as designed         |
| `org.team_memberships`      | Temporal team membership             | Full table as designed         |
| `org.repositories`          | Linking PRs to repos and teams       | Full table as designed         |
| ~~`org.repo_scans`~~        | Not needed                           | Phase 3+ (repository analysis) |
| ~~`org.repo_scan_results`~~ | Not needed                           | Phase 3+ (repository analysis) |

### `activity` schema — full

| Table                           | Needed for                                                         |
| ------------------------------- | ------------------------------------------------------------------ |
| `activity.contributions`        | Storing GitHub PRs and code reviews                                |
| `activity.ingestion_watermarks` | Tracking GitHub source cursor                                      |
| `activity.ingestion_runs`       | Ingestion history for the status page                              |
| `activity.etag_cache`           | Caching ETags for conditional requests (reduces rate limit usage)  |

### `metrics` schema — partial

| Table                             | Needed for                | Notes                                                                                                                   |
| --------------------------------- | ------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `metrics.team_snapshots`          | Pre-computed team metrics | Phase 1 uses only: `throughput`, `avg_review_turnaround_hours`, `raw_metrics`. Other DORA/flow columns remain nullable. |
| ~~`metrics.individual_profiles`~~ | Not needed                | Phase 2 (individual profile view)                                                                                       |
| ~~`metrics.snapshot_sources`~~    | Not needed                | Phase 2+ (traceability link table)                                                                                      |

### `reasoning` schema — not needed

Skipped entirely in Phase 1. No embeddings, enrichments, or AI insights.

### `auth` schema — full

| Table            | Needed for                              |
| ---------------- | --------------------------------------- |
| `auth.users`     | Admin user, first-run wizard, login     |
| `auth.sessions`  | Session token validation, auth interceptor |

### Migration file plan

```
migrations/
├── 0001_create_config_schema.sql      # config.source_configs, config.secrets, config.global_settings
├── 0002_create_org_schema.sql         # org.people, platform_identities, teams, team_memberships, repositories
├── 0003_create_activity_schema.sql    # activity.contributions, ingestion_watermarks, ingestion_runs, etag_cache
├── 0004_create_metrics_schema.sql     # metrics.team_snapshots
├── 0005_create_auth_schema.sql        # auth.users, auth.sessions (after org schema for person_id FK)
```

The `reasoning` schema and `metrics.individual_profiles` / `metrics.snapshot_sources` are added in later phase migrations. The `auth` schema references `org.people` so it must come after the `org` migration.

---

## 5. Proto Definitions Needed in Phase 1

Proto files live in `proto/` and are managed by `buf`. Phase 1 needs six services.

### `proto/performance_studio/v1/auth.proto`

```protobuf
service AuthService {
  rpc GetSetupStatus(GetSetupStatusRequest) returns (GetSetupStatusResponse);
  rpc CompleteSetup(CompleteSetupRequest) returns (CompleteSetupResponse);
  rpc PreviewBackup(stream PreviewBackupRequest) returns (PreviewBackupResponse);
  rpc RestoreBackup(stream RestoreBackupRequest) returns (RestoreBackupResponse);
  rpc Login(LoginRequest) returns (LoginResponse);
  rpc Logout(LogoutRequest) returns (LogoutResponse);
  rpc GetCurrentUser(GetCurrentUserRequest) returns (GetCurrentUserResponse);
}
```

**Key messages:** `GetSetupStatusResponse` (setup_complete bool), `CompleteSetupRequest` (username, display_name, password), `PreviewBackupRequest` (chunk bytes — streamed upload), `PreviewBackupResponse` (schema_version, exported_at, table_counts map, source_names, watermarks map), `RestoreBackupRequest` (chunk bytes — streamed upload), `RestoreBackupResponse` (session_token, expires_at, tables_restored map), `LoginRequest` (username, password), `LoginResponse` (session_token, expires_at), `GetCurrentUserResponse` (user_id, username, display_name, role). See [07-authentication.md](./07-authentication.md) for full message definitions.

### `proto/performance_studio/v1/admin.proto`

```protobuf
service AdminService {
  // Stream a full state backup to the client (authenticated)
  rpc CreateBackup(CreateBackupRequest) returns (stream CreateBackupResponse);

  // Generate a long-lived API token for CLI access (authenticated)
  rpc CreateApiToken(CreateApiTokenRequest) returns (CreateApiTokenResponse);

  // List active API tokens (authenticated)
  rpc ListApiTokens(ListApiTokensRequest) returns (ListApiTokensResponse);

  // Revoke an API token (authenticated)
  rpc RevokeApiToken(RevokeApiTokenRequest) returns (RevokeApiTokenResponse);
}
```

**Key messages:** `CreateBackupRequest` (empty), `CreateBackupResponse` (chunk bytes — streamed download), `CreateApiTokenRequest` (name/label for the token), `CreateApiTokenResponse` (token string — shown once, never retrievable again), `ListApiTokensResponse` (list of token metadata: id, name, created_at, last_used_at — never the token value), `RevokeApiTokenRequest` (token_id).

### `proto/performance_studio/v1/org.proto`

```protobuf
service OrgService {
  rpc ListTeams(ListTeamsRequest) returns (ListTeamsResponse);
  rpc GetTeam(GetTeamRequest) returns (GetTeamResponse);
  rpc ListPeople(ListPeopleRequest) returns (ListPeopleResponse);
  rpc ImportDirectory(ImportDirectoryRequest) returns (ImportDirectoryResponse);
}
```

**Key messages:** `Team` (id, name, org_name, parent_team_id, lead, github_team_slug, member_count), `Person` (id, name, email, level, identities), `PlatformIdentity` (platform, username), `TeamMembership` (person, start_date, end_date).

### `proto/performance_studio/v1/config.proto`

```protobuf
service ConfigService {
  rpc ListSources(ListSourcesRequest) returns (ListSourcesResponse);
  rpc GetSource(GetSourceRequest) returns (GetSourceResponse);
  rpc CreateSource(CreateSourceRequest) returns (CreateSourceResponse);
  rpc UpdateSource(UpdateSourceRequest) returns (UpdateSourceResponse);
  rpc DeleteSource(DeleteSourceRequest) returns (DeleteSourceResponse);
  rpc SetSecret(SetSecretRequest) returns (SetSecretResponse);
  rpc TestConnection(TestConnectionRequest) returns (TestConnectionResponse);
}
```

**Key messages:** `SourceConfig` (id, source_type, name, enabled, settings as google.protobuf.Struct, secret_status as map<string, bool> indicating which secrets are set without exposing values, schedule_cron, created_at, updated_at), `CreateSourceRequest` (source_type, name, settings, schedule_cron), `UpdateSourceRequest` (id, enabled, settings, schedule_cron — uses field masks for partial updates), `SetSecretRequest` (source_id, secret_key e.g. "github_token", secret_value — plaintext, encrypted server-side before storage), `SetSecretResponse` (success), `TestConnectionResponse` (success, error_message, details e.g. org name, rate limit info).

### `proto/performance_studio/v1/ingestion.proto`

```protobuf
service IngestionService {
  rpc GetStatus(GetStatusRequest) returns (GetStatusResponse);
  rpc ListRuns(ListRunsRequest) returns (ListRunsResponse);
  rpc TriggerRun(TriggerRunRequest) returns (TriggerRunResponse);
  rpc TriggerBackfill(TriggerBackfillRequest) returns (TriggerBackfillResponse);
}
```

**Key messages:** `SourceStatus` (name, source_type, state enum, last_run, next_run, items_collected, rate_limit_info), `IngestionRun` (id, source_name, started_at, completed_at, status, items_collected, error_message, rate_limit_waits_seconds).

### `proto/performance_studio/v1/metrics.proto`

```protobuf
service MetricsService {
  rpc GetTeamMetrics(GetTeamMetricsRequest) returns (GetTeamMetricsResponse);
  rpc CompareTeams(CompareTeamsRequest) returns (CompareTeamsResponse);
  rpc ListPeriods(ListPeriodsRequest) returns (ListPeriodsResponse);
}
```

**Key messages:** `TeamMetrics` (team_id, period, throughput, avg_review_turnaround_hours, raw_metrics), `Period` (type enum: WEEK/MONTH/QUARTER, start, end), `TeamComparison` (list of TeamMetrics).

### Buf configuration

`buf.yaml` at project root with lint rules enabled. `buf.gen.yaml` generating:

- Rust prost types into `crates/ps-proto/src/gen/`
- TypeScript Connect clients into `frontend/lib/api/gen/`

---

## 6. Key Decisions and Risks

### Decisions to make during Phase 1

| Decision                                | Context                                                                          | Resolution                                                                                                                                                                                                                              |
| --------------------------------------- | -------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Directory file format**               | Contristat uses a specific format. Should we adopt it as-is or define a new one? | **DECIDED** — Start with the contristat format for compatibility, but abstract behind a `DirectorySource` trait so the format can change without touching import logic. Reference `~/code/canonical/contristat/src/infrastructure/directory.rs` for the initial parser. |
| **Metrics computation trigger**         | Compute on a schedule, after each ingestion run, or on demand?                   | **DECIDED** — After each ingestion run (as a follow-up step in the Restate workflow). Also support on-demand re-computation for historical periods.                                                                                     |
| **GitHub API: REST vs GraphQL**         | REST is simpler; GraphQL can fetch PRs + reviews in fewer calls.                 | **DECIDED** — Support both REST and GraphQL, using each where it's strongest. GraphQL is preferred where it can reduce call count (e.g. fetching PRs with reviews in a single query). REST is used where GraphQL coverage is weaker or for endpoints that have no GraphQL equivalent (e.g. Events API for change radar). Plan for hitting rate limits early and often — use GitHub App tokens, aggressive ETag caching, and change radar to minimise API calls. |
| **How to handle Restate communication** | The API server needs to trigger ingestion runs and read status.                  | **DECIDED** — The API server calls Restate's HTTP invocation API to trigger runs. Ingestion status is read from PostgreSQL (`ingestion_watermarks`, `ingestion_runs`), not from Restate state — keeps the API server decoupled from the orchestrator. |
| **CLI vs UI for operations**            | Should there be a CLI for bootstrap/import, or should everything go through the UI? | **DECIDED** — Primary operations through UI and gRPC API. A lightweight CLI tool (`psctl`) is also provided as a thin client over the same API — useful for scripting, dev workflows, and quick status checks. `psctl` authenticates via API tokens generated in the admin UI. See [psctl design](#psctl--lightweight-cli-client) below. |
| **Source credential storage**           | Store tokens as env var references, or encrypted in the DB? | **DECIDED** — Encrypted in DB. Source credentials (API tokens) stored in `config.secrets` table, encrypted with AES-256-GCM (`aes-gcm` crate). Only `PS_SECRET_KEY` (the encryption key) is supplied via env var / K8s Secret. All other credentials managed through the admin UI via `ConfigService.SetSecret`. This aligns with the "no CLI" decision — admins don't need shell access to configure sources. Dev K8s manifests include a static `PS_SECRET_KEY` for local development. |
| **State backup/restore**                | During dev, the k8s cluster may be destroyed. Re-fetching data from APIs is slow and wasteful. | **DECIDED** — UI-driven backup/restore. Export via "Download Backup" in admin UI (`AdminService.CreateBackup`). Restore via the first-run setup wizard (`AuthService.RestoreBackup`). Format: gzipped tar with JSON manifest + JSONL per table. Assumes same `PS_SECRET_KEY` across instances (encrypted secrets exported as raw bytes). Also available via `psctl backup` / `psctl restore`. See [07-authentication.md — Backup & Restore](./07-authentication.md#backup--restore). |
| **API tokens for CLI/automation**       | `psctl` and potential CI integrations need non-interactive authentication. | **DECIDED** — Long-lived API tokens generated via admin UI (`AdminService.CreateApiToken`). Stored in `auth.sessions` with `session_type = 'api_token'` and no expiry. Token shown once at creation, never retrievable again. `psctl` reads token from `PS_API_TOKEN` env var. Same Bearer auth mechanism as browser sessions — no new auth path needed. |

### Risks

| Risk                                               | Likelihood | Impact | Mitigation                                                                                                                                                                                            |
| -------------------------------------------------- | ---------- | ------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ~~**Spike takes longer than 8 days**~~             | —          | —      | **RESOLVED** — spike complete, Restate confirmed.                                                                                                                                                     |
| ~~**Restate Rust SDK has gaps for our use case**~~ | —          | —      | **RESOLVED** — spike confirmed Restate SDK (0.9) is suitable. All required primitives (virtual objects, durable side effects, durable sleep) work as needed.                                          |
| **Restate pre-1.0 breaking changes**               | Low-Medium | Medium | SDK is at 0.9, approaching stability. Core API surface (objects, `ctx.run()`, `ctx.sleep()`) is stable. The `IngestionJob` trait abstraction limits blast radius if the Restate API changes.          |
| **GitHub rate limits slow initial backfill**       | Medium     | Medium | Plan for hitting limits frequently. Use a GitHub App token (not PAT) for higher limits from day one. GraphQL reduces call count significantly (PRs + reviews in one query). ETag conditional requests on REST eliminate cost for unchanged data. Change radar (Events API) identifies active repos. Dual-API strategy provides fallback — if GraphQL points are exhausted, REST calls may still be available. A 6-month backfill for a few dozen repos will span multiple runs. |
| **Identity resolution has many unresolved users**  | Medium     | Medium | Surface unresolved identities prominently in the admin UI. Allow manual mapping. Contributions with `person_id = NULL` are still stored and counted in org-level metrics — they aren't lost.          |
| **sqlx offline mode/query cache causes friction**  | Low        | Low    | Establish the `cargo sqlx prepare` workflow in CI from the start. Document it clearly.                                                                                                                |
| **Proto schema changes break frontend/backend**    | Low        | Medium | `buf breaking --against .git#branch=main` in CI catches this automatically. Establish the discipline early.                                                                                           |

---

## 7. `psctl` — Lightweight CLI Client

A thin CLI tool that talks to the same gRPC API as the frontend. It does not embed business logic — all operations are delegated to `ps-server` via Connect/gRPC calls. Useful for scripting, dev workflows, and quick status checks without opening the browser.

### Design principles

- **Separate binary, shared crates** — `psctl` is its own crate in the workspace (`crates/psctl/`), depending on `ps-proto` for generated types. It does **not** depend on `ps-core`, `ps-server`, or `ps-ingestion` — it's a pure API client.
- **Thin client** — every command maps to one or more gRPC calls. No database access, no direct k8s interaction, no embedded business logic.
- **Auth via API token** — reads `PS_API_TOKEN` from environment (or `--token` flag). API tokens are generated in the admin UI via `AdminService.CreateApiToken`. For unauthenticated operations (restore into a fresh instance), no token needed.
- **Server address** — reads `PS_SERVER_URL` from environment (or `--server` flag). Defaults to `http://localhost:18080` for local dev.

### Phase 1 commands

```
psctl status                  # Ingestion watermarks, row counts, source states
                              # → calls IngestionService.GetStatus + summary queries

psctl backup [--output FILE]  # Download a .ps-backup file
                              # → calls AdminService.CreateBackup, streams to file

psctl restore FILE            # Restore into a fresh instance (no auth needed)
                              # → calls AuthService.PreviewBackup (shows summary)
                              # → on confirm, calls AuthService.RestoreBackup

psctl trigger SOURCE          # Manually trigger an ingestion run
                              # → calls IngestionService.TriggerRun

psctl backfill SOURCE --since DATE  # Trigger a backfill
                              # → calls IngestionService.TriggerBackfill

psctl runs [SOURCE]           # List recent ingestion runs
                              # → calls IngestionService.ListRuns

psctl sources                 # List configured data sources
                              # → calls ConfigService.ListSources
```

### Future phase commands (not implemented in Phase 1, but the shape is clear)

```
# Phase 2
psctl people [--unresolved]   # List people, flag unresolved identities
psctl metrics TEAM --period MONTH  # Quick metrics lookup

# Phase 3
psctl enrich --type sentiment --since DATE  # Trigger re-enrichment
psctl cost-report [--since DATE]            # AI API spend summary

# Phase 4
psctl insights TEAM --period MONTH  # View generated insights
```

### Implementation notes

- Built with `clap` for argument parsing, `tonic` or `reqwest` (Connect HTTP) for API calls
- Included in the Nix flake devshell so it's available during development
- Added to the workspace `Cargo.toml` but **not** packaged in the production container images — it's a developer/operator tool, not a runtime service
- Included in the Tiltfile for automatic rebuild on changes (binary synced to dev container or run on host)

### Dependency

`psctl` depends on W2 (proto definitions + generated types) and can be built incrementally as API endpoints land. The `status`, `backup`, and `restore` commands are the initial priority.

---

## 8. Spike Outcome

The [Restate vs Temporal spike](./09-spike-restate-vs-temporal.md) is **complete**. See [evaluation.md](~/code/canonical/temporal-restate-spike/evaluation.md) for full results. **Restate is confirmed** as the orchestration engine (scored 3.9 vs Temporal's 3.1).

This removes what was the primary dependency risk in Phase 1 — W5 (GitHub Ingestion) can proceed immediately with the Restate integration rather than waiting for a spike to conclude. The proven patterns (virtual objects, `ctx.run()`, `ctx.sleep()`, `IngestionJob` trait) are documented in W1 and W5 above.

### Updated sequencing

1. **Week 1 (days 1-3):** W0 (project tooling) — Nix flake, devshell, treefmt, prek, clippy config, CLAUDE.md, `.cargo/config.toml` with clang + mold.
2. **Week 1-2:** Start W2 (backend scaffolding) and W3 (frontend scaffolding) in parallel once W0 is done.
3. **Week 2-3:** W4 (org context + source config) starts once W2 has migrations and domain types. `ConfigService` API lands in week 2 so W5 can configure its GitHub source. W5 starts with GitHub API client + Restate integration. **W6a starts in parallel with W5** — builds the ingestion status page against the `IngestionService` proto so the pipeline is testable through the UI immediately.
4. **Week 3-4:** W5 full pipeline running. W6a complete — you can now trigger runs, watch progress, and see history in the UI. W6b begins metrics computation.
5. **Week 4-5:** W6b team comparison UI built against real data. Integration testing. Polish.
6. **Week 5-6:** End-to-end testing on Canonical K8s. Bug fixes. Exit criteria validation.

---

## Summary Timeline

| Week | W0 Tooling           | W1 Spike    | W2 Backend                                 | W3 Frontend                | W4 Org                          | W5 GitHub                     | W6a Ingestion UI       | W6b Metrics + Team UI |
| ---- | -------------------- | ----------- | ------------------------------------------ | -------------------------- | ------------------------------- | ----------------------------- | ---------------------- | --------------------- |
| 1    | Nix, prek, CLAUDE.md | ✅ COMPLETE | Workspace, migrations, proto               | Next.js, tooling, ShadCN   |                                 |                               |                        |                       |
| 2    |                      |             | Domain types, CI, k8s                      | Connect gen, layout, hooks | Parser, org API, ConfigService  | API client + Restate scaffold | Status page scaffold   |                       |
| 3    |                      |             | Stable                                     | Stable                     | Import + source config admin UI | Full pipeline running         | Trigger + run history  | Computation logic     |
| 4    |                      |             |                                            |                            | Polish                          | Polish + backfill             | ✅ Complete            | Team comparison UI    |
| 5    |                      |             |                                            |                            |                                 |                               |                        | Polish                |
| 6    |                      |             | Integration testing across all workstreams |                            |                                 |                               |                        | Exit criteria         |

**Total estimate:** ~5-6 weeks with one developer, shorter with parallel contributors on frontend vs backend. W0 overlaps with week 1 (first 2-3 days). W1 (spike) is already complete, removing the primary schedule risk.
