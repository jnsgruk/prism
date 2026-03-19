# Plan 48 — Handlers Tab Redesign

**Status:** Proposed
**Scope:** Frontend only — no backend changes required

## Problem

The Handlers tab in the Admin view treats all eight handlers identically — flat list of cards, each with the same visual weight. Key issues:

1. **No categorical distinction** — ingestion handlers (GitHub, Jira, Discourse, GithubTeamSync) are mixed with system handlers (MetricsCompute, IdentityResolution, Enrichment, ModelCatalogue). These have fundamentally different purposes and interaction patterns: ingestion handlers require a source key and run frequently; system handlers are fire-and-forget maintenance tasks.
2. **Cards are too tall** — each handler is a full card with icon, description, method badges, and active run info. With 8 handlers, the page scrolls significantly before reaching run history.
3. **No at-a-glance status** — you can't quickly see "2 handlers running, 6 idle" without scanning every card.
4. **Trigger dialog is heavyweight** — opening a modal just to pick a method and click "Run" adds unnecessary friction, especially for system handlers that have only one method and no source key.
5. **Run history filters are cluttered** — 8 handler name buttons plus 5 status buttons create a wide, wrapping filter bar that's hard to scan.

## Design Goals

- **Separate ingestion from system handlers** — group them visually so the admin can reason about each category independently.
- **Compact rows** — adopt the same compact row pattern proven in Plan 47's ingestion redesign. Each handler is one row, not a card.
- **One-click run for simple handlers** — system handlers with a single method and no key should be triggerable directly from the row, no dialog needed.
- **Glanceable status** — summary counts at the top, consistent with the ingestion page pattern.
- **Cleaner run history** — replace button-bar filters with dropdowns, default to non-running entries.
- **Reuse shared components** — extract small, generic UI pieces from the Plan 47 ingestion work rather than duplicating them.

## Design

### 1. Two-Section Layout

Split the page into two visually distinct sections:

```
┌─────────────────────────────────────────────────────────────────┐
│  Ingestion Handlers                                              │
│  Handlers that fetch data from external platforms.               │
├──────────────────┬────────────┬─────────────────────┬───────────┤
│ Handler           │ Methods    │ Status              │           │
├──────────────────┼────────────┼─────────────────────┼───────────┤
│ GithubIngestion   │ 2 methods  │ ● Running — github  │ Cancel    │
│ JiraIngestion     │ 2 methods  │ ○ Idle              │ Run ▾     │
│ DiscourseIngest.  │ 2 methods  │ ○ Idle              │ Run ▾     │
│ GithubTeamSync    │ 1 method   │ ○ Idle              │ Run ▾     │
└──────────────────┴────────────┴─────────────────────┴───────────┘

┌─────────────────────────────────────────────────────────────────┐
│  System Handlers                                                 │
│  Background tasks for metrics, identity, AI, and maintenance.    │
├──────────────────┬────────────┬─────────────────────┬───────────┤
│ Handler           │ Description│ Status              │           │
├──────────────────┼────────────┼─────────────────────┼───────────┤
│ MetricsCompute    │ Recomputes…│ ○ Idle              │ Run       │
│ IdentityResolut.  │ Resolves…  │ ○ Idle              │ Run       │
│ Enrichment        │ AI enrich… │ ● Running           │ Cancel    │
│ ModelCatalogue    │ Fetches…   │ ○ Idle              │ Run       │
└──────────────────┴────────────┴─────────────────────┴───────────┘

┌─────────────────────────────────────────────────────────────────┐
│  Run History                                                     │
│  [Handler ▾] [Status ▾]                                          │
│  ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄  │
│  (existing table with Select-based filters)                      │
└─────────────────────────────────────────────────────────────────┘
```

The split is determined by the existing `requiresKey` field on `HandlerInfo`:
- **Ingestion Handlers:** `requiresKey === true`
- **System Handlers:** `requiresKey === false`

### 2. Handler Row Component

Each handler rendered as a single compact row. Replaces the current `HandlerCard`.

**Row layout:**

```
┌─[icon]─ Name ──── Description (truncated) ─── Methods ─── Status ─── Actions ─┐
```

- **Icon:** platform-specific for ingestion handlers (GitHub octicon, Jira icon, Discourse icon), `Cog` for system handlers.
- **Name:** handler name with "Handler" suffix stripped, as today.
- **Description:** short text, `text-muted-foreground`, truncated with ellipsis on narrow screens.
- **Methods:** badge count ("2 methods") for multi-method handlers. Single-method handlers show the method name directly.
- **Status:** coloured dot + label. When running, shows method name and key (if applicable): "● Running — run_ingestion · github-canonical". When idle: "○ Idle".
- **Actions:** context-dependent (see §3).

