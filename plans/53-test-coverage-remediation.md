# Plan 53 — Test Coverage Remediation

## Problem Statement

Plans 13–15, 44, 49, 50, and 52 all specified extensive test coverage, but the vast majority was never implemented. The current state:

| Area | Existing tests | Approximate source LoC | Coverage |
| --- | --- | --- | --- |
| ps-core unit (auth, crypto, backup, models, pagination) | 35 tests in 8 files | ~5,600 | Low — no repo layer tests, no enum round-trips |
| ps-metrics unit (flow, discourse, lib) | 27 tests in 3 files | ~300 | Moderate — computation logic covered, but no DB query tests |
| ps-workers unit (github client only) | 8 tests in 2 files | ~6,300 | Very low — no source adapter, handler, or retry tests |
| ps-reasoning unit (embedding text only) | 5 tests in 1 file | ~1,600 | Very low — no enrichment, routing, or catalogue tests |
| Integration tests (gRPC via `define_api_test!`) | ~22 tests in 4 files (~970 LoC) | — | Auth, config, org, ingestion status only |
| ps-server (all service impls) | 0 inline tests | ~2,650 | None — relies on integration tests above |

**Total: ~97 tests covering ~16,500 LoC across 6 crates.** The repository layer (19 files, all DB access), source adapters (15 files), handler orchestration, and most gRPC services have zero test coverage.

## Goals

1. **Repository layer integration tests** — every repo method tested against real PostgreSQL
2. **Source adapter tests** — GitHub, Jira, Discourse fetch/store logic tested with wiremock
3. **Handler orchestration tests** — ingestion lifecycle, cursor transitions, failure isolation
4. **Metrics computation tests** — DORA formulas with known datasets, snapshot storage
5. **gRPC API coverage** — expand existing integration tests to cover metrics, admin, reasoning services
6. **AI/reasoning tests** — enrichment extraction, model routing, embedding pipeline (mocked providers)

## Non-Goals

- 100% line coverage — focus on correctness of critical paths
- Testing generated proto code (ps-proto)
- Testing Restate runtime behaviour (requires Restate server; handlers tested via business logic extraction)
- E2E browser tests (frontend tests are a separate concern)
- Testing ps-migrate or psctl (thin wrappers)

## Test Infrastructure

### Existing

- **`define_api_test!`** macro: creates isolated PG database per test, starts real gRPC server, runs migrations, cleans up. Lives in `tests/integration/src/common/macros.rs`.
- **`TestServer`**: starts tonic server on random port with all services wired. Lives in `tests/integration/src/common/server.rs`.
- **`create_admin_user()`** fixture: inserts admin user + session. Lives in `tests/integration/src/common/fixtures.rs`.

### New Macros Needed

#### `define_repo_test!`

For testing repository methods directly against PostgreSQL without a gRPC server. Lighter than `define_api_test!` — just a `PgPool` + `Repos`.

```rust
macro_rules! define_repo_test {
    ($name:ident, |$repos:ident, $pool:ident| async move $body:block) => {
        #[tokio::test]
        async fn $name() {
            // Same DB setup as define_api_test! but no gRPC server
            // Provides: $repos: Repos, $pool: PgPool
        }
    };
}
```

#### `define_source_test!`

For testing Source trait implementations with wiremock + real PostgreSQL. Sets up a wiremock server, source config, and IngestionContext.

```rust
macro_rules! define_source_test {
    ($name:ident, |$ctx:ident, $mock:ident, $repos:ident| async move $body:block) => {
        #[tokio::test]
        async fn $name() {
            // DB setup + wiremock::MockServer::start()
            // Creates SourceConfig pointing at mock server
            // Builds IngestionContext with test secret key
            // Provides: $ctx: IngestionContext, $mock: MockServer, $repos: Repos
        }
    };
}
```

### New Fixture Builders Needed

Expand `tests/integration/src/common/fixtures.rs`:

