# Plan 47 — Ingestion Page Redesign

**Status:** Proposed
**Scope:** Frontend only — no backend changes required

## Problem

The ingestion page is too information-dense and visually busy. Key issues:

1. **No overall summary** — you can't glance at the page and know "3 of 6 sources are running, ~15% complete". You have to mentally aggregate across all cards.
2. **Inconsistent progress reporting** — GitHub shows a detailed progress bar with repo counts, Jira shows "Finalising", Charmhub shows "Starting", Discourse shows nothing. Each source handler emits different `progressJson` shapes, but the UI tries to render them all the same way.
3. **Cards are too tall** — when a source is collecting, the card expands to show phase labels, rate limits, PR/review counts, skipped identities, a progress bar, and a status message. This pushes other cards off-screen.
4. **AI Pipeline is visually disconnected** — it sits as a separate section below all sources, but it's conceptually part of the same pipeline ("ingest → enrich").
5. **Run history duplicates live state** — the bottom table shows running sources alongside historical runs, so you see the same information twice.

## Design Goals

- **Glanceable overall status** — a single summary strip at the top tells you what's happening right now.
- **Compact source rows** — each source is a single row (not a card) in all states. Progress shown inline, not expanded.
- **Unified progress model** — normalise the different `progressJson` shapes into a common "percentage + short label" so all sources look consistent.
- **AI Pipeline integrated** — treat enrichment as another row in the same list, not a separate section.
- **Run history is for history** — filter out currently-running entries (they're already shown above) or visually de-duplicate them.

## Design

### 1. Summary Strip

A horizontal bar at the top of the page, replacing the current section header.

```
┌─────────────────────────────────────────────────────────────────┐
│  Sources: 3 running · 3 idle    Items this session: 3,231      │
│  ████████░░░░░░░░░░░░░░░ 35%    ▸ Run All                      │
└─────────────────────────────────────────────────────────────────┘
```

- **Left:** count of running/idle/error sources. When nothing is running: "All sources idle · Last run 5m ago".
- **Center:** aggregate items collected across all currently-running sources.
- **Right:** Run All button (already exists, just relocated).
- **Progress bar:** aggregate progress across all running sources (see §3 for normalisation). Only shown when ≥1 source is running.

Implementation: new `<IngestionSummary>` component. Derives counts from the existing `SourceStatus[]` array.

### 2. Compact Source List

Replace the stacked cards with a table-like list. Each source is always **one row**, regardless of state.

```
┌──────────────┬────────────┬────────────────────────────────┬───────────┬──────────┐
│ Source        │ Status     │ Progress                       │ Items     │          │
├──────────────┼────────────┼────────────────────────────────┼───────────┼──────────┤
│ Github        │ ● Collecting│ ████████░░░░ 35%  22/617 repos │ 245       │ Cancel   │
│ Jira          │ ● Collecting│ ████████████████░ 95%  Finalising│ 1,950   │ Cancel   │
│ Charmhub      │ ● Collecting│ ░░░░░░░░░░░░ —   Starting     │ 1,036     │ Cancel   │
│ AI Pipeline   │ ● Running  │ ████░░░░░░░░ 30%  8,739/13,863│ 8,739     │ Cancel   │
│ Canonical     │ ○ Idle     │                   just now     │ 4         │ Run · ⊙  │
│ Snapcraft     │ ○ Idle     │                   just now     │ 0         │ Run · ⊙  │
│ Ubuntu        │ ○ Idle     │                   just now     │ 38        │ Run · ⊙  │
└──────────────┴────────────┴────────────────────────────────┴───────────┴──────────┘
```

Key changes from current cards:

- **Status column:** coloured dot + single word. No badge — just text with a coloured indicator.
- **Progress column:** when collecting, shows a thin inline progress bar + percentage + short context label. When idle, shows relative time since last run ("just now", "2h ago"). This is the column that absorbs all the varied progress info into a consistent format.
- **Items column:** always shows `itemsCollected` (whether running or historical).
- **Actions column:** Cancel when running, Run + Backfill (as icon `⊙`) when idle.
- **No expanded state.** Detail like rate limits, PR/review counts, skipped identities moves to a hover tooltip or a click-to-expand detail row (see §5).

Sorting: running sources first (sorted by start time), then idle (sorted by last run, most recent first).

Implementation: replace `<SourceStatusCard>` with `<SourceRow>` inside a `<Table>`. The AI Pipeline becomes another row in this table (sourced from `useEnrichmentPipelineStatus()` and mapped to the same row shape).

### 3. Normalised Progress

The `progressJson` field contains handler-specific shapes. Normalise them into a common model on the frontend:

```typescript
type NormalisedProgress = {
  percent: number | null;    // 0–100, null if indeterminate
  label: string;             // short context: "22/617 repos", "Finalising", "Starting"
};
```

Normalisation rules per source type:

| Source type | Percent calculation | Label |
|---|---|---|
| GitHub (team_repos phase) | `repos_completed / repos_total * 100` | `"{n}/{total} repos"` |
| GitHub (member_search) | `search_users_completed / search_users_total * 100` | `"{n}/{total} members"` |
| GitHub (complete) | 100 | `"Finalising"` |
| Jira | If `status_message` contains numbers, parse; else null | `status_message` truncated |
| Discourse | null (indeterminate) | `status_message` or `"Collecting"` |
| Enrichment | `enrichedThisRun / pending * 100` | `"{enriched}/{total}"` |
| Fallback | null | `status_message` or `"Processing"` |

A `normaliseProgress(sourceType: string, progressJson: string): NormalisedProgress` function in a new `lib/progress.ts` file.

When percent is null, the progress bar shows an indeterminate animation (CSS `animate-pulse` on a partial-width bar or a shimmer).

### 4. AI Pipeline as a Source Row

Instead of a separate `<AiPipelineStatus>` card, the enrichment pipeline appears as a row in the source list with:

- **Name:** "AI Pipeline"
- **Status:** Running/Idle derived from active enrichment runs
- **Progress:** normalised from enrichment stats
- **Items:** `enrichedThisRun` when running, `totalEnrichments` when idle

The enrichment-by-type breakdown (Review Depth, Sentiment, etc.) moves to the detail view (§5), not the main row.

### 5. Detail Expansion

Clicking a source row expands an inline detail panel below that row (using Collapsible). This is where secondary information lives:

**For running sources:**
- Rate limit info (remaining/total, coloured warning if low)
- Source-specific metrics (PRs fetched, reviews fetched, skipped identities for GitHub)
- Current operation (e.g., "Fetching PRs from canonical/ubuntu-sso-k8s-operator")

**For the AI Pipeline:**
- Enrichments by type breakdown (the badges currently shown)
- Pending vs completed counts

**For idle sources:**
- Last run duration
- Error message (if last run failed)
- Next scheduled run (when cron scheduling is implemented)

This keeps the main list scannable while preserving access to all current information.

### 6. Run History Cleanup

Minor changes to the existing run history table:

- **Default filter: exclude running** — show "Completed" + "Partial" + "Failed" by default. Running sources are already visible in the source list above. Add a "Running" toggle to include them if desired.
- **Collapse the filter bar** — the source name buttons take a lot of space. Replace with a `<Select>` dropdown for source filtering.
- **Keep everything else** — pagination, columns, detail dialog all work well.

## Implementation Plan

### Phase 1: Normalised Progress + Compact List

1. Create `frontend/views/ingestion/lib/progress.ts` — the `normaliseProgress()` function with tests.
2. Create `<SourceRow>` component — single-row rendering with inline progress bar.
3. Create `<SourceList>` component — wraps the table, integrates AI Pipeline as a row, handles sorting (running first).
4. Update `ingestion-page.tsx` — replace source cards section and AI Pipeline section with `<SourceList>`.

### Phase 2: Summary Strip + Detail Expansion

5. Create `<IngestionSummary>` component — aggregate status bar at the top.
6. Add collapsible detail expansion to `<SourceRow>` — click to show secondary info.
7. Update run history default filters and replace source filter buttons with a Select.

### Phase 3: Polish

8. Add indeterminate progress bar animation for sources without percentage data.
9. Add tooltip on progress bar showing the full status message.
10. Update tests (`ingestion-page.test.tsx`) for new component structure.

## Files Changed

| File | Change |
|---|---|
| `views/ingestion/lib/progress.ts` | **New** — normaliseProgress function |
| `views/ingestion/components/source-row.tsx` | **New** — compact row component |
| `views/ingestion/components/source-list.tsx` | **New** — table wrapper with AI Pipeline integration |
| `views/ingestion/components/ingestion-summary.tsx` | **New** — aggregate status strip |
| `views/ingestion/pages/ingestion-page.tsx` | **Modified** — use new components |
| `views/ingestion/components/source-status-card.tsx` | **Deleted** — replaced by source-row |
| `views/ingestion/components/ai-pipeline-status.tsx` | **Deleted** — absorbed into source-list |
| `views/ingestion/components/ingestion-runs-table.tsx` | **Modified** — default filters, Select dropdown |
| `views/ingestion/pages/ingestion-page.test.tsx` | **Modified** — update for new structure |

## Non-Goals

- **Backend changes** — all data is already available, just poorly presented.
- **New progress fields** — we work with whatever `progressJson` each handler emits today. If a handler doesn't emit percentage-friendly data, we show indeterminate.
- **Real-time WebSocket** — polling is fine for this use case.
