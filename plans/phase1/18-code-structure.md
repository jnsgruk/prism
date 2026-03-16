# Code Structural Strategy

## 1. Guiding Principle

Code is organised **feature-first, layer-second**. The primary organising unit is a domain feature (e.g. `sources`, `teams`, `ingestion`). Within a feature, code is subdivided by concern — handlers, services, components, hooks. Transport protocols (gRPC) and UI framework layers (pages, components) are dimensions _within_ a feature, not containers for features.

This philosophy applies across the entire codebase regardless of language. Rust and TypeScript differ in idioms and tooling, but the structural intent is the same: a developer looking for "teams" code should find one directory, not a scattering across transport or layer buckets.

### Why not layer-first?

Layer-first groups code by technical role. It feels natural at first but fragments features as the codebase grows:

```
# Layer-first — a sources change touches four directories
crates/ps-server/src/
  services/
    config.rs           ← handler
  ...
crates/ps-core/src/
  repo/
    config.rs           ← data access
  models/
    config.rs           ← types
```

Feature-first inverts this. Everything for sources lives together — a new developer can understand (or delete) a feature by looking in one place:

```
# Feature-first — a sources change stays in one directory
crates/ps-server/src/
  features/
    sources/
      mod.rs
      handler.rs        ← gRPC service impl
      service.rs        ← business logic
      repository.rs     ← data access
      types.rs
```

The same principle in TypeScript:

```
# Layer-first — scattered
frontend/
  components/
    source-card.tsx
    secret-form.tsx
  lib/hooks/
    use-config.ts
  app.tsx              # route: /admin → SourcesPage

# Feature-first — colocated
frontend/
  views/
    sources/
      components/
        source-card.tsx
        secret-form.tsx
      hooks/
        use-sources.ts
      pages/
        sources-page.tsx
```

## 2. Naming Conventions

The module path provides sufficient context — do not prefix filenames with the feature name. Redundant prefixing adds noise without value.

```
sources/repository.rs              ← unambiguous
sources/source_repository.rs       ← redundant, avoid

teams/components/member-list.tsx   ← unambiguous
teams/components/team-member-list.tsx  ← redundant, avoid
```

### Rust

| Element       | Convention                             | Example                     |
| :------------ | :------------------------------------- | :-------------------------- |
| Files/modules | snake_case                             | `repository.rs`             |
| Module entry  | `mod.rs`                               |                             |
| Doc comments  | `//!` at the top of every handler file | `//! Sources — gRPC CRUD`   |

### TypeScript

| Element     | Convention               | Example                     |
| :---------- | :----------------------- | :-------------------------- |
| All files   | kebab-case               | `source-card.tsx`           |
| Hooks       | `use-` prefix, `.ts`     | `use-sources.ts`            |
| Pages       | `-page` suffix, `.tsx`   | `sources-page.tsx`          |
| Zod schemas | `.schemas.ts` suffix     | `source-config.schemas.ts`  |
| Type files  | `.types.ts` suffix       | `source.types.ts`           |
| Tests       | `.test.ts` / `.test.tsx` | `parser.test.ts`            |
| Barrels     | `index.ts`               | `index.ts`                  |

### Standard subdirectory names

Use these names consistently within feature modules. Only create directories when the concern exists — a feature with no hooks does not need a `hooks/` directory.

**Rust** — `handlers/` (if multiple transports exist), plus flat sibling files: `service.rs`, `repository.rs`, `types.rs`, `errors.rs`.

**TypeScript** — `components/`, `hooks/`, `lib/`, `pages/`, `types/`.

### Import paths (TypeScript)

All imports use absolute paths — no relative parent (`../`) imports. Within a view, use `@/views/<feature>/...` for cross-directory references (e.g. a page importing from its components). Same-directory relative imports (`./sibling`) are permitted. Shared lib uses `@ps/...`, app-level paths use `@/...`.

## 3. Three-Tier Escalation

Every piece of code has a default home. Start at the lowest tier and only escalate when a concrete second consumer exists — not speculatively.

**Feature-local** (default home for all new code): Rust: `src/features/<name>/`. TypeScript: `views/<name>/` (frontend) or `src/features/<name>/` (backend services).

