# ps-workers: Feature-First Refactor

## Problem

The `ps-workers` crate has grown to 13 handlers but uses a flat `handlers/` directory with shared infrastructure mixed into its module root. This creates two issues:

1. **Shared handler infrastructure is scattered** — `SharedState` and `load_source_config()` live in `handlers/mod.rs`, journaling macros in `run_lifecycle.rs`, and ingestion orchestration in `ingestion_common.rs` (905 LOC). These are all "how to be a Restate handler in Prism" concerns but aren't cohesive.

2. **Handlers are flat files alongside a module directory** — 12 single-file handlers sit next to `agentic_query/` (which already has its own module). There's no feature grouping — `github_ingestion.rs`, `github_team_sync.rs`, and the `github/` source adapter are three separate things at three levels of the crate.

The source adapters (`github/`, `jira/`, `discourse/`) are already well-structured feature-first modules. The handlers are not.

## Design

Reorganise `ps-workers/src/` so each **domain feature** owns both its handler and its platform-specific code under `features/`, with shared Restate infrastructure extracted into `lib/` — matching plan #18's convention for service-level plumbing.

### Current structure (flat handlers + separate sources)

```
src/
├── handlers/
│   ├── mod.rs                    ← SharedState, load_source_config, 13 pub mods
│   ├── run_lifecycle.rs          ← macros (create_run!, journaled!, etc.)
│   ├── ingestion_common.rs       ← 905 LOC orchestration + secrets + progress
│   ├── github_ingestion.rs       ← handler
│   ├── jira_ingestion.rs         ← handler
│   ├── discourse_ingestion.rs    ← handler
│   ├── github_team_sync.rs       ← handler
│   ├── enrichment.rs             ← handler
│   ├── embedding.rs              ← handler
│   ├── identity_resolution.rs    ← handler
│   ├── metrics_compute.rs        ← handler
│   ├── insights.rs               ← handler
│   ├── model_catalogue.rs        ← handler
│   ├── agent_reaper.rs           ← handler
│   └── agentic_query/            ← already a module
├── github/                       ← source adapter (client, graphql, types, source/)
├── jira/                         ← source adapter (client, source/)
├── discourse/                    ← source adapter (client, source/)
├── registry.rs
├── retry.rs
├── main.rs
└── lib.rs
```

### Target structure (feature-first)