```rust
// Source configs
pub async fn create_github_source(repos: &Repos) -> SourceConfig;
pub async fn create_jira_source(repos: &Repos) -> SourceConfig;
pub async fn create_discourse_source(repos: &Repos) -> SourceConfig;

// People & teams
pub async fn create_team(repos: &Repos, name: &str) -> Team;
pub async fn create_person(repos: &Repos, name: &str, email: &str) -> Person;
pub async fn create_person_with_identity(repos: &Repos, platform: Platform, username: &str) -> Person;
pub async fn add_team_member(repos: &Repos, team_id: Uuid, person_id: Uuid);

// Contributions
pub async fn create_contribution(repos: &Repos, input: ContributionInput) -> Uuid;
pub async fn create_github_pr(repos: &Repos, person_id: Uuid, repo: &str) -> Uuid;
pub async fn create_jira_ticket(repos: &Repos, person_id: Uuid, project: &str) -> Uuid;
pub async fn create_discourse_post(repos: &Repos, person_id: Uuid) -> Uuid;

// Ingestion runs
pub async fn create_completed_run(repos: &Repos, source_id: Uuid, items: i32) -> Uuid;
```

### Test Directory Structure

```
tests/integration/src/
├── common/
│   ├── macros.rs          # define_api_test!, define_repo_test!, define_source_test!
│   ├── fixtures.rs        # Builder functions for test data
│   ├── server.rs          # TestServer (existing)
│   └── wiremock_helpers.rs # Canned API responses for GitHub/Jira/Discourse
├── api/                   # gRPC integration tests (existing + expanded)
│   ├── auth.rs            # (existing)
│   ├── config.rs          # (existing)
│   ├── org.rs             # (existing, expand)
│   ├── ingestion.rs       # (existing)
│   ├── metrics.rs         # NEW
│   ├── admin.rs           # NEW
│   └── reasoning.rs       # NEW
├── repo/                  # Repository layer tests
│   ├── mod.rs
│   ├── auth.rs
│   ├── config.rs
│   ├── org.rs
│   ├── activity.rs
│   ├── metrics.rs
│   └── reasoning.rs
├── source/                # Source adapter tests (wiremock)
│   ├── mod.rs
│   ├── github.rs
│   ├── jira.rs
│   └── discourse.rs
├── metrics/               # Metrics computation with real DB
│   ├── mod.rs
│   ├── flow.rs
│   ├── snapshots.rs
│   └── discourse.rs
└── lib.rs                 # (existing, add new modules)
```

### Dependencies to Add

```toml
# tests/integration/Cargo.toml
wiremock = "0.6"
ps-workers = { path = "../../crates/ps-workers" }
ps-metrics = { path = "../../crates/ps-metrics" }
ps-reasoning = { path = "../../crates/ps-reasoning" }
```

---

## Phases

### Phase 1 — Repository Layer (highest risk, no current coverage)

The repo layer is the foundation — every other test layer depends on correct DB operations. Test all repos against real PostgreSQL.

#### 1A: Auth & Config Repos

**File:** `tests/integration/src/repo/auth.rs`

| Test | What it verifies |
| --- | --- |
| `create_user_and_find_by_username` | User insert, case-insensitive lookup |
| `create_session_and_validate` | Session insert, token hash lookup, expiry check |
| `delete_expired_sessions` | Cleanup query removes only expired rows |
| `create_api_token_and_list` | API token CRUD, listing by user |
| `revoke_api_token` | Soft-delete / hard-delete behaviour |

**File:** `tests/integration/src/repo/config.rs`

| Test | What it verifies |
| --- | --- |
| `create_source_and_get` | Source insert, retrieval by ID |
| `list_sources_filters_by_type` | Platform type filter in list query |
| `update_source_settings` | JSONB settings merge/replace |
| `store_and_retrieve_secret` | Encrypted secret round-trip (encrypt → store → load → decrypt) |
| `secret_status_shows_which_keys_set` | Boolean map of set/unset secrets |
| `delete_source_cascades` | Cascade to secrets, watermarks, runs |

#### 1B: Org Repo (people, teams, identities)

**File:** `tests/integration/src/repo/org.rs`

| Test | What it verifies |
| --- | --- |
| `batch_upsert_people` | UNNEST bulk insert, ON CONFLICT update |
| `find_person_by_email` | Email lookup, case-insensitive |
| `batch_upsert_identities` | Platform identity creation, person linking |
| `resolve_person_by_identity` | Identity → person_id resolution |
| `case_insensitive_identity_lookup` | Mixed-case username resolves correctly (plan 50) |
| `create_team_hierarchy` | Parent-child team relationships |
| `list_team_members_with_pagination` | Cursor-based pagination, sort order |
| `search_people_with_like_escaping` | `%` and `_` in search terms don't break LIKE |
| `batch_upsert_github_teams` | GitHub team sync data storage |
| `import_directory_creates_teams_and_people` | Full directory import flow |
| `resolve_discourse_identities` | Identity resolution heuristics |