### 3. Streamlined Actions

Different action patterns based on handler complexity:

**System handlers (single method, no key):**
- When idle: single "Run" button that triggers immediately — no dialog.
- When running: "Cancel" button.

**Ingestion handlers (multiple methods, requires key):**
- When idle: "Run" button that opens a compact inline dropdown or popover (not a full dialog) to select method + source. The dropdown pre-selects `run_ingestion` as the default method.
- When running: "Cancel" button with the active run's source name shown in the status column.

**Multi-method system handlers (if any emerge):**
- When idle: "Run" button opens a method-only popover (no source picker).

This eliminates the modal dialog for the common case (running a system handler) while preserving the method/source picker where needed.

Implementation: replace `TriggerHandlerDialog` with a `<TriggerHandlerPopover>` component using shadcn `Popover`. The popover anchors to the Run button and contains a compact form: method select (if >1 method) + source select (if `requiresKey`) + trigger button.

### 4. Summary Counts

A small status line above each section showing aggregate state:

```
Ingestion Handlers — 1 running · 3 idle
System Handlers — 1 running · 3 idle
```

When nothing is running in a section: "All idle".

This is lighter than the ingestion page's summary strip — just a text line, no progress bar (handlers don't have meaningful aggregate progress).

### 5. Active Run Inline Detail

When a handler has an active run, clicking the expand chevron reveals a detail row with `bg-muted/40` background and `py-2.5` padding. Uses the same compact dot-separated `Stat` pattern from the ingestion page:

```
│ > GithubIngestion  │ 2 methods │ ● Running                      │ Cancel │
│  run_ingestion · github-canonical · Started 2m ago                        │
```

Only show the expand chevron when there's useful detail to display (method + key + start time). Don't expand for handlers that have no active run.

### 6. Run History Improvements

Same table as today, but with cleaner filters:

- **Handler filter:** replace the 8 button strip with a `<Select>` dropdown. Options: "All handlers", then each handler name. Group options into "Ingestion" and "System" using `SelectGroup` + `SelectLabel`.
- **Status filter:** replace the 5 buttons with a `<Select>` dropdown. Options: "All statuses", "Completed", "Partial", "Failed", "Running".
- **Default state:** exclude "Running" entries by default (they're visible in the sections above). Selecting "Running" in the status filter overrides this.
- **Layout:** both selects sit on one line, left-aligned, compact.

## Shared Component Extractions

Plan 47's ingestion redesign evolved through several iterations into a compact, information-dense design. The key patterns to reuse are:

### Design patterns established in Plan 47

1. **Compact rows with fixed-width grid columns** — each item is one row using `grid-cols-[1rem_minmax(8rem,1fr)_minmax(12rem,2fr)_6rem_7rem]`. Items and Actions columns use fixed widths (6rem/7rem) so headers align with data.
2. **StatusDot** — animated coloured dot (blue=active, green=idle, red=error, amber=waiting) with optional ping animation.
3. **Inline progress** — thin `h-1.5` progress bar + optional percent + short label. Indeterminate state uses `animate-pulse` on a partial-width bar.
4. **Compact inline stats for expanded detail** — dot-separated `Stat` spans (`2,489 PRs · 5,609 reviews · 2426 skipped`) on a `bg-muted/40` background with `py-2.5` padding. Not stat pills or cards — just inline text with icons.
5. **Expand only when useful** — chevron only appears when there are real stats to show. Sources with only a status message (which duplicates the progress label) don't get an expand chevron.
6. **Ghost-variant action buttons** — `RunButton` / `CancelButton` using `variant="ghost"` with `h-7` height, icon + hidden-on-mobile text label.
7. **Enrichment renamed to "Enrichments"** — not "AI Pipeline".
8. **Summary strip** — lightweight status bar: `"3 running · 3 idle · 88,554 items collected"` with Run All button. Text-only, no progress bar at this level.

### Components to extract

| Component | Current location | New location | What it does |
|---|---|---|---|
| `StatusDot` | `source-row.tsx` | `components/status-dot.tsx` | Animated coloured dot. Accepts `color` (Tailwind bg class) and `animate` boolean. |
| `Stat` / `DOT_SEP` | `source-row.tsx` | `components/inline-stat.tsx` | Compact inline stat: `value label` with optional icon and warning/danger variant. Plus the dot separator constant. |
| `RunButton` / `CancelButton` | `source-row.tsx` | `components/run-cancel-buttons.tsx` | Ghost-variant action buttons with loading spinner swap. |
| `formatRelativeTimeIso` | `source-row.tsx` | `lib/format.ts` | Formats an ISO date string as relative time ("5m ago", "2h ago"). |

### What stays feature-local

- `SourceRow`, `SourceList`, `IngestionSummary` — tightly coupled to ingestion data model
- `normaliseProgress()` / `extractDetail()` — handler-specific progress parsing
- `SourceDetail` / `EnrichmentDetail` — ingestion-specific expansion content

### What's already shared

- `components/run-detail-dialog.tsx` — click-to-inspect run details
- `lib/run-status.ts` — status badge config (variant + icon + label)
- `useListHandlers`, `useTriggerHandler`, `useCancelHandlerRun` hooks

## Implementation Plan

### Phase 0: Extract Shared Components

1. Extract `StatusDot` → `components/status-dot.tsx`. Accept `color` (Tailwind bg class) and `animate` boolean. Update `source-row.tsx` to import from the new location.
2. Extract `Stat` / `DOT_SEP` → `components/inline-stat.tsx`. Update `source-row.tsx` imports.
3. Extract `RunButton` / `CancelButton` → `components/run-cancel-buttons.tsx`. Update `source-row.tsx` imports.
4. Move `formatRelativeTimeIso` → `lib/format.ts`. Update `source-row.tsx` imports.
5. Verify ingestion page still works: `bun test` + manual check.

### Phase 1: Row Component + Section Split

6. Create `views/admin/components/handler-row.tsx` — compact single-row component using same grid pattern as ingestion rows (`grid-cols-[1rem_minmax(8rem,1fr)_minmax(12rem,2fr)_6rem_7rem]`). Uses shared `StatusDot`, `RunButton`/`CancelButton`, `Stat`. Expand chevron only when handler has an active run.
7. Create `views/admin/components/handler-section.tsx` — titled section with column headers and handler rows. Summary count in header text ("1 running · 3 idle").
8. Update `handlers-tab.tsx` — split handlers into ingestion vs system using `requiresKey`, render two `<HandlerSection>` components.

### Phase 2: Streamlined Trigger

9. Create `views/admin/components/trigger-handler-popover.tsx` — popover anchored to Run button. Compact form: method select (if >1) + source select (if `requiresKey`) + trigger button. Single-method no-key handlers trigger directly, no popover.
10. Remove `trigger-handler-dialog.tsx` — replaced by the popover.

### Phase 3: Run History Cleanup

11. Update `handler-runs-table.tsx` — replace handler filter buttons with grouped `<Select>`, replace status filter buttons with `<Select>`.

### Phase 4: Polish

12. Add active run detail row with `bg-muted/40` background using dot-separated `Stat` spans (method · key · started time).
13. Ensure narrow-width responsiveness — truncate descriptions, stack action buttons if needed.

## Files Changed

| File | Change |
|---|---|
| `components/status-dot.tsx` | **New** — extracted from source-row, shared animated status dot |
| `components/inline-stat.tsx` | **New** — extracted `Stat` component and `DOT_SEP` separator |
| `components/run-cancel-buttons.tsx` | **New** — extracted from source-row, shared Run/Cancel buttons |
| `lib/format.ts` | **Modified** — add `formatRelativeTimeIso` (moved from source-row) |
| `views/ingestion/components/source-row.tsx` | **Modified** — import extracted components instead of defining inline |
| `views/admin/components/handler-row.tsx` | **New** — compact row for a single handler |
| `views/admin/components/handler-section.tsx` | **New** — titled section with column headers and handler rows |
| `views/admin/components/trigger-handler-popover.tsx` | **New** — inline popover for triggering handlers |
| `views/admin/components/handlers-tab.tsx` | **Modified** — split into two sections, add summary counts |
| `views/admin/components/handler-runs-table.tsx` | **Modified** — Select-based filters |
| `views/admin/components/trigger-handler-dialog.tsx` | **Deleted** — replaced by popover |

## Non-Goals

- **Backend changes** — the `ListHandlers` RPC already returns `requiresKey` which is sufficient for the ingestion/system split. No new fields needed.
- **Progress bars in handler rows** — unlike the ingestion page (Plan 47), handlers don't have normalised progress data. The Handlers tab is an admin operations view, not a monitoring dashboard. If progress visibility is needed, the admin navigates to the Ingestion page.
- **Reordering or renaming handlers** — the handler list is static, defined in `HANDLER_DEFS` on the backend.
- **Moving ingestion handlers out of this tab** — the Ingestion page (Plan 47) already shows source-level status with progress. The Handlers tab shows the handler-level view (methods, direct trigger, run history). Both views are useful for different tasks.