```
src/
├── lib/                          ← service-level plumbing (plan #18 §3)
│   ├── mod.rs                    ← pub use SharedState, load_source_config, secrets
│   ├── state.rs                  ← SharedState struct
│   ├── run_lifecycle.rs          ← macros (unchanged content, moved here)
│   ├── secrets.rs                ← decrypt_required_secret, decrypt_optional_secret
│   ├── registry.rs               ← create_source() (moved from src/registry.rs)
│   └── retry.rs                  ← retry_transient() (moved from src/retry.rs)
│
├── features/
│   ├── mod.rs                    ← pub use all feature modules
│   │
│   ├── ingestion/
│   │   ├── mod.rs                ← pub use all three handlers + lib
│   │   ├── lib/
│   │   │   ├── mod.rs            ← pub use execute_ingestion, IngestionSpec, etc.
│   │   │   ├── orchestration.rs  ← execute_ingestion(), fetch_store_loop()
│   │   │   ├── progress.rs       ← ProgressTracker trait, SerFetchResult
│   │   │   └── finalise.rs       ← finalise_run(), extract helpers, enqueue_enrichments
│   │   ├── github/
│   │   │   ├── mod.rs            ← pub use handlers + source adapter
│   │   │   ├── handler.rs        ← GithubIngestionHandler (from handlers/github_ingestion.rs)
│   │   │   ├── team_sync.rs      ← GithubTeamSyncHandler (from handlers/github_team_sync.rs)
│   │   │   ├── client.rs         ← REST client (from github/client.rs)
│   │   │   ├── graphql.rs        ← GraphQL client (from github/graphql.rs)
│   │   │   ├── types.rs          ← GitHub data structures (from github/types.rs)
│   │   │   ├── etag.rs           ← ETag support (from github/etag.rs)
│   │   │   ├── repos.rs          ← Repo discovery (from github/repos.rs)
│   │   │   └── source/           ← Source trait impl (from github/source/)
│   │   │       ├── mod.rs
│   │   │       ├── plan.rs
│   │   │       ├── fetch.rs
│   │   │       └── store.rs
│   │   ├── jira/
│   │   │   ├── mod.rs
│   │   │   ├── handler.rs        ← JiraIngestionHandler (from handlers/jira_ingestion.rs)
│   │   │   ├── client.rs         ← Jira REST client (from jira/client.rs)
│   │   │   └── source/           ← Source trait impl (from jira/source/)
│   │   │       ├── mod.rs
│   │   │       ├── plan.rs
│   │   │       ├── fetch.rs
│   │   │       └── store.rs
│   │   └── discourse/
│   │       ├── mod.rs
│   │       ├── handler.rs        ← DiscourseIngestionHandler (from handlers/discourse_ingestion.rs)
│   │       ├── client.rs         ← Discourse client (from discourse/client.rs)
│   │       └── source/           ← Source trait impl (from discourse/source/)
│   │           ├── mod.rs
│   │           ├── plan.rs
│   │           ├── fetch.rs
│   │           └── store.rs
│   │
│   ├── identity_resolution/
│   │   ├── mod.rs
│   │   └── handler.rs            ← IdentityResolutionHandler (from handlers/identity_resolution.rs)
│   │
│   ├── reasoning/                ← AI pipeline + agentic infrastructure
│   │   ├── mod.rs                ← pub use all handlers
│   │   ├── enrichment.rs         ← EnrichmentHandler (from handlers/enrichment.rs)
│   │   ├── embedding.rs          ← EmbeddingHandler (from handlers/embedding.rs)
│   │   ├── insights.rs           ← InsightsHandler (from handlers/insights.rs)
│   │   ├── model_catalogue.rs    ← ModelCatalogueHandler (from handlers/model_catalogue.rs)
│   │   ├── agent_reaper.rs       ← AgentPodReaperHandler (from handlers/agent_reaper.rs)
│   │   └── agentic_query/        ← already a module, moves here
│   │       ├── mod.rs
│   │       ├── handler.rs
│   │       ├── query_core.rs
│   │       ├── event_loop.rs
│   │       ├── step_registry.rs
│   │       ├── artifact.rs
│   │       └── trace.rs
│   │
│   └── metrics/
│       ├── mod.rs
│       └── handler.rs            ← MetricsComputeHandler (from handlers/metrics_compute.rs)
│
├── main.rs
└── lib.rs
```

### Key decisions

**1. `lib/` not `common/`** — Plan #18 §3 defines the service-level shared tier as `lib/`: "It holds infrastructure that multiple features depend on but that isn't a feature itself." `SharedState`, lifecycle macros, retry, registry, and secret decryption are exactly this — Restate service plumbing used by all features.

**2. `features/` directory** — Plan #18 §3 specifies the default home for Rust features as `src/features/<name>/`. Using `features/` provides clear separation between service plumbing (`lib/`, `main.rs`) and domain modules, consistent with `ps-server`'s convention.

**3. Ingestion source adapters merge into their handler modules** — Currently `github/` (source adapter) and `handlers/github_ingestion.rs` (handler) are separate. Feature-first says everything for GitHub ingestion lives together. The handler moves into the source module, not the other way around — the source adapter is the bulk of the code.

**4. `ingestion_common.rs` splits into `ingestion/lib/`** — At 905 LOC this file mixes orchestration, secret handling, progress tracking, and finalisation. Secret decryption lifts to `lib/secrets.rs` (used by identity resolution too). The remaining ~800 LOC splits by concern into `ingestion/lib/` since only the 3 ingestion handlers consume it. This mirrors the crate-level `lib/` convention — shared plumbing within a feature gets its own `lib/` subdirectory.

**5. Small handlers still get directories** — Even `metrics_compute.rs` (76 LOC) gets `mod.rs` + `handler.rs`. This is consistent and gives each handler room to grow without restructuring. The `mod.rs` is 2-3 lines of re-exports — negligible cost.

**6. GitHub team sync lives inside `ingestion/github/`** — Team sync uses the same `GitHubClient`, same types, same secret decryption. It's a second handler within the GitHub platform feature, not a separate domain feature. Colocating means a change to GitHub API types or client stays in one directory, and deleting GitHub support removes everything. If other platforms need team sync later, they get their own handler within their own platform module.