#### 1C: Activity Repo (contributions, runs, watermarks)

**File:** `tests/integration/src/repo/activity.rs`

| Test | What it verifies |
| --- | --- |
| `batch_upsert_contributions` | UNNEST bulk upsert, dedup by external_id |
| `upsert_contribution_idempotent` | Same external_id twice → one row, updated fields |
| `list_contributions_with_filters` | Platform, type, state, date range, person filters |
| `list_contributions_pagination` | Cursor pagination, sort by multiple fields |
| `create_run_and_update_progress` | Run lifecycle: created → running → completed |
| `fail_run_records_metadata` | Failed run with error metadata JSONB |
| `complete_run_with_warnings` | Partial failure metadata |
| `get_watermark_and_advance` | Watermark read/write per source+key |
| `watermark_not_advanced_on_failure` | Watermark stays at old value |
| `list_runs_by_source` | Filter runs by source_id, status |
| `get_source_statuses_cross_schema_join` | Joins config + activity for status view |
| `store_and_check_etag` | ETag cache insert/lookup |
| `invocation_tracking` | Current invocation ID set/clear |
| `contribution_data_jsonb_roundtrip` | All ContributionData variants survive DB storage |

#### 1D: Metrics & Reasoning Repos

**File:** `tests/integration/src/repo/metrics.rs`

| Test | What it verifies |
| --- | --- |
| `upsert_snapshot_and_retrieve` | Team metric snapshot storage, period type |
| `get_team_contributions_for_period` | Date range query, platform filter |
| `get_flow_metrics_for_team` | Cycle time, lead time, WIP from contributions |
| `get_throughput_by_period` | Completed items count over date range |
| `get_person_metrics` | Individual contributor stats |
| `get_discourse_metrics_by_team` | Discourse-specific metric queries |
| `source_activity_summary` | Per-source contribution counts |

**File:** `tests/integration/src/repo/reasoning.rs`

| Test | What it verifies |
| --- | --- |
| `bulk_enqueue_enrichments` | Queue insert, no duplicates on re-enqueue |
| `find_pending_enrichments` | Queue query with limit |
| `store_enrichment_result` | Enrichment value + metadata storage |
| `bulk_enqueue_embeddings` | Embedding queue insert |
| `store_embedding_vector` | pgvector storage (if extension available) |
| `find_similar_contributions` | Cosine similarity search |
| `store_and_list_insights` | Insight persistence, team/period filter |

**Estimated effort:** 50–60 test functions, ~2,000 LoC.

---

### Phase 2 — Source Adapter Tests (wiremock)

Test the `Source` trait implementations for each platform. These tests use wiremock to simulate external APIs and real PostgreSQL for storage verification.

#### 2A: GitHub Source

**File:** `tests/integration/src/source/github.rs`

**Wiremock fixtures needed:** `tests/integration/src/common/wiremock_helpers.rs`
- Canned GraphQL response for PR list (with reviews inline)
- Canned GraphQL response for member search
- Canned REST response for team repos
- Rate limit headers (normal, low, exhausted)
- 304 Not Modified response
- 5xx error response
- 403 forbidden response

| Test | What it verifies |
| --- | --- |
| `plan_loads_repos_from_team_sync` | Plan phase discovers repos from org.github_teams |
| `plan_falls_back_to_org_discovery` | No teams configured → full org repo list |
| `fetch_batch_parses_graphql_prs` | GraphQL response → ContributionInput mapping |
| `fetch_batch_handles_pagination` | Cursor advances through pages |
| `fetch_batch_handles_304` | Etag cache hit → empty batch, no error |
| `store_batch_upserts_contributions` | ContributionInput → DB rows |
| `store_batch_creates_identities` | New GitHub usernames create platform_identity rows |
| `watermark_advances_after_store` | max_updated_at moves forward |
| `two_phase_transitions_correctly` | TeamRepos phase → MemberSearch phase cursor |
| `failed_repo_recorded_in_cursor` | 403 for one repo → FailedItem, others succeed |
| `rate_limit_low_logs_warning` | Rate limit remaining < 100 triggers warning |
| `transient_error_retried` | 5xx → retry up to 3 times |

#### 2B: Jira Source

**File:** `tests/integration/src/source/jira.rs`

