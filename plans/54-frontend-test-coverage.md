# Plan 54 — Frontend Test Coverage

**Status:** In Progress — Phase 3 complete
**Created:** 2026-03-23

## Problem Statement

The frontend has 2 test files with 8 test cases across ~150 source files spanning utilities, hooks, components, and pages — effective test coverage is near **0%**.

This plan proposes a phased approach to build meaningful frontend test coverage, prioritising business logic and data transformation over presentational rendering.

---

## Current State Assessment

### Test Infrastructure

| Aspect | Status | Notes |
| --- | --- | --- |
| Framework | Vitest 4.1.0 | Correct choice, fast |
| DOM environment | happy-dom 20.8.3 | Lightweight, but globals not initialising |
| Component testing | @testing-library/react 16.3.2 | Installed, unused due to env bug |
| Assertion matchers | @testing-library/jest-dom | Imported in setup file |
| API mocking | `createRouterTransport` (Connect) | Type-safe, in-memory — excellent pattern |
| React Query | Fresh `QueryClient` per test, `retry: false` | Correct approach in `test-wrapper.tsx` |

**Blocking issue:** `vitest.setup.ts` only imports jest-dom matchers. happy-dom is configured as the environment but `document`, `window`, and `localStorage` globals are not available at test time. All 8 existing tests fail with `ReferenceError: document is not defined`.

### Existing Test Files

| File | Tests | Status | What it covers |
| --- | --- | --- | --- |
| `lib/session.test.ts` | 3 | Failing | localStorage get/set/clear |
| `views/ingestion/pages/ingestion-page.test.tsx` | 5 | Failing | Source rendering, run history, controls |
| `views/ingestion/pages/test-wrapper.tsx` | — | Helper | QueryClient + SidebarProvider wrapper |

### Untested Code Inventory

**~150 source files** across these categories:

| Category | Files | Testable Logic | Priority |
| --- | --- | --- | --- |
| **Formatting utilities** (`lib/format.ts`, `lib/format-metrics.ts`) | 2 | ~12 pure functions: timestamps, durations, relative time, metric formatting, em-dash fallbacks | **Critical** |
| **Ingestion progress** (`views/ingestion/lib/progress.ts`) | 1 | JSON parsing, phase-specific normalisation (GitHub/Jira/Discourse), detail extraction | **Critical** |
| **Run status mapping** (`lib/run-status.ts`) | 1 | Status → variant/icon/label mapping | High |
| **Admin utilities** (`views/admin/lib/source-types.ts`, `team-utils.ts`) | 2 | Source type normalisation, tree flattening | High |
| **Shared hooks** (`lib/hooks/`) | 6 | Auth flows, config CRUD, metrics queries, debounce, embeddings | High |
| **Feature hooks** (`views/*/hooks/`) | 10 | Ingestion control, team tree traversal, insights period mapping, AI settings | High |
| **Visualisation components** (delta-badge, sentiment-bar, coverage-indicator, etc.) | 8 | Percentage calculations, colour mapping, format switching, zero-division guards | Medium |
| **Data table** (`components/data-table/`) | 2 | Sorting, pagination, row click handlers | Medium |
| **Period selector** (`views/teams/components/period-selector.tsx`) | 1 | Date arithmetic for period bounds | Medium |
| **Enrichment badge** (`components/enrichment-badge.tsx`) | 1 | JSON parsing, type-specific label extraction, variant selection | Medium |
| **Feature pages** (`views/*/pages/`) | 9 | Full integration: data fetching, rendering, interactions | Low |
| **Layout components** (app-shell, sidebar, page-header) | 3 | Minimal logic, mostly presentational | Skip |
| **shadcn/ui components** (`components/ui/`) | 26 | Third-party primitives | Skip |

### What NOT to Test (per CLAUDE.md)

- shadcn/ui primitives
- Chart SVG output (Recharts)
- React Router configuration
- CSS / Tailwind classes
- Generated protobuf code (`lib/api/gen/`)

---

## Phased Plan

### Phase 0 — Fix Test Infrastructure [COMPLETE]