**7. AI pipeline handlers group under `reasoning/`** — Enrichment, embedding, insights, model catalogue, agent reaper, and agentic query all depend on `ps_reasoning` and operate on the `reasoning` DB schema. They form a pipeline (enrichment → embedding + insights) but are **not** combined into a single handler because:

- They're separate Restate services with independent invocation/cancellation. Combining modules wouldn't reduce the 4+ `#[restate_sdk::service]` trait definitions.
- Enrichment and embedding need `Arc<RwLock<TaskRouter>>` (AI routing). Insights doesn't. Merging would force simpler handlers to carry unused dependencies.
- They have different trigger patterns: enrichment triggers embedding + insights; embedding self-chains; insights is fire-and-forget. They're pipeline *stages*, not variations of one thing.

Grouping them under `reasoning/` reflects the domain relationship without pretending they're one feature. Metrics stays separate — it's pure computation via `ps_metrics`, has no AI dependency, and is downstream of ingestion, not enrichment. It just happens to share the week/month/quarter iteration pattern with insights.

**8. `bind()` convention for handler registration** — Every feature's `mod.rs` exposes a `bind()` function that takes an `EndpointBuilder`, instantiates its handlers, and returns the builder with them bound. This makes the set of handlers per feature immediately discoverable and keeps `main.rs` clean — it chains feature registrations instead of managing 12+ individual handler structs.

Features that only need `SharedState`:

```rust
// features/ingestion/mod.rs
pub fn bind(endpoint: EndpointBuilder, state: &SharedState) -> EndpointBuilder {
    let github = github::GithubIngestionHandlerImpl { state: state.clone() };
    let team_sync = github::GithubTeamSyncHandlerImpl { state: state.clone() };
    let jira = jira::JiraIngestionHandlerImpl { state: state.clone() };
    let discourse = discourse::DiscourseIngestionHandlerImpl { state: state.clone() };
    endpoint
        .bind(github.serve())
        .bind(team_sync.serve())
        .bind(jira.serve())
        .bind(discourse.serve())
}
```

Features with additional dependencies take them as extra parameters:

```rust
// features/reasoning/mod.rs
pub fn bind(
    endpoint: EndpointBuilder,
    state: &SharedState,
    router: Arc<RwLock<TaskRouter>>,
) -> EndpointBuilder {
    let enrichment = enrichment::EnrichmentHandlerImpl { state: state.clone(), router: router.clone() };
    let embedding = embedding::EmbeddingHandlerImpl { state: state.clone(), router };
    let insights = insights::InsightsHandlerImpl { state: state.clone() };
    let model_catalogue = model_catalogue::ModelCatalogueHandlerImpl { state: state.clone() };
    let agent_reaper = agent_reaper::AgentPodReaperHandlerImpl { state: state.clone() };
    let agentic_query = agentic_query::AgenticQueryHandlerImpl { state: state.clone() };
    endpoint
        .bind(enrichment.serve())
        .bind(embedding.serve())
        .bind(insights.serve())
        .bind(model_catalogue.serve())
        .bind(agent_reaper.serve())
        .bind(agentic_query.serve())
}
```

Then `main.rs` becomes:

```rust
let endpoint = Endpoint::builder();
let endpoint = features::ingestion::bind(endpoint, &state);
let endpoint = features::reasoning::bind(endpoint, &state, ai_router);
let endpoint = features::identity_resolution::bind(endpoint, &state);
let endpoint = features::metrics::bind(endpoint, &state);
```

Handler trait imports (e.g. `use GithubIngestionHandler;`) needed by Restate's `.serve()` method stay inside the `bind()` function's module — `main.rs` never imports individual handler traits. The `bind()` function is part of the feature's public API surface, consistent with plan #18's rule that `mod.rs` defines what a feature offers.

### What doesn't move

- `main.rs` — stays at `src/main.rs`, simplified to chain `bind()` calls
- `lib.rs` — updates module declarations to match new structure

## Breaking down `ingestion_common.rs`

The 905-line file splits across two locations based on consumer scope:

**`lib/secrets.rs`** (~80 LOC) — `decrypt_required_secret()`, `decrypt_optional_secret()`. Used by all 3 ingestion handlers *and* identity resolution (4 consumers across 2 features), so this belongs in crate-wide `lib/`.

**`features/ingestion/lib/`** (~800 LOC across 3 files) — everything else is only consumed by the 3 ingestion handlers:

| File | Contents | ~LOC |
| --- | --- | --- |
| `orchestration.rs` | `execute_ingestion()`, `fetch_store_loop()`, `fetch_batch()`, `store_batch()`, `advance_watermark()` | ~350 |
| `progress.rs` | `ProgressTracker` trait, `SerFetchResult`, `RateLimitInfo` re-exports, `IngestionSpec` | ~120 |
| `finalise.rs` | `finalise_run()`, `extract_watermark()`, `extract_failed_items()`, `enqueue_enrichments()`, `diff_rate_limit_sleep_duration()`, `retry_skipped_diffs()` | ~300 |

The `ingestion/lib/mod.rs` re-exports the public API so handlers import from `crate::features::ingestion::lib::*` — same ergonomics, better organisation.

## Import path changes

All handler files currently import from:
- `crate::handlers::{SharedState, load_source_config}`
- `crate::handlers::ingestion_common::*`
- `crate::handlers::run_lifecycle::*` (macro-expanded paths)
- `crate::github::*`, `crate::jira::*`, `crate::discourse::*`
- `crate::registry::*`
- `crate::retry::*`

After refactor:
- `crate::lib::{SharedState, load_source_config}`
- `crate::features::ingestion::lib::*`
- `crate::lib::run_lifecycle::*` (macro paths update)
- Source adapters become sibling modules: `super::client`, `super::graphql`, `super::source`
- `crate::lib::registry::*`
- `crate::lib::retry::*`

## Execution order

Each step should compile and pass `cargo check` before moving to the next.

### Phase 1 — Extract `lib/` (no handler moves yet)

1. Create `lib/state.rs` with `SharedState` (from `handlers/mod.rs`)
2. Create `lib/mod.rs` with `load_source_config()`, re-exports from `state.rs`
3. Move `run_lifecycle.rs` → `lib/run_lifecycle.rs`
4. Extract secret decryption from `ingestion_common.rs` → `lib/secrets.rs`
5. Move `registry.rs` → `lib/registry.rs`
6. Move `retry.rs` → `lib/retry.rs`
7. Update `handlers/mod.rs` to re-export from `lib` for backwards compatibility
8. Update `lib.rs` to declare `lib` module (note: `lib.rs` and `lib/` coexist in Rust 2021 — `lib.rs` declares `mod lib;` which resolves to `lib/mod.rs`)
9. `cargo check` — all existing code compiles via re-exports

### Phase 2 — Move ingestion handlers into feature modules

10. Create `features/mod.rs`
11. Split remaining `ingestion_common.rs` → `features/ingestion/lib/{mod, orchestration, progress, finalise}.rs`
12. Create `features/ingestion/mod.rs`
13. Move `github/` → `features/ingestion/github/`, add `handler.rs` (from `handlers/github_ingestion.rs`) and `team_sync.rs` (from `handlers/github_team_sync.rs`)
14. Move `jira/` → `features/ingestion/jira/`, add `handler.rs` (from `handlers/jira_ingestion.rs`)
15. Move `discourse/` → `features/ingestion/discourse/`, add `handler.rs` (from `handlers/discourse_ingestion.rs`)
16. Update `lib.rs` — remove old `github`, `jira`, `discourse` modules, add `features`
17. Update all internal imports
18. `cargo check`

### Phase 3 — Move remaining handlers to feature directories

19. Move each handler into its feature directory:
    - `handlers/identity_resolution.rs` → `features/identity_resolution/handler.rs`
    - `handlers/enrichment.rs` → `features/reasoning/enrichment.rs`
    - `handlers/embedding.rs` → `features/reasoning/embedding.rs`
    - `handlers/insights.rs` → `features/reasoning/insights.rs`
    - `handlers/model_catalogue.rs` → `features/reasoning/model_catalogue.rs`
    - `handlers/agent_reaper.rs` → `features/reasoning/agent_reaper.rs`
    - `handlers/agentic_query/` → `features/reasoning/agentic_query/`
    - `handlers/metrics_compute.rs` → `features/metrics/handler.rs`
