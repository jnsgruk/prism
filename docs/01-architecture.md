# Architecture

## System Components

```
                                  ┌─────────────┐
                                  │   Frontend   │
                                  │  (Vite SPA)  │
                                  └──────┬───────┘
                                         │ Connect (gRPC-Web)
                                  ┌──────▼───────┐
                                  │Envoy Gateway │
                                  └──────┬───────┘
                                         │
                          ┌──────────────▼──────────────┐
                          │         ps-server            │
                          │  (gRPC API + auth interceptor)│
                          └──┬────────────────────┬──────┘
                             │                    │ fire-and-forget
                     ┌───────▼───────┐    ┌──────▼───────┐
                     │  PostgreSQL   │    │   Restate    │
                     │  + pgvector   │    │ (orchestrator)│
                     └───────────────┘    └──────┬───────┘
                                                 │
                                          ┌──────▼───────┐
                                          │  ps-workers  │
                                          │  (handlers)  │
                                          └──────┬───────┘
                                                 │
                                    ┌────────────┼────────────┐
                                    │            │            │
                              ┌─────▼──┐  ┌─────▼──┐  ┌─────▼──┐
                              │ GitHub │  │  Jira  │  │Discourse│
                              └────────┘  └────────┘  └────────┘
```

**Request flow:** Frontend sends Connect (gRPC-Web) requests through Envoy Gateway to ps-server. ps-server handles queries synchronously via the repository layer. For long-running work (ingestion, metrics computation, AI enrichment), ps-server fires requests to Restate, which orchestrates ps-workers handlers durably.

**Agentic query flow:** ps-server triggers Restate to prepare an OpenCode pod (ps-agent manages K8s lifecycle), then streams SSE events directly from the pod to the gRPC client. The pod runs ps-mcp as an MCP server providing Prism data tools.

## Crate Roles and Boundaries

### ps-core — Domain Foundation

Domain types, models, enums, repository layer (all DB access), auth (password/token/session), crypto (AES-256-GCM), backup/restore. The shared foundation crate.

**Belongs here:** domain models, repo methods, traits like `Source`, error types, anything shared across ps-server and ps-workers.
**Does NOT belong here:** proto types, transport concerns, handler logic, provider-specific API clients.

### ps-proto — Generated Code

Generated Rust code from protobuf definitions. Pedantic lints disabled.

**Never hand-edit.** Regenerate with `buf generate`.

### ps-server — API Server

gRPC API binary (tonic + tonic-web). Thin service adapters that map between proto types and domain types. Auth interceptor with allow-list for public RPCs.

**Belongs here:** gRPC service implementations, proto <-> domain conversions, auth middleware.
**Does NOT belong here:** direct SQL, business logic that doesn't need proto types (put it in ps-core), long-running work (use Restate).

### ps-workers — Restate Handlers

Restate worker binary. Ingestion handlers (GitHub, Jira, Discourse), team sync, metrics computation, AI pipeline (enrichment, embedding, insights, model catalogue, agent reaper). `infra/` holds service plumbing (SharedState, journaling macros, retry, registry, secrets).

**Belongs here:** handler implementations, source adapters, Restate-specific orchestration logic.
**Does NOT belong here:** gRPC service definitions, direct SQL (use repos via SharedState).

### ps-metrics — Metric Computation

Pure metric computation logic (DORA, flow metrics, individual stats). Depends only on ps-core.

**Belongs here:** metric calculations, period logic, snapshot builders.
**Does NOT belong here:** database access, handler logic.

### ps-reasoning — AI Abstraction

LLM abstraction via Rig framework. TaskRouter for model routing, model catalogue, cost tracking. Features for enrichment, embeddings, insights.

**Belongs here:** Rig model wrappers, structured extraction, embedding logic, prompt templates.
**Does NOT belong here:** Restate handler logic (that's in ps-workers), pod lifecycle (that's in ps-agent).

### ps-agent — Agent Lifecycle

Agent container lifecycle management. K8s pod spec construction, container manager (create/delete pods via K8s API), OpenCode SSE event mapper.

**Belongs here:** pod specs, K8s API calls, SSE event parsing.
**Does NOT belong here:** MCP tool implementations (that's ps-mcp), conversation state (that's ps-core/repo).

### ps-mcp — MCP Server