**Wiremock fixtures:** Canned Jira REST responses (issue search, project list, pagination tokens).

| Test | What it verifies |
| --- | --- |
| `plan_discovers_projects` | Jira project list → plan items |
| `fetch_batch_parses_issues` | Jira issue JSON → ContributionInput |
| `fetch_batch_computes_cycle_time` | Status changelog → cycle_time_hours |
| `fetch_batch_computes_lead_time` | Created → resolved → lead_time_hours |
| `store_batch_upserts_jira_tickets` | DB storage, JSONB data field |
| `watermark_advances_per_project` | Per-project watermark tracking |
| `failed_project_isolation` | 403 for one project → partial success |
| `pagination_token_advances` | startAt/maxResults pagination |

#### 2C: Discourse Source

**File:** `tests/integration/src/source/discourse.rs`

**Wiremock fixtures:** Canned Discourse API responses (category list, topic list, topic detail with posts).

| Test | What it verifies |
| --- | --- |
| `plan_discovers_categories` | Category list → plan items |
| `fetch_batch_parses_topics` | Topic JSON → ContributionInput list (topic + posts + likes) |
| `store_batch_merges_first_post` | First post content merged into topic, not stored separately |
| `store_batch_creates_identities` | Discourse usernames → platform_identity rows |
| `watermark_uses_bumped_at` | max_bumped_at (not updated_at) as watermark |
| `category_iteration` | Multiple categories iterated in order |
| `etag_handling` | 304 responses handled correctly |

**Estimated effort:** 30–35 test functions, ~2,500 LoC (including wiremock fixtures).

---

### Phase 3 — Metrics Computation

Test the ps-metrics crate's computation functions with real database data. These tests seed known contributions and verify metric calculations produce expected results.

#### 3A: Flow Metrics (Integration)

**File:** `tests/integration/src/metrics/flow.rs`

| Test | What it verifies |
| --- | --- |
| `cycle_time_from_jira_tickets` | Known tickets with cycle_time_hours → correct average |
| `lead_time_from_prs_and_jira` | Mixed platform lead times |
| `wip_counts_open_items` | Items in "in_progress" state counted correctly |
| `throughput_counts_completed_by_period` | Weekly/monthly throughput from completed contributions |
| `flow_efficiency_from_state_durations` | Active vs. wait time ratio |
| `empty_period_returns_none` | No contributions → None, not zero |

#### 3B: Snapshot Computation

**File:** `tests/integration/src/metrics/snapshots.rs`

| Test | What it verifies |
| --- | --- |
| `compute_week_snapshot` | End-to-end: seed contributions → compute → verify snapshot row |
| `compute_month_snapshot` | Monthly period boundaries correct |
| `compute_quarter_snapshot` | Quarterly period boundaries correct |
| `recompute_overwrites_stale` | Second computation for same period updates existing snapshot |
| `snapshot_includes_source_counts` | Per-platform contribution counts in snapshot JSONB |
| `multi_team_computation` | Multiple teams computed in single batch |

#### 3C: Discourse Metrics (Integration)

**File:** `tests/integration/src/metrics/discourse.rs`

| Test | What it verifies |
| --- | --- |
| `basic_discourse_metrics` | Post count, topic count, like count from contributions |
| `multi_instance_breakdown` | Metrics split by Discourse instance |
| `discourse_metrics_per_person` | Individual contributor discourse stats |

**Estimated effort:** 15–18 test functions, ~1,000 LoC.

---

### Phase 4 — gRPC API Expansion

Expand the existing `define_api_test!` suite to cover services not currently tested.

#### 4A: Metrics Service

**File:** `tests/integration/src/api/metrics.rs`

| Test | What it verifies |
| --- | --- |
| `get_team_metrics_empty` | No snapshots → empty response |
| `get_team_metrics_with_data` | Seeded snapshot → correct proto response |
| `get_team_contributions` | Paginated contribution list for team |
| `get_flow_metrics` | DORA metrics via gRPC |
| `get_individual_metrics` | Person-level metrics |

#### 4B: Admin Service

**File:** `tests/integration/src/api/admin.rs`

| Test | What it verifies |
| --- | --- |
| `list_users` | Admin can list all users |
| `create_user_as_admin` | Admin creates non-admin user |
| `non_admin_rejected` | Regular user gets PermissionDenied |
| `change_user_role` | Promote/demote user |
| `backup_and_restore` | Export → import round-trip |

#### 4C: Reasoning Service