20. Delete `handlers/` directory entirely
21. Update `features/mod.rs` with all feature modules
22. `cargo check` + `cargo clippy`

### Phase 4 — `bind()` convention and `main.rs` simplification

23. Add `bind()` function to each feature's `mod.rs`:
    - `features/ingestion/mod.rs` — binds GitHub (ingestion + team sync), Jira, Discourse handlers
    - `features/reasoning/mod.rs` — binds enrichment, embedding, insights, model catalogue, agent reaper, agentic query; takes `Arc<RwLock<TaskRouter>>` parameter
    - `features/identity_resolution/mod.rs` — binds identity resolution handler
    - `features/metrics/mod.rs` — binds metrics compute handler
24. Rewrite `main.rs` handler section to chain `bind()` calls — remove all individual handler imports and struct instantiations
25. `cargo check`

### Phase 5 — Cleanup and documentation

26. Remove backwards-compatibility re-exports from Phase 1 (if any remain)
27. Update plan #18 §6 crate roles table — `ps-workers` description should reflect full scope
28. Update CLAUDE.md crate structure table to reflect new layout
29. `prek run -av`

## Documentation updates

### Plan #18 §6 — Crate roles table

Current:

| Crate | Role | Structure |
| --- | --- | --- |
| `ps-workers` | Service | Restate worker handlers: ingestion, team sync, metrics compute |

Updated:

| Crate | Role | Structure |
| --- | --- | --- |
| `ps-workers` | Service | `features/` with full tier escalation; `lib/` for Restate plumbing |

The description in §6 body should be updated from:

> The `ps-workers` crate hosts all Restate handlers (ingestion, team sync, metrics compute). Source adapters are nested by platform (github/, jira/, etc.) and implement the `Source` trait from `ps-core`. The three-tier escalation applies normally: domain types shared across crates belong in `ps-core`.

To:

> The `ps-workers` crate hosts all Restate handlers. Features live in `features/` with full tier escalation: `ingestion/` (GitHub, Jira, Discourse — each owning handler + source adapter + client; GitHub also includes team sync), `identity_resolution/`, `reasoning/` (enrichment, embedding, insights, model catalogue, agent reaper, agentic query), and `metrics/`. Service-level plumbing (`SharedState`, journaling macros, retry, source registry, secret decryption) lives in `lib/`. Source adapters implement the `Source` trait from `ps-core`. Domain types shared across crates belong in `ps-core`.

### CLAUDE.md — Crate structure

The `ps-workers` entry in the crate structure section should be updated to match the new layout:

```
├── ps-workers/       # Restate worker binary
│   └── src/
│       ├── lib/      # Service plumbing: SharedState, journaling macros, retry, registry, secrets
│       └── features/ # Feature modules with full tier escalation
│           ├── ingestion/           # GitHub (+ team sync), Jira, Discourse (handler + source + client each)
│           ├── identity_resolution/ # Discourse identity mapping
│           ├── reasoning/           # AI pipeline: enrichment, embedding, insights, agentic query, model catalogue, agent reaper
│           └── metrics/             # Metric snapshot computation
```

## Risks

- **`lib.rs` vs `lib/` naming** — In Rust 2021, `lib.rs` at the crate root and a `lib/` directory coexist: `mod lib;` in `lib.rs` resolves to `lib/mod.rs`. This is valid but unusual. If it causes confusion, an alternative is naming the module `restate_lib` or `infra` — but `lib` matches plan #18's vocabulary. Verify at Phase 1 step 9.
- **Macro path sensitivity** — `run_lifecycle.rs` macros use `$crate` paths. Moving the file changes what `$crate` resolves to. Since the macros are in `lib/` and `$crate` will resolve to the crate root, this should work — but needs verification at Phase 1 step 9.
- **Restate codegen** — the `#[restate_sdk::object]` / `#[restate_sdk::service]` attribute macros generate code. Moving handler files shouldn't affect this since the macros operate on the trait/impl, not file paths. Verify at Phase 2/3.
- **Git history** — `git mv` preserves rename tracking. Do each logical move as a separate commit for clean history.
