# Plan 31 — Replace stringly-typed fields with Rust enums

## Problem

Several core domain fields are stored as `String` in Rust models despite having a small, fixed set of valid values. This means invalid values can only be caught at runtime (or not at all), match arms need catch-all branches that hide bugs, and every consumer has to agree on magic strings like `"github"` or `"merged"`.

`TeamType` already demonstrates the correct pattern — a `#[derive(sqlx::Type)]` enum with a custom Postgres type. This plan extends that pattern to the remaining stringly-typed fields.

## Audit findings

| Field | Model / location | Current type | Known values | Priority |
|---|---|---|---|---|
| `platform` | `Contribution`, `PlatformIdentity`, `ContributionInput` | `String` | `github`, `launchpad` | **Critical** — used across repos, services, workers, metrics |
| `source_type` | `SourceConfig` | `String` | `github` (jira config exists but no source impl) | **High** — overlaps with platform; used in registry dispatch + secret validation |
| `contribution_type` | `Contribution`, `ContributionInput` | `String` | `pull_request`, `pr_review` | **High** — used in metrics filtering |
| `state` | `Contribution`, `ContributionInput` | `Option<String>` | `open`, `closed`, `merged` (PRs); `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, `PENDING`, `DISMISSED` (reviews — from GitHub API) | **High** — used in metrics filtering and DORA calculations |
| `status` | `IngestionRun` | `String` | `running`, `completed`, `failed`, `cancelled` | **Medium** — used in repo queries |
| `period_type` | `SnapshotInput`, `TeamSnapshotRow` | `String` | `week`, `month`, `quarter` | **Medium** — proto enum already exists, Rust enum missing |
| `level` | `Person` | `Option<String>` | User-defined, not enumerable | **Skip** — genuinely free-form |

## Design decisions

### 1. Platform vs SourceType

`platform` and `source_type` are closely related but not identical:
- **Platform** = the external system a piece of data came from (GitHub, Jira, Launchpad, etc.)
- **SourceType** = the configured integration type, which today maps 1:1 to platform

Keep them as a **single enum `Platform`** for now with only the platforms that are actually implemented (`Github`, `Launchpad`). New variants get added when a source is built, not before. If source types diverge from platforms in future (e.g. "github-enterprise" as a distinct source type), we can split then — but today they're the same values and having two enums would just add conversion boilerplate.

### 2. Contribution state complexity

Different platforms use different state vocabularies. Two options:
- **(a) Single `ContributionState` enum** with the union of all values, where each contribution type uses a subset.
- **(b) Typed per-contribution-type** — different enums for PR state vs review state.

Go with **(a)** — a single enum. The state field is already a flat column in the DB, and metrics code filters on it uniformly. We can add `#[serde(rename_all = "snake_case")]` and normalise the casing (e.g. `APPROVED` → `Approved`).

### 3. Postgres custom types vs TEXT columns

`TeamType` uses a Postgres custom enum type (`org.team_type`). We have two options for the new enums:
- **(a) Postgres custom types** — DB enforces validity, but requires a migration per new variant.
- **(b) Keep TEXT columns** — Rust enum handles ser/de via `sqlx::Type` with `rename_all`, stored as text. No migration needed for new variants.

Go with **(b) — TEXT columns with Rust-side enforcement**. The Postgres enum approach requires `ALTER TYPE ... ADD VALUE` migrations that can't run inside transactions, which is painful. Rust's type system catches invalid values at compile time in application code, and the DB already has the data as TEXT. We'll use `sqlx::Type` with `type_name = "TEXT"` or implement `sqlx::Encode`/`sqlx::Decode` manually for TEXT round-tripping.

### 4. Proto enums

The proto definitions currently use `string` for most of these fields. We should add proto enum definitions for the critical ones (`Platform`, `ContributionType`, `ContributionState`) so the frontend also gets type safety. `PeriodType` already has a proto enum.

## Implementation plan

### Phase 1 — Define enums in `ps-core/src/models/` (no migration needed)

**Step 1.1: `Platform` enum**

```rust
// models/mod.rs or models/platform.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Github,
    Launchpad,
}

impl fmt::Display for Platform { ... }

// For sqlx TEXT round-tripping:
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Platform { ... }
impl sqlx::Encode<'_, sqlx::Postgres> for Platform { ... }
impl sqlx::Type<sqlx::Postgres> for Platform {
    fn type_info() -> PgTypeInfo { PgTypeInfo::with_name("TEXT") }
}
```