**Goal:** All existing tests pass. New tests can be written without fighting the environment.

**What was done:**

1. **Verified happy-dom environment works** — the test infrastructure was functional all along. The earlier `document is not defined` errors only occurred when running `bun test` (Bun's built-in test runner) instead of `bunx vitest run` / `bun run test`. Vitest + happy-dom provides DOM globals correctly.

2. **Promoted `TestWrapper` to shared test utility** — created `lib/test-utils.tsx` exporting:
   - `createTestQueryClient()` — fresh QueryClient with `retry: false`
   - `TestWrapper` — QueryClientProvider + SidebarProvider wrapper
   - `renderWithProviders()` — custom render that auto-wraps in TestWrapper
   - `setupCleanup()` — call in describe() to auto-cleanup after each test

3. **Updated ingestion page test** to import from `@ps/test-utils` instead of the local `test-wrapper.tsx`. Removed the local `test-wrapper.tsx`.

4. **Mock transport factory deferred** — the `createRouterTransport` inline pattern is clear and type-safe. A factory would add indirection without saving much code. Revisit if a pattern emerges after Phase 2.

**Result:** 8/8 tests pass. Shared utilities available at `@ps/test-utils`.

---

### Phase 1 — Pure Function Unit Tests [COMPLETE]

**Goal:** Cover all formatting utilities and data transformation functions. These are the highest-value, lowest-effort tests — pure functions with no React or async dependencies.

**Target files and expected test cases:**

#### `lib/format.ts` (~15 tests)
- `formatTimestamp()` — null input → em-dash, valid timestamp → "Mar 16 14:30" (24h), timezone handling
- `formatFullTimestamp()` — full locale string output
- `formatDateOnly()` — "Mar 16, 2026" format
- `formatDuration()` — start/end → "2m 15s", null inputs → em-dash, zero duration
- `formatRelativeTime()` — "5m ago", "2h ago", "1d ago", >30 days falls back to date
- `formatRelativeTimeIso()` — ISO string input variant

#### `lib/format-metrics.ts` (~10 tests)
- `fmtHours()` — number → "Xh", null/undefined → em-dash
- `fmtFloat()` — one decimal place formatting
- `fmtPercent()` — "42%" format, rounding
- `instanceLabel()` — "discourse-ubuntu" → "Ubuntu", edge cases

#### `views/ingestion/lib/progress.ts` (~12 tests)
- `parseProgress()` — valid JSON, invalid JSON, null input
- `normaliseProgress()` — GitHub team_repos phase, member_search phase, complete phase, generic fallback, zero totals
- `extractDetail()` — rate limit info extraction, PR/review counts, skipped identities, conditional field inclusion

#### `lib/run-status.ts` (~5 tests)
- Status config mapping for each status value
- Icon and variant selection

#### `views/admin/lib/source-types.ts` (~4 tests)
- `baseSourceType()` — "discourse-ubuntu" → "discourse", "github" → "github"
- Secret keys lookup by type

#### `views/admin/lib/team-utils.ts` (~4 tests)
- `flattenTeams()` — empty tree, single level, nested tree, depth tracking

#### `views/teams/hooks/use-teams.ts` (pure utility exports, ~8 tests)
- `flattenTree()` — recursive flattening with depth
- `findTeam()` — find by ID, missing ID returns undefined
- `getAncestors()` — breadcrumb path computation
- `teamTypeLabel()` — enum → label mapping
- `teamTypeBadgeVariant()` — enum → badge variant

#### `views/teams/hooks/use-insights.ts` (pure function export, ~5 tests)
- `periodKeyToInsightsPeriod()` — "1w"→"last_week", "1m"→"last_month", "1q"→"last_quarter", "1y"→"last_year", default case

**Exit criteria:** ~60 tests covering all pure utility functions. All pass in CI.

**Estimated scope:** ~10 new test files colocated with their source files.

**Result:** 108 test cases across 8 test files. All pass. Also fixed `renderWithProviders` return type in `test-utils.tsx` (removed non-portable `RenderResult` import, used `ReturnType<typeof render>`).

---

### Phase 2 — React Hook Tests [COMPLETE]

**Goal:** Test hooks that contain meaningful logic beyond simple React Query wrappers. Focus on query key construction, conditional enables, mutation side effects (invalidation chains, token storage), and timeout-based patterns.

**Approach:** Use `@testing-library/react`'s `renderHook` with `TestWrapper` for provider context. Mock the Connect transport at module level (same pattern as `ingestion-page.test.tsx`).

#### Shared hooks (`lib/hooks/`)

**`use-debounced-value.ts`** (~4 tests)
- Returns initial value immediately
- Updates after delay
- Resets timer on rapid changes
- Cleanup on unmount

**`use-auth.ts`** (~8 tests)
- `useSetupStatus()` — query key, returns setup state
- `useCurrentUser()` — no retry on failure, query key
- `useLogin()` — on success: stores token, invalidates all queries
- `useLogout()` — clears token, clears query cache
- `useCompleteSetup()` — stores token, invalidates setup status

**`use-config.ts`** (~6 tests)
- `useGetSource()` — disabled when sourceId is empty
- `useCreateSource()` — invalidates source list on success
- `useDeleteSource()` — invalidates source list on success
- `useSetSecret()` — invalidation chain
- `useTestConnection()` — mutation fires correctly

#### Feature hooks

**`views/ingestion/hooks/use-ingestion.ts`** (~6 tests)
- `useIngestionStatus()` — configurable refetch interval
- `useTriggerRun()` — invalidates status + runs on success
- `useCancelRun()` — invalidates status + runs on success
- `useListRuns()` — optional sourceName filter in query key

**`views/admin/hooks/use-ai-settings.ts`** (~4 tests)
- `useSetProviderSecret()` — delayed model catalogue invalidation (setTimeout pattern)
- `useRefreshModelCatalogue()` — delayed invalidation after refresh
- `useStorageHealth()` — 60s refetch interval

**`views/admin/hooks/use-admin.ts`** (~6 tests)
- Team CRUD mutations — correct invalidation targets
- Person CRUD mutations — invalidation of people + team queries
- `useResetData()` — invalidates everything

**Exit criteria:** ~35 hook tests covering auth flows, invalidation chains, conditional enables, and timing patterns.

**Estimated scope:** ~8 new test files.

**Result:** 60 test cases across 6 test files. All pass. Tests cover query key construction, conditional enables (`useGetSource` disabled on empty ID), mutation success callbacks (token storage, query invalidation), timer-based debouncing, and all CRUD operations across auth, config, ingestion, AI settings, and admin hooks.

---

### Phase 3 — Component Integration Tests [COMPLETE]

**Goal:** Test components that contain meaningful data transformation, conditional rendering logic, or user interaction handling. Not testing visual appearance — testing behaviour.

#### Visualisation components with logic

**`components/delta-badge.tsx`** (~8 tests)
- Zero delta → returns null
- Positive delta → green with "+" prefix
- Negative delta → red
- Inversion logic (higher-is-worse metrics)
- Format switching: decimal (2dp), percent (rounded), integer
- Suffix appended correctly

**`components/coverage-indicator.tsx`** (~5 tests)
- Empty input → null
- Zero eligible → no division error
- Percentage calculation correctness
- Type label mapping (review_depth → "Review depth")

**`components/sentiment-bar.tsx`** (~5 tests)
- All zeros → null
- Single category → 100% width
- Zero-count segments omitted
- Percentage calculation across segments

**`components/enrichment-badge.tsx`** (~8 tests)
- JSON parsing of valueJson
- review_depth type → "X/5" label
- sentiment type → variant mapping (hostile → destructive, constructive → default)
- significance type → variant mapping
- Confidence percentage calculation
- Invalid JSON handling

**`components/depth-histogram.tsx`** (~4 tests)
- Height normalisation relative to max value
- Colour assignment by depth level
- Zero-count bars get minimum height (4%)

**`components/significance-breakdown.tsx`** (~4 tests)
- All zeros → null
- Percentage calculation
- Zero segments filtered out

#### Interactive components

**`components/data-table/data-table.tsx`** (~6 tests)
- Renders column headers
- Sort handler calls onSortingChange
- Row click fires callback
- Empty state message
- Manual sorting mode passes through

**`views/teams/components/period-selector.tsx`** (~5 tests)
- `buildPeriod()` returns correct date ranges for each preset
- Default period key is "1m"
- Date arithmetic edge cases (month boundaries)

**Exit criteria:** ~45 component tests covering data transformation and interaction logic.

**Estimated scope:** ~10 new test files colocated with components.

**Result:** 54 test cases across 8 test files. All pass. Tests cover null-return edge cases, data transformation (percentage calculation, normalisation, color mapping), interactive behaviour (row clicks, sort toggling), format switching, and proto object construction for period selector.

---

### Phase 4 — Page-Level Integration Tests

**Goal:** Test key pages end-to-end with mocked API responses. These are the most expensive tests to write and maintain, so focus on the pages with the most logic and user interaction.

**Approach:** Same pattern as the existing `ingestion-page.test.tsx` — mock full service responses via `createRouterTransport`, render the page in `TestWrapper`, assert on rendered output and interaction results.

#### Priority pages

**Fix and expand `views/ingestion/pages/ingestion-page.test.tsx`** (~10 tests total)
- Fix existing 5 tests (blocked on Phase 0)
- Add: "Run All" button disabled when all sources running
- Add: backfill dialog opens and shows date picker
- Add: status polling updates reflected
- Add: error state rendering for failed sources
- Add: AI pipeline section with enrichment counts

**`views/admin/pages/admin-page.tsx`** (~10 tests)
- Tab switching between Sources, Teams, People, AI, API Tokens, System
- Source creation dialog flow
- Team CRUD operations
- API token creation and revocation
- Reset data confirmation dialog

**`views/teams/pages/teams-page.tsx`** (~8 tests)
- Period selector changes trigger data refetch
- Team tree navigation
- Comparison table sorting
- Drill-down to team detail panel

**`views/login/pages/login-page.tsx`** (~4 tests)
- Form submission calls login mutation
- Error message on invalid credentials
- Redirect on successful login

**`views/dashboard/pages/dashboard-page.tsx`** (~5 tests)
- Metric cards render with data
- Empty state when no data
- Period selection

**Exit criteria:** ~37 page tests covering critical user flows.

**Estimated scope:** ~5 new test files, 1 expanded.

---

## Prioritisation Summary

| Phase | Tests | Effort | Value | Dependencies |
| --- | --- | --- | --- | --- |
| **0 — Fix infrastructure** | 0 new | Small | Unblocks everything | None |
| **1 — Pure functions** | ~60 | Small | High — covers core business logic | Phase 0 |
| **2 — Hooks** | ~35 | Medium | High — covers data flow and side effects | Phase 0 |
| **3 — Components** | ~45 | Medium | Medium — covers data transformation in UI | Phase 0 |
| **4 — Pages** | ~37 | Large | Medium — covers integration, high maintenance cost | Phases 0–3 |

**Total: ~177 tests** across ~34 test files.

Phases 1 and 2 can run in parallel after Phase 0. Phase 3 can start once the component testing patterns are established. Phase 4 should be last — page tests are the most brittle and benefit from having well-tested lower layers.

---

## Testing Patterns to Establish

### 1. Pure function tests (Phase 1)

```typescript
// lib/format.test.ts — colocated with source
import { describe, it, expect } from "vitest";
import { formatDuration } from "./format";

describe("formatDuration", () => {
  it("returns em-dash for null start", () => {
    expect(formatDuration(null, someEnd)).toBe("—");
  });

  it("formats minutes and seconds", () => {
    expect(formatDuration(start, fiveMinutesLater)).toBe("5m 0s");
  });
});
```

### 2. Hook tests (Phase 2)

```typescript
// lib/hooks/use-auth.test.ts
import { renderHook, waitFor } from "@testing-library/react";
import { TestWrapper } from "@ps/test-utils";

// Mock transport at module level
vi.mock("@ps/api/transport", () => ({ transport: createRouterTransport(...) }));

describe("useLogin", () => {
  it("stores token and invalidates queries on success", async () => {
    const { result } = renderHook(() => useLogin(), { wrapper: TestWrapper });
    result.current.mutate({ username: "admin", password: "pass" });
    await waitFor(() => expect(getSessionToken()).toBe("returned-token"));
  });
});
```

### 3. Component tests (Phase 3)

```typescript
// components/delta-badge.test.tsx
import { render, screen } from "@testing-library/react";
import { DeltaBadge } from "./delta-badge";

describe("DeltaBadge", () => {
  it("returns null for zero delta", () => {
    const { container } = render(<DeltaBadge delta={0} />);
    expect(container.firstChild).toBeNull();
  });

  it("shows positive delta in green with plus prefix", () => {
    render(<DeltaBadge delta={1.5} format="decimal" />);
    expect(screen.getByText("+1.50")).toBeInTheDocument();
  });
});
```

### 4. Page tests (Phase 4)

Follow the existing `ingestion-page.test.tsx` pattern: mock full service at module level, dynamic import the page, render in `TestWrapper`, wait for data to appear, assert on UI state and interactions.

---

## Files to Create / Modify

### Phase 0 [DONE]
- **Created:** `frontend/lib/test-utils.tsx` — shared TestWrapper, renderWithProviders, setupCleanup
- **Modified:** `frontend/views/ingestion/pages/ingestion-page.test.tsx` — uses `@ps/test-utils`
- **Removed:** `frontend/views/ingestion/pages/test-wrapper.tsx` — replaced by shared utility

### Phase 1 (new test files, colocated)
- `frontend/lib/format.test.ts`
- `frontend/lib/format-metrics.test.ts`
- `frontend/lib/run-status.test.ts`
- `frontend/views/ingestion/lib/progress.test.ts`
- `frontend/views/admin/lib/source-types.test.ts`
- `frontend/views/admin/lib/team-utils.test.ts`
- `frontend/views/teams/hooks/use-teams.test.ts` (pure utility exports only)
- `frontend/views/teams/hooks/use-insights.test.ts` (pure function export only)

### Phase 2 (new test files, colocated)
- `frontend/lib/hooks/use-debounced-value.test.ts`
- `frontend/lib/hooks/use-auth.test.ts`
- `frontend/lib/hooks/use-config.test.ts`
- `frontend/views/ingestion/hooks/use-ingestion.test.ts`
- `frontend/views/admin/hooks/use-ai-settings.test.ts`
- `frontend/views/admin/hooks/use-admin.test.ts`

### Phase 3 (new test files, colocated)
- `frontend/components/delta-badge.test.tsx`
- `frontend/components/coverage-indicator.test.tsx`
- `frontend/components/sentiment-bar.test.tsx`
- `frontend/components/enrichment-badge.test.tsx`
- `frontend/components/depth-histogram.test.tsx`
- `frontend/components/significance-breakdown.test.tsx`
- `frontend/components/data-table/data-table.test.tsx`
- `frontend/views/teams/components/period-selector.test.tsx`

### Phase 4 (new + modified test files)
- **Modify:** `frontend/views/ingestion/pages/ingestion-page.test.tsx`
- `frontend/views/admin/pages/admin-page.test.tsx`
- `frontend/views/teams/pages/teams-page.test.tsx`
- `frontend/views/login/pages/login-page.test.tsx`
- `frontend/views/dashboard/pages/dashboard-page.test.tsx`

---

## Open Questions

1. **happy-dom vs jsdom** — happy-dom is faster but the current version may have compatibility issues with RTL. Should we switch to jsdom if the fix isn't straightforward?
2. **Coverage thresholds** — should we set a minimum coverage threshold in vitest.config.ts after Phase 2? A target like 60% for `lib/` and `views/*/lib/` would catch regressions without being burdensome.
3. **Snapshot tests** — deliberately excluded from this plan. They add maintenance burden without catching logic bugs. Confirm this is the right call.
4. **CI test time budget** — with ~177 tests and happy-dom, expect <10s total runtime. No parallelisation or sharding needed at this scale.
