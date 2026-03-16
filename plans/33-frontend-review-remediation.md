# Plan 33 — Frontend Review Remediation

_Source: `reports/deep-review-frontend.md` (2026-03-16)_

Address high and medium severity findings from the frontend deep review. Low-severity items are captured at the end for opportunistic cleanup but are not primary targets.

---

## WS-1: Extract Shared Run/Handler Components (high)

**Problem:** `views/admin/components/handlers-tab.tsx` and `views/ingestion/components/ingestion-runs-table.tsx` share near-identical `StatusStyle`, `statusConfig`, formatting functions, `RunDetailDialog`, and filter/pagination logic.

**Approach:**
1. Create `lib/run-utils.ts` — export `StatusStyle` type, `defaultStatus`, `statusConfig` record
2. Create `lib/format.ts` — export `formatTimestamp`, `formatFullTimestamp`, `formatDuration`, `formatRelativeTime` (also consumed by `source-status-card.tsx` and `api-tokens-tab.tsx`)
3. Create `components/run-detail-dialog.tsx` — shared `RunDetailDialog` component parameterized by column config
4. Refactor both consumers to import from the shared modules
5. Verify both views render identically after extraction

**Files:**
- `frontend/views/admin/components/handlers-tab.tsx`
- `frontend/views/ingestion/components/ingestion-runs-table.tsx`
- `frontend/views/ingestion/components/source-status-card.tsx` (duplicate `formatTimestamp`/`formatRelativeTime`)
- `frontend/views/admin/components/api-tokens-tab.tsx` (duplicate timestamp formatting)
- New: `frontend/lib/format.ts`
- New: `frontend/lib/run-utils.ts`
- New: `frontend/components/run-detail-dialog.tsx`

---

## WS-2: Fix Render-Phase Side Effects (medium)

**Problem:** `navigate()` called during render in `app-shell.tsx:52-61` and `login-page.tsx:23-26`. Violates React patterns, causes warnings.

**Approach:**
1. Wrap both redirect blocks in `useEffect` with appropriate deps
2. Verify navigation still works correctly for unauthenticated → login and setup-required → setup flows

**Files:**
- `frontend/components/app-shell.tsx`
- `frontend/views/login/pages/login-page.tsx`

---

## WS-3: Coordinate Mutations in PersonDetailDialog (medium)

**Problem:** `people-tab.tsx:245-270` fires multiple independent mutations without coordinating completion. Dialog closes before mutations resolve; errors silently lost.

**Approach:**
1. Await all mutations with `Promise.all` (or sequential if order matters)
2. Close dialog only after all resolve
3. Show toast error if any mutation fails
4. Disable submit button while mutations are in flight

**Files:**
- `frontend/views/admin/components/people-tab.tsx`

---

## WS-4: Remove Hardcoded orgName (medium)

**Problem:** `add-team-dialog.tsx:66` has hardcoded `orgName: "Canonical"`.

**Approach:**
1. Pull org name from app config (existing `useConfig` hook or equivalent)
2. If no config exists for org name, add one to the config service

**Files:**
- `frontend/views/admin/components/add-team-dialog.tsx`

---

## WS-5: Add 404 Catch-All Route (medium)

**Problem:** Unknown paths render blank inside AppShell.

**Approach:**
1. Add a `*` catch-all route in `app.tsx` that renders a simple "Page not found" empty state with a link back to dashboard
2. Follow empty state conventions from CLAUDE.md (dashed border, icon, title, description)

**Files:**
- `frontend/app.tsx`
- New: `frontend/views/not-found/pages/not-found-page.tsx`

---

## WS-6: Extract Duplicated Helpers (medium)

**Problem:** `flattenTeams` duplicated in `people-tab.tsx` and `teams-tab.tsx`. Timestamp formatting duplicated across 3+ files (covered partially by WS-1).

**Approach:**
1. Move `flattenTeams` to `lib/hooks/use-org.ts` or a new `views/admin/lib/team-utils.ts` (prefer feature-local since both consumers are in admin)
2. Update both consumers

**Files:**
- `frontend/views/admin/components/people-tab.tsx`
- `frontend/views/admin/components/teams-tab.tsx`

---

## WS-7: Type Safety Improvements (medium)

**Problem:** Loose `string` types where domain enums should be used; missing `as const` on query key factories.

