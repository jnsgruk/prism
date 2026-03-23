# Plan 55 — Protobuf Style Guide Alignment

Align Prism's `.proto` files with the [Buf style guide](https://buf.build/docs/best-practices/style-guide) and the [files & packages reference](https://buf.build/docs/reference/protobuf-files-and-packages/). The current protos are already well-structured — services use `PascalCase` + `Service` suffix, enums have prefixed `UPPER_SNAKE_CASE` values with `_UNSPECIFIED` zero values, request/response messages follow `MethodNameRequest`/`MethodNameResponse`, and `STANDARD` lint rules are enabled. This plan addresses the remaining gaps.

## Current State

`buf lint` passes clean. The `PACKAGE_VERSION_SUFFIX` rule is excepted in `buf.yaml` despite `prism.v1` already containing a version suffix — this exception may be unnecessary and should be tested for removal.

## Gap Analysis

### P1 — String-typed fields that should be proto enums

The Rust codebase uses strong domain enums (`Platform`, `ContributionType`, `ContributionState`, `IngestionStatus`, `Role`), but the proto schema represents many of these as `string`. This is the biggest type-safety gap. Affected fields:

| Field | File | Current | Proposed Enum |
|---|---|---|---|
| `SourceConfig.source_type` | config.proto | `string` | `Platform` |
| `PlatformIdentity.platform` | org.proto | `string` | `Platform` |
| `Contribution.platform` | metrics.proto | `string` | `Platform` |
| `Contribution.contribution_type` | metrics.proto | `string` | `ContributionType` |
| `Contribution.state` | metrics.proto | `string` | `ContributionState` |
| `HandlerRun.status` | handlers.proto | `string` | `RunStatus` |
| `AiTaskConfig.provider` | reasoning.proto | `string` | `AiProvider` |
| `Enrichment.enrichment_type` | reasoning.proto | `string` | `EnrichmentType` |
| `ListTeamContributionsRequest.contribution_type` | metrics.proto | `string` | `ContributionType` |
| `ListTeamContributionsRequest.state` | metrics.proto | `string` | `ContributionState` |
| `ListTeamContributionsRequest.platform` | metrics.proto | `string` | `Platform` |
| `ListPersonContributionsRequest.platform` | metrics.proto | `string` | `Platform` |
| `ListPersonContributionsRequest.contribution_type` | metrics.proto | `string` | `ContributionType` |
| `ListPersonContributionsRequest.state` | metrics.proto | `string` | `ContributionState` |
| `ListPeopleRequest.filter` | org.proto | `string` | `PersonFilter` |
| `GetTeamInsightsRequest.period` | insights.proto | `string` | `InsightPeriod` |
| `GetPersonInsightsRequest.period` | insights.proto | `string` | `InsightPeriod` |
| `GetOrgInsightsRequest.period` | insights.proto | `string` | `InsightPeriod` |

**Risk:** This is a breaking API change. Every field type change from `string` to `enum` breaks wire compatibility. Must be done as a coordinated backend + frontend release.

**Decision:** In-place on `prism.v1` (no external API consumers — all clients are rebuilt together). The coordinated update must touch: proto definitions → `buf generate` → Rust `From`/`Into` conversions in services → TypeScript hooks and components. All changes ship as a single atomic commit or tightly sequenced PR.

### P2 — Inconsistent timestamp types

Some timestamp fields use `google.protobuf.Timestamp` while others use `string` (ISO 8601). The style guide recommends using well-known types.

| Field | File | Current | Fix |
|---|---|---|---|
| `Enrichment.created_at` | reasoning.proto | `string` | `google.protobuf.Timestamp` |
| `SimilarItem.created_at` | reasoning.proto | `string` | `google.protobuf.Timestamp` |
| `GetEnrichmentPipelineStatusResponse.last_enrichment_at` | reasoning.proto | `string` | `google.protobuf.Timestamp` |
| `GetEmbeddingStatusResponse.last_embedded_at` | reasoning.proto | `string` | `google.protobuf.Timestamp` |
| `ListAiModelsResponse.last_refreshed` | reasoning.proto | `map<string, string>` | `map<string, google.protobuf.Timestamp>` |

**Note:** Date-only fields (`Period.start`, `Period.end`, `ThroughputDataPoint.date`, etc.) are acceptable as `string` since there's no well-known `Date` type in proto3 without importing `google/type/date.proto`.