**Service/app-local** (a second feature within the same service needs it): Rust: top-level modules like `src/auth/`, `src/interceptor.rs`. TypeScript: `components/`, `lib/hooks/`, `lib/`.

Service-level `lib/` deserves specific attention. It holds infrastructure that multiple features depend on but that isn't a feature itself — gRPC transport setup, session management, React Query providers. These are service plumbing, not domain logic. Code belongs in `lib/` when it configures or wraps an external system for use by features (e.g. `lib/api/transport.ts`, `lib/session.ts`). If it contains domain logic, it belongs in a feature instead.

**Shared crate/package** (a second service or package needs it): Rust: `crates/ps-core`. TypeScript: `@ps/shared`, `@ps/ui`.

The signal to lift is always a concrete second consumer. The signal to lift to `ps-core` is another crate (ps-workers, ps-metrics) needing a type or trait. The signal to lift to `@ps/shared` is another package needing a type or utility. The signal to lift to `@ps/ui` is a second app needing a UI component. Move at that point, not before.

## 4. Module Size Tiers

Choose the appropriate structure based on a feature's actual complexity. Do not impose structure preemptively.

### Tier 1 — Small (< ~150 LOC, single concern)

Everything lives in a single file or a handful of colocated files. No subdirectories, no barrel/module file.

**Rust:**

```
features/
  admin/
    mod.rs          ← handler, service, types — all here
```

**TypeScript:**

```
views/
  login/
    login-page.tsx  ← single file, no index.ts
```

### Tier 2 — Medium (150–500 LOC or 3+ distinct concerns)

The module file becomes a pure declaration + re-export surface. Logic moves into named sibling files.

**Rust:**

```
features/
  teams/
    mod.rs           ← declarations + pub use only
    handler.rs       ← gRPC service impl
    service.rs       ← business logic (team membership, directory import)
    repository.rs    ← sqlx queries against org schema
    types.rs
```

**TypeScript:**

```
views/
  teams/
    index.ts              ← re-exports only
    teams.types.ts        ← flat sibling, no types/ dir at this tier
    components/
      team-card.tsx
      member-list.tsx
    pages/
      teams-page.tsx
```

### Tier 3 — Large (500+ LOC, multiple surfaces/pages, complex internals)

Introduce nested subdirectories for complex concerns. The module file remains a pure declaration + re-export surface.

**Rust:**

```
features/
  ingestion/
    mod.rs
    handler.rs           ← Restate handler (plan/fetch/store orchestration)
    registry.rs          ← Source factory
    types.rs
    sources/
      mod.rs
      github/
        mod.rs
        client.rs        ← GraphQL client
        source.rs        ← Source trait impl
        repos.rs         ← Repository discovery
        etag.rs          ← Conditional fetch
        types.rs
      jira/
        mod.rs
        client.rs
        source.rs
        types.rs
```

**TypeScript:**

```
views/
  sources/
    index.ts
    components/
      source-card.tsx
      connection-status.tsx
      secret-form.tsx
    hooks/
      use-sources.ts
      use-connection-test.ts
    lib/
      source-registry/
        config-schemas.ts
        field-definitions.ts
        index.ts        ← allowed: high-cohesion submodule
    pages/
      sources-page.tsx
    types/
      source.types.ts
```

## 5. The Role of `mod.rs` / `index.ts`

Both `mod.rs` (Rust) and `index.ts` (TypeScript) serve the same purpose: declare submodules and define the feature's public API surface. They never contain business logic. A useful test: _can you understand what a feature offers by reading only its module file?_

**Tier 1 exception**: small features may have all logic in `mod.rs` until they grow to warrant promotion to Tier 2. TypeScript Tier 1 features skip `index.ts` entirely — a barrel that re-exports a single file adds indirection for no encapsulation benefit, and at this size direct imports are clearer about what depends on what.

### Rust rules

- `mod.rs` contains `mod` declarations and `pub use` re-exports only.
- If it grows beyond ~50 lines of declarations, logic has leaked in and should be extracted.

### TypeScript rules