**File:** `tests/integration/src/api/reasoning.rs`

| Test | What it verifies |
| --- | --- |
| `get_enrichment_status` | Queue counts via gRPC |
| `get_embedding_status` | Embedding coverage stats |
| `list_models` | Model catalogue listing |
| `find_similar_empty` | No embeddings → empty response |

**Estimated effort:** 15–18 test functions, ~800 LoC.

---

### Phase 5 — Unit Tests for Untested Pure Logic

Inline `#[cfg(test)]` modules for pure functions that don't need a database.

#### 5A: Domain Enums (`ps-core/src/models/enums.rs`)

| Test | What it verifies |
| --- | --- |
| `platform_roundtrip` | Every Platform variant → Display → FromStr → same variant |
| `contribution_type_roundtrip` | All ContributionType variants |
| `contribution_state_roundtrip` | All ContributionState variants |
| `ingestion_status_roundtrip` | All IngestionStatus variants |
| `period_type_roundtrip` | All PeriodType variants |
| `role_roundtrip` | All Role variants |
| `unknown_string_errors` | `"bogus".parse::<Platform>()` returns Err |

#### 5B: Retry Logic (`ps-workers/src/retry.rs`)

| Test | What it verifies |
| --- | --- |
| `retries_on_transient_error` | 5xx → retried up to 3 times |
| `no_retry_on_permanent_error` | 4xx (not 429) → immediate failure |
| `succeeds_on_second_attempt` | First call 500, second call 200 → success |
| `backoff_increases` | Delays are 1s, 2s, 4s (verify via elapsed time or mock clock) |

#### 5C: Error Types (`ps-core/src/error.rs`)

| Test | What it verifies |
| --- | --- |
| `http_status_is_transient_for_5xx` | 500, 502, 503 classified as transient |
| `http_status_not_transient_for_4xx` | 400, 403, 404 not transient |
| `rate_limit_not_transient` | 429 classified separately |

#### 5D: Cursor Serialisation (`ps-workers/src/{github,jira,discourse}/source/`)

| Test | What it verifies |
| --- | --- |
| `github_cursor_roundtrip` | Serialize → deserialize preserves all fields |
| `github_cursor_forward_compat` | Missing new field → serde(default) fills it |
| `jira_cursor_roundtrip` | Same |
| `discourse_cursor_roundtrip` | Same |

#### 5E: AI/Reasoning Pure Logic (`ps-reasoning/src/`)

| Test | What it verifies |
| --- | --- |
| `cost_calculation_per_model` | Token count × price → correct cost |
| `enrichment_prompt_construction` | Known contribution → expected prompt string |
| `enrichment_type_extraction` | Known JSON → correct EnrichmentResult fields |
| `model_routing_selects_cheapest` | Given routing config → correct provider chosen |
| `catalogue_response_parsing` | Canned OpenRouter/Gemini JSON → ModelInfo list |

**Estimated effort:** 25–30 test functions, ~800 LoC.

---

### Phase 6 — Handler Orchestration Tests

Handler tests are the hardest because they depend on Restate's runtime. Strategy: **extract testable business logic from handler methods into free functions**, then test those functions. The thin Restate `ctx` layer stays untested (acceptable — it's glue code).

#### 6A: Ingestion Common (`ingestion_common.rs`)

Extract and test:
- `finalise_run()` logic (given failed items count → correct status + watermark decision)
- Progress tracker implementations (given batch → correct counts)
- Cursor transition logic (given fetch result → next cursor)

| Test | What it verifies |
| --- | --- |
| `finalise_no_failures_advances_watermark` | 0 failures, items > 0 → completed + watermark advanced |
| `finalise_all_failures_no_watermark` | All failed → failed status, watermark unchanged |
| `finalise_partial_warnings` | Some failed → completed_with_warnings, watermark unchanged |
| `github_progress_counts_prs_and_reviews` | Batch counting logic |
| `jira_progress_counts_issues` | Batch counting logic |
| `discourse_progress_counts_topics_and_posts` | Batch counting logic |

#### 6B: Run Lifecycle

| Test | What it verifies |
| --- | --- |
| `create_run_stores_record` | Run row created with correct initial state |
| `complete_run_clears_invocation` | current_invocation_id set to NULL |
| `fail_run_stores_error` | Error message in run metadata |

**Estimated effort:** 10–12 test functions, ~500 LoC.

---

## Execution Order & Dependencies