**Step 1.2: `ContributionType` enum**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionType {
    PullRequest,
    PrReview,
}
```

**Step 1.3: `ContributionState` enum**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionState {
    // PR states
    Open,
    Closed,
    Merged,
    // Review states (from GitHub API)
    Approved,
    ChangesRequested,
    Commented,
    Pending,
    Dismissed,
}
```

**Step 1.4: `IngestionStatus` enum**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestionStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}
```

**Step 1.5: `PeriodType` enum**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeriodType {
    Week,
    Month,
    Quarter,
}
```

### Phase 2 — Update models to use enums

Update struct definitions:
- `Contribution`: `platform: String` → `platform: Platform`, `contribution_type: String` → `contribution_type: ContributionType`, `state: Option<String>` → `state: Option<ContributionState>`
- `ContributionInput`: same fields
- `PlatformIdentity`: `platform: String` → `platform: Platform`
- `SourceConfig`: `source_type: String` → `source_type: Platform`
- `IngestionRun`: `status: String` → `status: IngestionStatus`
- `SnapshotInput` / `TeamSnapshotRow`: `period_type: String` → `period_type: PeriodType`

### Phase 3 — Update repo layer

Replace all hardcoded string literals in SQL bind parameters:
- `"github"` → `Platform::Github`
- `"running"` → `IngestionStatus::Running`
- `"pull_request"` → `ContributionType::PullRequest`
- etc.

For `sqlx::query!` macros, the TEXT columns will accept the enum values via the `Encode` impl. No schema changes needed.

Key files:
- `crates/ps-core/src/repo/activity.rs` — ingestion status updates, contribution queries
- `crates/ps-core/src/repo/metrics.rs` — period_type, contribution_type, state filtering
- `crates/ps-core/src/repo/org/people.rs` — platform identity queries

### Phase 4 — Update services and workers

Replace string literals and comparisons:
- `crates/ps-server/src/services/config.rs` — source type validation (`"github" | "jira"` → match on `Platform`)
- `crates/ps-server/src/services/metrics.rs` — period_type parse/convert functions (replace with `From` impls)
- `crates/ps-server/src/services/org.rs` — platform string comparisons
- `crates/ps-workers/src/registry.rs` — source dispatch (match on `Platform` instead of `&str`)
- `crates/ps-workers/src/github/source.rs` — contribution creation (use enum variants instead of `.into()`)
- `crates/ps-metrics/src/lib.rs` — contribution_type and state filtering

### Phase 5 — Update proto definitions

Add enum types to proto and update message fields:

```protobuf
// metrics.proto — add alongside existing PeriodType
enum Platform {
  PLATFORM_UNSPECIFIED = 0;
  PLATFORM_GITHUB = 1;
  PLATFORM_LAUNCHPAD = 2;
}

enum ContributionType {
  CONTRIBUTION_TYPE_UNSPECIFIED = 0;
  CONTRIBUTION_TYPE_PULL_REQUEST = 1;
  CONTRIBUTION_TYPE_PR_REVIEW = 2;
}

enum ContributionState {
  CONTRIBUTION_STATE_UNSPECIFIED = 0;
  CONTRIBUTION_STATE_OPEN = 1;
  CONTRIBUTION_STATE_CLOSED = 2;
  CONTRIBUTION_STATE_MERGED = 3;
  CONTRIBUTION_STATE_APPROVED = 4;
  CONTRIBUTION_STATE_CHANGES_REQUESTED = 5;
  CONTRIBUTION_STATE_COMMENTED = 6;
  CONTRIBUTION_STATE_PENDING = 7;
  CONTRIBUTION_STATE_DISMISSED = 8;
}
```

Update message fields from `string` to the enum types. Add `From`/`Into` conversion impls between proto enums and domain enums in the service layer.

### Phase 6 — Update frontend

After `buf generate`, update TypeScript code to use the generated enum types instead of string comparisons. The Connect clients will automatically use the new enum types.

## Ordering and risk

- **Phases 1–4 are purely backend** and can be done without any proto/frontend changes by keeping the sqlx TEXT encoding. This is the safest path.
- **Phase 5 is a proto breaking change** — existing string fields become enums. This should be coordinated with a frontend release.
- **Phase 6 follows naturally** from phase 5.

Recommend doing phases 1–4 first as a single PR, then phases 5–6 as a follow-up.

## Not in scope

- `Person.level` — genuinely free-form, not enumerable
- `Repository.primary_language` — too many possible values for an enum
- `Repository.default_branch` — free-form string
- Restate handler states (`pending`, `ready`, `suspended`, `backing-off`) — these come from the Restate framework, not our domain