- **View-level `index.ts`** (Tier 2+): re-exports only. If it grows beyond ~30 lines, logic has leaked in. Never place `index.ts` at view level for Tier 1 features.
- **Submodule `index.ts`**: allowed at high-cohesion submodule boundaries within `lib/` (e.g. `lib/source-registry/index.ts`). These are internal organisational barrels, not package entry points.
- **Library packages** (`@ps/shared`, `@ps/ui`): use **subpath exports** in `package.json` exclusively. Never use barrel files — they defeat tree-shaking and create circular dependency risks.

## 6. Package and Crate Roles

### Rust crates

| Crate           | Role       | Structure                                                                |
| :-------------- | :--------- | :----------------------------------------------------------------------- |
| `ps-server`     | Service    | `features/` with full tier escalation                                    |
| `ps-workers`    | Service    | Restate worker handlers: ingestion, team sync, metrics compute           |
| `ps-core`       | Library    | Domain types, traits, and repository layer organised by concern          |
| `ps-metrics`    | Library    | Metric computation logic (DORA, flow, etc.)                              |
| `ps-proto`      | Generated  | Protobuf types — never manually edited                                   |
| `ps-migrate`    | Migrations | Sequential migration files, k8s init container                           |
| `psctl`         | CLI        | Lightweight gRPC client                                                  |

The `ps-workers` crate hosts all Restate handlers (ingestion, team sync, metrics compute). Source adapters are nested by platform (github/, jira/, etc.) and implement the `Source` trait from `ps-core`. The three-tier escalation applies normally: domain types shared across crates belong in `ps-core`.

### TypeScript packages

| Package     | Role              | Structure                                        |
| :---------- | :---------------- | :----------------------------------------------- |
| `web`       | Frontend app      | `views/` with full tier escalation               |
| `@ps/shared`| Library           | Subpath exports, no barrels                      |
| `@ps/ui`    | Component library | Flat, subpath exports only                       |

The frontend app uses `views/` as the top-level feature directory (features map to routes/pages). If backend TypeScript services are added, they use `features/`, the same convention as Rust crates.

## 7. Export Strategy

### Rust

`pub use` in `mod.rs` defines the feature's public API. External consumers import through the module path — never reach into internal files directly.

### TypeScript

| Context                  | Strategy                          | Rationale                                                              |
| :----------------------- | :-------------------------------- | :--------------------------------------------------------------------- |
| Library packages         | Subpath exports in `package.json` | Tree-shaking, no circular dependency risk                              |
| App view boundaries      | `index.ts` re-exports             | Internal organisation, not consumed externally                         |
| High-cohesion submodules | `index.ts` barrel                 | Encapsulate complex internal modules (e.g. `lib/source-registry/`)     |
| Generated types          | Subpath exports                   | Never re-export through barrels                                        |

Never re-export something just to shorten an import path — prefer explicit imports that make dependencies visible.

## 8. Test Placement

Tests are colocated with their source in both languages. No separate test directories.

### Rust

- Inline `#[cfg(test)] mod tests` at the bottom of the file under test. Tier 1 features keep tests in `mod.rs`; Tier 2+ features test each file individually (e.g. `service.rs` contains its own test module).
- Integration tests that exercise multiple features or require external services belong in `tests/integration/`.

### TypeScript

- `*.test.ts` / `*.test.tsx` alongside the file under test. A `parser.ts` has a sibling `parser.test.ts`.
- Do not create `__tests__/` directories.
- Integration / E2E tests live in separate test infrastructure outside feature directories.

## 9. Invariants

These hold at all tiers and in both languages:

- A change to a feature's business logic should touch at most two top-level directories: the feature directory and possibly a shared location.
  - Rust: `features/<name>/` and possibly `ps-core`
  - TypeScript: `views/<name>/` and possibly `lib/` or `@ps/shared`
- Deleting a feature directory should remove > 80% of that feature's code.
- No cross-feature imports that bypass a feature's public surface (`mod.rs` or `index.ts`).
- `utils/` and `helpers/` directories must not exist — if something lands there, it needs a proper home in either a feature or a shared location.