### P3 — Shared types in wrong file

`PaginationRequest`, `PaginationResponse`, and `SortOrder` are defined in `org.proto` but are conceptually shared infrastructure. If another service needed them, it would create an awkward cross-service import.

**Fix:** Extract to `proto/prism/v1/common.proto`. This is not a wire-breaking change — the package stays `prism.v1`, so the fully-qualified names don't change. Only the file-level import paths change.

### P4 — Package naming depth

The [files & packages reference](https://buf.build/docs/reference/protobuf-files-and-packages/) recommends at least 3 components: `<org>.<product>.<version>`. The current `prism.v1` has only 2.

**Decision:** Rename to `canonical.prism.v1`. This requires:
1. Restructure directories: `proto/prism/v1/` → `proto/canonical/prism/v1/`
2. Update `package` declaration in all 8 proto files
3. Update `buf.yaml` module path
4. Update `buf.gen.yaml` if paths are referenced
5. Regenerate all Rust and TypeScript code (`buf generate`)
6. Update any hardcoded package references in backend code (Connect service paths, interceptor routes)

### P5 — Documentation

The style guide recommends "over-document, and use complete sentences." Current protos have minimal documentation — a few inline field comments and section separators.

**What to add:**
- **File-level overview comment** on each `.proto` file (below syntax, above package) describing the service's responsibility
- **Service-level comment** documenting what the service manages
- **RPC-level comments** on every method (some already exist in insights.proto, reasoning.proto)
- **Message-level comments** on domain entities (`Team`, `Person`, `Contribution`, `Enrichment`, etc.)
- **Field-level comments** on non-obvious fields (many already exist, extend to all fields where the name alone isn't sufficient)

### P6 — Remove `PACKAGE_VERSION_SUFFIX` exception

The `buf.yaml` excepts `PACKAGE_VERSION_SUFFIX` but `prism.v1` already has a version suffix. Test removing the exception — if `buf lint` still passes, delete the exception.

### P7 — File organisation order

The style guide recommends: license header → file overview → syntax → package → imports → file options → everything else. Current files put syntax first (before any comments). Add file overview comments between syntax and package.

## Implementation Plan

### Phase 1 — Package rename + CI hardening

Do this first so all subsequent phases only need one round of `buf generate`.

1. **Rename package to `canonical.prism.v1`**
   - Create directory `proto/canonical/prism/v1/`
   - Move all `.proto` files from `proto/prism/v1/` to the new path
   - Update `package` declaration in all 8 proto files to `canonical.prism.v1`
   - Update `buf.yaml` module name to `buf.build/canonical/prism`
   - Run `buf generate` and verify all generated code paths are correct
   - Update any hardcoded Connect service paths in backend (e.g., `/prism.v1.AuthService/Login` → `/canonical.prism.v1.AuthService/Login`)
   - Update auth interceptor allow-list if it references package-qualified RPC names
   - Verify frontend Connect transport auto-discovers the renamed services
2. **Remove `PACKAGE_VERSION_SUFFIX` exception** from `buf.yaml` — verify `buf lint` still passes with the new package name
3. **Add `buf breaking` to CI** — run `buf breaking --against .git#branch=main` on every PR that touches proto files
4. **Full integration test** — backend + frontend rebuild, manual smoke test

### Phase 2 — Non-breaking improvements

1. **Extract shared types** to `common.proto` — move `PaginationRequest`, `PaginationResponse`, `SortOrder` out of `org.proto`, add import in `org.proto`
2. **Add file-level documentation** — overview comment on each proto file
3. **Add service and RPC documentation** — complete sentences on every service, every RPC, and non-obvious messages/fields
4. **Run `buf generate`** and rebuild both backend and frontend to verify

### Phase 3 — Timestamp consistency (minor breaking)

1. **Convert string timestamps to `google.protobuf.Timestamp`** in reasoning.proto (5 fields)
2. **Update Rust service code** — change serialisation for affected fields
3. **Update TypeScript consumers** — adjust any direct string parsing to use Timestamp helpers
4. **Run `buf generate`** and full test suite

### Phase 4 — Proto enums for domain types (breaking)

This is the largest change. Do it in sub-phases to limit blast radius.

#### 4a — Define enums in a shared file

Create `proto/prism/v1/enums.proto` (or add to `common.proto`) with:

```protobuf
// Platform is the base platform type. For platforms that support multiple
// instances (e.g. Discourse), the instance name is carried in a separate
// `platform_instance` field on the containing message.
enum Platform {
  PLATFORM_UNSPECIFIED = 0;
  PLATFORM_GITHUB = 1;
  PLATFORM_JIRA = 2;
  PLATFORM_DISCOURSE = 3;
  PLATFORM_LAUNCHPAD = 4;
  PLATFORM_GOOGLE_DRIVE = 5;
  PLATFORM_MAILING_LIST = 6;
}

enum ContributionType {
  CONTRIBUTION_TYPE_UNSPECIFIED = 0;
  CONTRIBUTION_TYPE_PULL_REQUEST = 1;
  CONTRIBUTION_TYPE_PR_REVIEW = 2;
  CONTRIBUTION_TYPE_JIRA_TICKET = 3;
  CONTRIBUTION_TYPE_DISCOURSE_TOPIC = 4;
  CONTRIBUTION_TYPE_DISCOURSE_POST = 5;
  // ... other types as needed
}

enum ContributionState {
  CONTRIBUTION_STATE_UNSPECIFIED = 0;
  CONTRIBUTION_STATE_OPEN = 1;
  CONTRIBUTION_STATE_MERGED = 2;
  CONTRIBUTION_STATE_CLOSED = 3;
  CONTRIBUTION_STATE_IN_PROGRESS = 4;
  CONTRIBUTION_STATE_DONE = 5;
  // ...
}

enum RunStatus {
  RUN_STATUS_UNSPECIFIED = 0;
  RUN_STATUS_RUNNING = 1;
  RUN_STATUS_COMPLETED = 2;
  RUN_STATUS_COMPLETED_WITH_WARNINGS = 3;
  RUN_STATUS_FAILED = 4;
  RUN_STATUS_CANCELLED = 5;
}

enum AiProvider {
  AI_PROVIDER_UNSPECIFIED = 0;
  AI_PROVIDER_GOOGLE = 1;
  AI_PROVIDER_OPENROUTER = 2;
}

enum EnrichmentType {
  ENRICHMENT_TYPE_UNSPECIFIED = 0;
  ENRICHMENT_TYPE_REVIEW_DEPTH = 1;
  ENRICHMENT_TYPE_SENTIMENT = 2;
  ENRICHMENT_TYPE_SIGNIFICANCE = 3;
  ENRICHMENT_TYPE_TOPIC = 4;
}

enum PersonFilter {
  PERSON_FILTER_UNSPECIFIED = 0;
  PERSON_FILTER_UNASSIGNED = 1;
  PERSON_FILTER_INACTIVE = 2;
}

enum InsightPeriod {
  INSIGHT_PERIOD_UNSPECIFIED = 0;
  INSIGHT_PERIOD_LAST_WEEK = 1;
  INSIGHT_PERIOD_LAST_MONTH = 2;
  INSIGHT_PERIOD_LAST_QUARTER = 3;
}
```

Messages that previously used `string` for platform with instance info (e.g., `"discourse-ubuntu"`) will split into two fields:

```protobuf
Platform platform = N;
// Instance identifier for multi-instance platforms (e.g., "ubuntu" for Discourse).
// Empty for single-instance platforms like GitHub.
optional string platform_instance = M;
```

This applies to: `Contribution`, `PlatformIdentity`, `PlatformIdentityInfo`, `PlatformActivitySummary`, `SimilarItem`, and filter fields on list requests.

#### 4b — Replace string fields with enum types

Update all affected message definitions across config.proto, org.proto, metrics.proto, handlers.proto, reasoning.proto, insights.proto.

#### 4c — Update Rust codegen and services

- Update `From`/`Into` conversions between domain enums and proto enums
- Update all service methods that construct proto responses
- Update all service methods that parse proto requests

#### 4d — Update TypeScript consumers

- Update all React Query hooks and components that use the affected fields
- Enum values in TypeScript will be numeric — may need display name mapping utilities

#### 4e — Verify

- `buf lint` clean
- `buf breaking --against .git#branch=main` will show breakages (expected, document)
- Full backend test suite passes
- Full frontend test suite passes
- Manual smoke test of all affected UI flows

### Phase 5 — Date types + language options

1. **Add `google/type/date.proto`** for date-only fields — replace `string` date fields in `Period.start`, `Period.end`, `ThroughputDataPoint.date`, `WipDataPoint.date`, `DailySpend.date`, `DiscourseActivityDataPoint.date` with `google.type.Date`
2. **Add language options** — set `go_package` and `java_package` for future-proofing, even if not currently consumed

## Streaming RPCs — Accepted Exception

The style guide says "avoid streaming RPCs." Prism uses streaming for backup/restore (`CreateBackup`, `PreviewBackup`, `RestoreBackup`) which transfer multi-megabyte files as chunked bytes. This is a legitimate use case — polling/pagination doesn't apply to file transfers. Document this as an accepted exception.

## CLAUDE.md Additions

After completing the work, add the following to the `Proto & Code Generation` section of CLAUDE.md:

```markdown
### Proto Style Rules (Buf Style Guide)

Follow the [Buf style guide](https://buf.build/docs/best-practices/style-guide). Key rules enforced by convention:

- **Package**: `canonical.prism.v1` — all files share the same package
- **File layout order**: syntax → package → imports (sorted alphabetically) → file options → service → enums → messages
- **Services**: `PascalCase` with `Service` suffix (e.g., `ConfigService`)
- **RPCs**: `PascalCase` verb-noun (e.g., `CreateTeam`, `ListPeople`)
- **Request/Response**: every RPC gets its own `MethodNameRequest`/`MethodNameResponse` — never use `google.protobuf.Empty`
- **Messages**: `PascalCase` — domain entities (`Team`), info types (`ApiTokenInfo`), summaries (`ReviewQualitySummary`)
- **Fields**: `lower_snake_case` — repeated fields must be plural, singular for single-value fields
- **Enums**: `PascalCase` name, `UPPER_SNAKE_CASE` values prefixed with enum name, zero value `_UNSPECIFIED` (e.g., `TEAM_TYPE_UNSPECIFIED = 0`)
- **Use domain enums, not strings** — if a field has a fixed set of valid values, define a proto enum. Mirror Rust domain enums in proto
- **Timestamps**: use `google.protobuf.Timestamp` — never `string` for datetime fields. Date-only fields (`YYYY-MM-DD`) may use `string`
- **Shared types**: common infrastructure messages (`PaginationRequest`, `SortOrder`) live in `common.proto`
- **No nested enums or messages** — define all types at the top level
- **No public/weak imports, no `allow_alias`**
- **Documentation**: every service, RPC, domain entity message, and non-obvious field must have a `//` comment using complete sentences. Over-document
- **Streaming RPCs**: avoid unless transferring large binary payloads (backup/restore is an accepted exception)
- **Comments**: use `//` only, never `/* */`

The `buf.yaml` uses `STANDARD` lint rules. Run `buf lint` before committing proto changes.
```

## Effort Estimate

| Phase | Scope | Risk |
|---|---|---|
| Phase 1 | Package rename to `canonical.prism.v1` + CI hardening | Medium — all service paths change, needs full integration test |
| Phase 2 | Documentation + shared types extraction + lint fix | None — no wire changes |
| Phase 3 | 5 timestamp fields in reasoning.proto | Low — localised to one service |
| Phase 4 | ~18 string→enum fields + platform/instance split across 6 files + all services + all frontend hooks | High — coordinated full-stack change |
| Phase 5 | `google.type.Date` for date fields + language options | Low — localised type changes |

## Resolved Decisions

1. **In-place break** — no `v2` package. All clients are rebuilt together, so breaking changes on `canonical.prism.v1` are acceptable.
2. **3-component package name** — rename to `canonical.prism.v1` with full directory restructure.
3. **All phases are mandatory** — `buf breaking` in CI, `google.type.Date` for date fields, and language options are part of the plan.
4. **`PACKAGE_VERSION_SUFFIX` exception** — test removal in Phase 1.
5. **Platform enum as base type** — `Platform` enum covers the base platform (GitHub, Jira, Discourse, etc.). Instance names (e.g., `"ubuntu"` for `discourse-ubuntu`) are carried in a separate `optional string platform_instance` field.
6. **Package rename first** — do the `canonical.prism.v1` rename in Phase 1 so all subsequent phases only regenerate once.

## Open Questions

None — all resolved.