**Approach:**
1. `lib/hooks/use-config.ts:44` — type `sourceType` with a `SourceType` union derived from proto enums
2. `lib/hooks/use-config.ts` — type `contributionType` and `state` in `ContributionFilters` with domain unions
3. `lib/hooks/use-metrics.ts:18-41` — add `as const` to `metricsKeys.compare` and `metricsKeys.contributions` return types
4. Update barrel exports in `lib/hooks/index.ts` to include `useListTeamContributions`, `ContributionFilters`, `Contribution`, `useIsMobile`

**Files:**
- `frontend/lib/hooks/use-config.ts`
- `frontend/lib/hooks/use-metrics.ts`
- `frontend/lib/hooks/index.ts`

---

## WS-8: Debounce GitHub Team Search (medium → low, CLAUDE.md violation)

**Problem:** `github-team-picker-dialog.tsx:29` fires `useListGithubTeams(search)` on every keystroke. CLAUDE.md specifies 300ms debounce for search inputs.

**Approach:**
1. Add `useRef` + `setTimeout` debounce pattern per CLAUDE.md search conventions
2. Ensure the debounced value drives the query, not the raw input

**Files:**
- `frontend/views/teams/components/github-team-picker-dialog.tsx`

---

## WS-9: Password Validation on Setup Page (medium)

**Problem:** `setup-page.tsx:88` enforces min length only via HTML `minLength={8}`. No client-side Zod validation.

**Approach:**
1. Add a Zod schema for the setup form (password min 8, confirm match)
2. Integrate with existing form submission logic
3. Server-side enforcement should already exist — verify

**Files:**
- `frontend/views/setup/pages/setup-page.tsx`

---

## WS-10: Split Oversized Files (medium)

**Problem:** `handlers-tab.tsx` (511 lines, 5 components) and `teams-page.tsx` (504 lines) exceed the 500-line guideline.

**Approach:**
1. `handlers-tab.tsx` — after WS-1 extraction, this should be well under 500 lines. If not, extract remaining sub-components
2. `teams-page.tsx` — extract `SortableHeader` and `MetricsRow` into `views/teams/components/`

**Files:**
- `frontend/views/admin/components/handlers-tab.tsx`
- `frontend/views/teams/pages/teams-page.tsx`

---

## Backlog (low severity — opportunistic)

These are not blockers but should be addressed when touching the relevant files:

| Item | File | Fix |
|------|------|-----|
| Remove `"use client"` directives | 8 files in `components/ui/` | Delete the no-op directive |
| Replace custom toggle switches | `source-settings-forms.tsx`, `source-row.tsx` | Use shadcn `Switch` |
| Replace `confirm()` with Dialog | `source-row.tsx`, `teams-tab.tsx` | Use shadcn AlertDialog |
| Add `<Suspense fallback>` | `app.tsx:15` | Add Loader2 spinner |
| Standardize router imports | `login-page.tsx`, `setup-page.tsx` | `react-router` not `react-router-dom` |
| Add `<React.StrictMode>` | `main.tsx` | Wrap root |
| Move `shadcn` to devDependencies | `package.json` | Move in deps |
| `getCoreRowModel()` outside component | `data-table.tsx` | Hoist call |
| `isActive` outside component | `app-sidebar.tsx` | Extract function |
| Validate `JSON.parse` calls with Zod | `source-status-card.tsx`, `team-mapping-suggestions.tsx` | Add runtime validation |

---

## Execution Order

1. **WS-1** (shared extraction) — largest impact, unblocks WS-10
2. **WS-2** (render side effects) — quick fix, eliminates React warnings
3. **WS-5** (404 route) — quick fix, improves UX
4. **WS-4** (hardcoded org) — quick fix
5. **WS-6** (flattenTeams) — quick extraction
6. **WS-3** (mutation coordination) — moderate effort
7. **WS-7** (type safety) — moderate effort, low risk
8. **WS-8** (debounce) — quick fix
9. **WS-9** (Zod validation) — moderate effort
10. **WS-10** (file splitting) — mostly handled by WS-1

---

## Checklist

- [ ] WS-1: Extract shared run/handler components and formatting utils
- [ ] WS-2: Fix render-phase navigate() calls
- [ ] WS-3: Coordinate mutations in PersonDetailDialog
- [ ] WS-4: Remove hardcoded orgName
- [ ] WS-5: Add 404 catch-all route
- [ ] WS-6: Extract duplicated flattenTeams helper
- [ ] WS-7: Type safety — domain enums, as const, barrel exports
- [ ] WS-8: Debounce GitHub team search
- [ ] WS-9: Zod validation on setup page
- [ ] WS-10: Split oversized files
- [ ] All `prek run -av` clean