```
Phase 1A (auth/config repos) ──┐
Phase 1B (org repo)            ├── Phase 2 (source adapters) ── Phase 6 (handlers)
Phase 1C (activity repo)  ─────┘         │
Phase 1D (metrics/reasoning repos)       │
                                         ├── Phase 4 (gRPC API expansion)
Phase 3 (metrics computation) ───────────┘
Phase 5 (unit tests) ── independent, can run in parallel with any phase
```

**Phase 1 must come first** — source adapter tests and API tests both need fixture builders and working repo tests as a foundation.

**Phase 5 is independent** — pure logic tests have no DB dependency and can be added at any point.

## Implementation Notes

### Running Tests

```bash
# Unit tests (no DB needed)
cargo test --lib

# Integration tests (requires DATABASE_URL)
DATABASE_URL=postgres://... cargo test -p ps-integration

# Specific phase
DATABASE_URL=postgres://... cargo test -p ps-integration repo::
DATABASE_URL=postgres://... cargo test -p ps-integration source::
DATABASE_URL=postgres://... cargo test -p ps-integration api::metrics
```

### pgvector Dependency

Reasoning repo tests for embeddings require the `pgvector` PostgreSQL extension. Tests should skip gracefully if the extension isn't available:

```rust
// At the start of embedding tests
let has_pgvector = sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'vector')")
    .fetch_one(&pool).await.unwrap();
if !has_pgvector.unwrap_or(false) {
    eprintln!("pgvector not available, skipping embedding test");
    return;
}
```

### Wiremock Response Files

Store canned API responses as JSON fixtures rather than inline strings:

```
tests/integration/fixtures/
├── github/
│   ├── graphql_prs.json
│   ├── graphql_member_search.json
│   ├── rest_team_repos.json
│   └── rest_rate_limit_low.json
├── jira/
│   ├── search_issues.json
│   ├── project_list.json
│   └── pagination_page2.json
└── discourse/
    ├── categories.json
    ├── topic_list.json
    └── topic_detail.json
```

### Test Isolation

Each test gets its own database (existing pattern). Source adapter tests additionally get their own wiremock server. This ensures full isolation — tests can run in parallel via `cargo test`.

### What NOT to Test

- Proto serialization (generated code, tested by prost/buf)
- sqlx query compilation (tested at compile time via `query!` macros)
- Restate SDK internals (runtime behaviour, journal replay)
- shadcn/ui component rendering
- Tonic server plumbing (connection handling, HTTP/2)

## Summary

| Phase | Tests (planned) | Tests (actual) | Status |
| --- | --- | --- | --- |
| 1 — Repos | ~50 | 86 | Done |
| 2 — Sources | ~32 | 28 | Done |
| 3 — Metrics | ~17 | 16 | Done |
| 4 — API | ~16 | 32 | Done |
| 5 — Unit | ~28 | 74 | Done |
| 6 — Handlers | ~11 | 23 | Done |
| **Total** | **~154** | **259** | **Complete** |

### Final test counts by crate

| Crate | Before | After |
| --- | --- | --- |
| ps-core | 35 | 65 |
| ps-workers | 12 | 54 |
| ps-metrics | 27 | 27 |
| ps-reasoning | 5 | 30 |
| ps-integration | ~22 | 190 |
| **Total** | **~97** | **366** |

### Deviations from plan

- **Phase 1 exceeded estimates** — org repo (15 vs ~11), activity repo (21 vs ~14), config repo (18 vs ~6) all had more methods worth testing than initially estimated.
- **Phase 4 exceeded estimates** — reasoning service (15 tests) covered more RPCs than the 4 originally planned. Admin tests covered API tokens, backup streaming, and reset lifecycle rather than user management (only admin role exists).
- **Phase 5 exceeded estimates** — enrichment helper tests (hash, preview, sanitize, input construction) and cost calculation tests added beyond the original scope.
- **Phase 6 adapted to architecture** — `finalise_run()` depends on Restate `ctx` so the three finalization outcome tests were replaced with tests for the pure functions it calls (`extract_watermark`, `extract_failed_items`) plus progress tracker tests. Run lifecycle tests (6B) are covered by repo::activity integration tests.
- **Some planned tests omitted** — catalogue response parsing (requires HTTP mocking, better suited to integration tests), model routing (requires instantiating provider clients), and some embedding/pgvector tests were deferred as they need external dependencies.