MCP stdio server binary running inside agent containers. Provides Prism data tools (query metrics, search contributions) and S3 artifact tools (upload files to RustFS).

**Belongs here:** MCP tool definitions, data query handlers, S3 upload logic.
**Does NOT belong here:** pod lifecycle, LLM calls.

### ps-migrate — Migration Runner

Migration binary for K8s init container. Runs sqlx migrations against PostgreSQL.

**The app binary never runs migrations.** Only ps-migrate does.

### psctl — CLI Client

Lightweight CLI for administrative operations. Depends only on ps-proto.

**Belongs here:** CLI commands, output formatting.
**Does NOT belong here:** domain logic, database access.

### Dependency DAG

```
psctl ──────────────────────────► ps-proto
ps-server ──► ps-core, ps-proto, ps-metrics, ps-agent
ps-workers ─► ps-core, ps-proto, ps-metrics, ps-agent
ps-metrics ─► ps-core
ps-reasoning ► ps-core
ps-agent ───► ps-core (+ kube)
ps-mcp ────► ps-core, ps-proto
```

## Code Organisation Principles

Code is organised **feature-first, layer-second**. The primary organising unit is a domain feature (e.g. sources, teams, ingestion). Within a feature, code is subdivided by concern — handlers, services, components, hooks.

This applies across the entire codebase in both Rust and TypeScript. A developer looking for "teams" code should find one directory, not a scattering across layer buckets.

### Why not layer-first?

Layer-first fragments features as the codebase grows. A sources change touches four directories (handlers, services, repo, models). Feature-first keeps everything together — a new developer can understand or delete a feature by looking in one place.

### Three-Tier Escalation

Every piece of code has a default home. Start at the lowest tier and only escalate when a concrete second consumer exists.

1. **Feature-local** (default): `src/features/<name>/` (Rust) or `views/<name>/` (TypeScript)
2. **Service/app-local** (a second feature in the same service needs it): top-level modules, `components/`, `lib/hooks/`
3. **Shared crate/package** (a second service needs it): `ps-core` (Rust), `@ps/shared` (TypeScript)

The signal to lift is always a concrete second consumer — not speculative future use.

### Module Size Tiers

| Tier | Size | Structure |
| --- | --- | --- |
| 1 — Small | < ~150 LOC | Single file or handful of colocated files, no subdirectories |
| 2 — Medium | 150–500 LOC | Module file as pure declaration + re-export, logic in named siblings |
| 3 — Large | 500+ LOC | Nested subdirectories for complex concerns |

### Naming Conventions

The module path provides context — don't prefix filenames with the feature name (`sources/repository.rs` not `sources/source_repository.rs`).

**Rust:** snake_case files, `mod.rs` for module entry, `//!` doc comments at top of handler files.

**TypeScript:** kebab-case files, `use-` prefix for hooks, `-page` suffix for pages, `.test.ts` for tests.

**Standard subdirectories:** Rust — flat sibling files (`handler.rs`, `service.rs`, `repository.rs`, `types.rs`). TypeScript — `components/`, `hooks/`, `lib/`, `pages/`, `types/`.

### The Role of `mod.rs` / `index.ts`

Both serve the same purpose: declare submodules and define the feature's public API surface. They never contain business logic. Tier 1 features may have all logic in `mod.rs` until they grow.

### Export Strategy

**Rust:** `pub use` in `mod.rs` defines the public API. External consumers import through the module path.

**TypeScript:** View-level `index.ts` for re-exports at Tier 2+. Subpath exports in `package.json` for library packages (never barrel files). Never re-export just to shorten an import path.

### Structural Invariants

- A change to a feature's business logic should touch at most two top-level directories
- Deleting a feature directory should remove > 80% of that feature's code
- No cross-feature imports that bypass `mod.rs` or `index.ts`
- No `utils/` or `helpers/` directories — give utilities a proper home

## Security Model

- **Fail-closed auth** — missing auth header returns an error, never silently forwards. The interceptor rejects non-public RPCs without auth.
- **Admin role enforcement** — privileged operations (reset, backup, token management) require `require_admin()`.
- **Error masking** — full database errors logged server-side with `tracing`, generic "internal error" returned to clients.
- **Encrypted secrets** — AES-256-GCM for source credentials at rest. Never decrypt inside Restate `ctx.run()` (journal persists side-effect results).
- **Input validation** — escape LIKE patterns, validate external identifiers before interpolation.
