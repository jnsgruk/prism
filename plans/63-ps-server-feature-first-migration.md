# Plan 63: ps-server Feature-First Migration

## Context

`ps-server` still uses a mostly layer-first `src/services/` layout, while project guidance (plan #18) requires feature-first organization for service crates (`src/features/<name>/`). This mismatch increases coupling, slows onboarding, and makes ownership boundaries harder to maintain.

This plan defines a safe, incremental migration to feature-first structure without changing external behavior.

---

## Goals

1. Align `ps-server` with plan #18 feature-first rules.
2. Preserve API behavior and proto compatibility.
3. Minimize risk via compatibility shims and phased cutover.
4. Keep commits small and reviewable.
5. Leave clear ownership boundaries for future work.

## Non-goals

1. No proto contract redesign.
2. No major business logic rewrites.
3. No cross-crate architectural changes beyond import-path alignment.
4. No migration of unrelated frontend features (handled separately).

---

## Current vs Target Structure

### Current

```
crates/ps-server/src/
├── main.rs
├── lib.rs
├── interceptor.rs
└── services/
    ├── mod.rs
    ├── common.rs            # auth helpers, error mappers, proto conversions
    ├── admin.rs             # 253 LOC
    ├── auth.rs              # 294 LOC
    ├── config.rs            # 663 LOC
    ├── insights.rs          # 357 LOC
    ├── handlers/            # 1185 LOC total
    │   ├── mod.rs           #   handler defs, platform mapping, state derivation
    │   ├── grpc.rs          #   gRPC service impl
    │   └── restate.rs       #   Restate HTTP dispatch
    ├── metrics/             # 966 LOC total
    │   ├── mod.rs           #   MetricsServiceImpl, conversion helpers
    │   └── grpc.rs          #   gRPC service impl
    ├── org/                 # 956 LOC total
    │   ├── mod.rs           #   OrgServiceImpl, gRPC trait impl dispatch
    │   ├── people.rs        #   people handler functions
    │   ├── teams.rs         #   team handler functions
    │   └── conversions.rs   #   person/team domain→proto builders
    └── reasoning/           # ~3000 LOC total
        ├── mod.rs           #   ReasoningServiceImpl, gRPC trait impl dispatch
        ├── ai_settings.rs
        ├── conversations.rs
        ├── convert.rs       #   enrichment/similarity domain→proto
        ├── cost.rs
        ├── embeddings.rs
        ├── enrichments.rs
        └── agent_query/
            ├── mod.rs
            ├── event_loop.rs
            ├── event_mapping.rs
            ├── artifact.rs
            ├── resume.rs
            ├── session.rs
            ├── step_registry.rs
            └── trace.rs
```

### Target (feature-first)

```
crates/ps-server/src/
├── main.rs
├── lib.rs
├── interceptor.rs                # service-level auth middleware
├── common/                       # service-level shared plumbing (Tier 2 escalation)
│   ├── mod.rs
│   ├── auth.rs                   # require_auth, require_admin, db_err, backup_err
│   └── conversions.rs            # proto/domain enum conversion helpers
└── features/
    ├── mod.rs
    ├── admin/
    │   └── mod.rs                # Tier 1: handler + service in one file
    ├── auth/
    │   └── mod.rs                # Tier 1: handler + service in one file
    ├── config/
    │   ├── mod.rs                # Tier 2: gRPC dispatch surface
    │   └── handler.rs            # gRPC handler functions
    ├── dispatch/                 # service-level Restate/handler plumbing
    │   ├── mod.rs                # handler defs, platform mapping, state derivation
    │   ├── grpc.rs               # gRPC service impl
    │   └── restate.rs            # Restate HTTP dispatch
    ├── insights/
    │   ├── mod.rs                # Tier 2: gRPC dispatch surface
    │   └── handler.rs            # gRPC handler functions
    ├── metrics/
    │   ├── mod.rs                # Tier 2: MetricsServiceImpl + conversion helpers
    │   └── handler.rs            # gRPC handler functions
    ├── org/
    │   ├── mod.rs                # Tier 2: OrgServiceImpl, gRPC dispatch
    │   ├── people.rs
    │   ├── teams.rs
    │   └── conversions.rs
    └── reasoning/
        ├── mod.rs                # Tier 3: ReasoningServiceImpl, gRPC dispatch
        ├── ai_settings.rs
        ├── conversations.rs
        ├── convert.rs
        ├── cost.rs
        ├── embeddings.rs
        ├── enrichments.rs
        └── agent_query/
            ├── mod.rs
            ├── event_loop.rs
            ├── event_mapping.rs
            ├── artifact.rs
            ├── resume.rs
            ├── session.rs
            ├── step_registry.rs
            └── trace.rs
```

### Structural Decisions

**`config` keeps its name.** The original plan proposed renaming to `sources`. That rename reflects domain intent but adds unnecessary risk to a pure structural refactor — it touches import paths, documentation, and mental models simultaneously. If a rename is desired, do it in a separate follow-up commit after the migration is complete.

**`handlers/` becomes `dispatch/`.** The old `handlers/` module is not a domain feature — it is service plumbing that maps platform types to Restate handler names, dispatches HTTP calls to Restate, and exposes gRPC endpoints for triggering/cancelling runs. Calling it `handlers/` inside `features/` conflicts with plan #18's use of `handler.rs` for gRPC service implementations. `dispatch/` accurately describes what it does. It lives under `features/` because it implements a gRPC service (HandlerService), but its handler defs and platform mapping are consumed by other features too — if that coupling grows, it could be promoted to service-level `common/` later.

**`admin` and `auth` are Tier 1.** At ~250-300 LOC each with a single concern, plan #18 says these stay as a single `mod.rs` file. No `handler.rs` split needed.

**`common.rs` splits into `auth.rs` + `conversions.rs`.** The current `common.rs` (382 LOC) mixes auth helpers (~50 LOC), error mappers (~10 LOC), and proto conversions (~320 LOC). Auth helpers and error mappers are both small and closely related (both produce `tonic::Status`) — they stay together in `common/auth.rs`. The proto conversion functions are a distinct concern and go in `common/conversions.rs`.

**Naming follows plan #18.** gRPC service impl files are named `handler.rs`. `mod.rs` files are declaration/re-export surfaces only (with Tier 1 exception for small features).

---

## Design Principles

1. Feature ownership first: transport and helpers live inside feature modules unless shared by at least two features.
2. Three-tier escalation per plan #18:
   - feature-local (`features/<name>/...`)
   - service-local (`common/...`, `interceptor.rs`)
   - shared crate (`ps-core`, `ps-metrics`) only when a second crate needs it.
3. `mod.rs` files are declaration/re-export surfaces only (Tier 1 exception: small features keep logic in `mod.rs`).
4. No `utils/` or `helpers/` directories.
5. Preserve existing public server behavior throughout migration.
6. Tier classification drives structure — do not impose Tier 2/3 structure on Tier 1 features.

---

## Migration Strategy

### Phase 0: Baseline and Safety Rails

1. Capture behavior baseline:
   - `cargo test`
   - integration API tests
   - selected manual smoke flows (login, setup, source CRUD, team CRUD, ask stream).
2. Freeze scope:
   - No behavior refactors in same commits as file moves.

Deliverable: green baseline with explicit migration branch policy.

### Phase 1: Scaffold features directory and split common

1. Create `src/features/mod.rs`.
2. Create `src/common/mod.rs` with `auth.rs` and `conversions.rs`, populated from `services/common.rs`.
3. Update all existing `services/*` modules to import from `crate::common::` instead of `crate::services::common::`.
4. Wire `lib.rs` to expose both `pub mod common` and `pub mod features`.
5. Keep `services/common.rs` as a thin re-export shim during transition.

Deliverable: new `common/` module is the source of truth; `services/common.rs` re-exports for compatibility.

### Phase 2: Move features

Migrate in this order (small/simple first to build confidence, complex last):

1. **auth** — Tier 1, no internal dependencies beyond `common`.
2. **admin** — Tier 1, depends only on `common`.
3. **config** — Tier 2, self-contained CRUD.
4. **insights** — Tier 2, depends only on `common`.
5. **org** — Tier 2, has internal submodules (people, teams, conversions).
6. **metrics** — Tier 2, has internal submodule (handler). Rename `grpc.rs` → `handler.rs`.
7. **dispatch** — Tier 3, rename from `handlers/`. Keep `grpc.rs` and `restate.rs` names since they describe distinct transport targets, not generic "handler" files.
8. **reasoning** — Tier 3, largest module. Move as a unit including `agent_query/` submodule.

For each feature:
1. Move files to `features/<name>/...`.
2. Update internal imports to use `crate::features::<name>::...`.
3. Keep old `services/<name>` as a re-export shim until Phase 3.
4. Run `cargo build && cargo test && cargo clippy --allow-dirty` after each feature.

Deliverable: all feature code physically colocated under `features/`.

### Phase 3: Remove shims and dead paths

1. Delete `src/services/` entirely — all re-export shims removed.
2. Update `lib.rs` to remove `pub mod services`.
3. Update `main.rs` imports if they reference old paths.
4. Run `prek run -av` as final gate.

Deliverable: single source of truth under `features/` + `common/`.

---

## Detailed File Mapping

| Current Path | Target Path | Notes |
|---|---|---|
| `services/common.rs` | `common/auth.rs` | `require_auth`, `require_admin`, `db_err`, `backup_err` |
| `services/common.rs` | `common/conversions.rs` | All proto/domain conversion functions + tests |
| `services/auth.rs` | `features/auth/mod.rs` | Tier 1: single file |
| `services/admin.rs` | `features/admin/mod.rs` | Tier 1: single file |
| `services/config.rs` | `features/config/mod.rs` + `handler.rs` | Tier 2 |
| `services/insights.rs` | `features/insights/mod.rs` + `handler.rs` | Tier 2 |
| `services/org/mod.rs` | `features/org/mod.rs` | Dispatch surface |
| `services/org/people.rs` | `features/org/people.rs` | |
| `services/org/teams.rs` | `features/org/teams.rs` | |
| `services/org/conversions.rs` | `features/org/conversions.rs` | Feature-local conversions |
| `services/metrics/mod.rs` | `features/metrics/mod.rs` | |
| `services/metrics/grpc.rs` | `features/metrics/handler.rs` | Renamed per plan #18 |
| `services/handlers/mod.rs` | `features/dispatch/mod.rs` | Renamed to avoid `handler` ambiguity |
| `services/handlers/grpc.rs` | `features/dispatch/grpc.rs` | |
| `services/handlers/restate.rs` | `features/dispatch/restate.rs` | |
| `services/reasoning/mod.rs` | `features/reasoning/mod.rs` | |
| `services/reasoning/ai_settings.rs` | `features/reasoning/ai_settings.rs` | |
| `services/reasoning/conversations.rs` | `features/reasoning/conversations.rs` | |
| `services/reasoning/convert.rs` | `features/reasoning/convert.rs` | |
| `services/reasoning/cost.rs` | `features/reasoning/cost.rs` | |
| `services/reasoning/embeddings.rs` | `features/reasoning/embeddings.rs` | |
| `services/reasoning/enrichments.rs` | `features/reasoning/enrichments.rs` | |
| `services/reasoning/agent_query/*` | `features/reasoning/agent_query/*` | All 8 files move as a unit |

---

## Validation & Quality Gates

Run after each phase (or feature chunk):

1. `cargo build`
2. `cargo test`
3. `cargo clippy --allow-dirty --fix`
4. `nix fmt`
5. `prek run -av` (required before final merge)

Proto changes are not expected — if proto/types were touched:
1. `buf lint`
2. `buf generate`

Required outcome before merge:
- Zero clippy warnings.
- Formatting clean.
- No behavior regressions in integration tests.

---

## Commit Hygiene Plan

Commit in logical slices:

1. `refactor(ps-server): split common module into auth and conversions`
2. `refactor(ps-server): introduce features directory with auth and admin`
3. `refactor(ps-server): migrate config and insights to features`
4. `refactor(ps-server): migrate org to features`
5. `refactor(ps-server): migrate metrics to features`
6. `refactor(ps-server): migrate handlers to features/dispatch`
7. `refactor(ps-server): migrate reasoning to features`
8. `refactor(ps-server): remove services compatibility shims`
9. `docs: update architecture references for ps-server feature layout`

Rules:
- Keep plan updates in separate `plans/` docs commits.
- Do not mix `.sqlx/` updates with code changes.
- Use `--no-gpg-sign` for autonomous commits.

---

## Risks and Mitigations

1. **Import-path breakage**
   Mitigation: temporary `services/*` re-export shims until Phase 3.

2. **Hidden behavior changes during file moves**
   Mitigation: no logic rewrites in move commits; isolate pure move + import changes.

3. **Large review complexity in reasoning feature**
   Mitigation: reasoning moves as a unit (its internal structure is already clean). The `agent_query/` submodule stays self-contained.

4. **Merge conflicts with active feature work**
   Mitigation: migrate by feature slices; rebase frequently; publish early structure changes.

5. **Incomplete documentation alignment**
   Mitigation: update `CLAUDE.md` + relevant plan docs in final docs commit.

---

## Rollback Plan

1. If any phase regresses behavior, revert only the latest feature migration commit.
2. Keep shims until Phase 3 so rollback is cheap and low risk.
3. If needed, pause at phase boundary with mixed layout but clean behavior.

---

## Success Criteria

1. `ps-server` has no feature logic under `src/services/`.
2. `src/features/` contains all domain-facing service modules.
3. `common/` only contains cross-feature service plumbing.
4. `mod.rs` files are API surfaces only (Tier 1 exception applies).
5. Tier classification matches plan #18 size guidelines.
6. Full checks pass with no new warnings.
7. Documentation reflects final structure unambiguously.

---

## Implementation Checklist

- [ ] Phase 0 baseline captured and green
- [ ] `common.rs` split into `common/{auth,conversions}.rs`
- [ ] Feature skeletons added (`features/mod.rs`)
- [ ] Auth migrated (Tier 1)
- [ ] Admin migrated (Tier 1)
- [ ] Config migrated (Tier 2)
- [ ] Insights migrated (Tier 2)
- [ ] Org migrated (Tier 2)
- [ ] Metrics migrated (Tier 2, `grpc.rs` → `handler.rs`)
- [ ] Dispatch migrated (renamed from `handlers/`)
- [ ] Reasoning migrated (Tier 3, including `agent_query/`)
- [ ] `services/` shims removed
- [ ] Full validation (`prek run -av`) passes
- [ ] Docs updated (CLAUDE.md crate structure section)
