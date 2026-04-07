# Plan 64 — Ingestion Page Simplification

**Status:** Proposed
**Scope:** Frontend only — no backend changes required
**Supersedes:** Builds on plan 47 (ingestion redesign) and 48 (handlers tab); does not replace them.

## Problem

The Ingestion page has too many independent controls. Users can:

1. **Run/Cancel the pipeline** (pipeline card header)
2. **Run All** sources (ingestion card header)
3. **Run/Cancel each source** individually (per-row buttons)
4. **Backfill** each source (per-row icon)
5. **Run/Cancel Enrichments** (AI Pipeline card)
6. **Run/Cancel Embeddings** (AI Pipeline card)

This creates decision paralysis. The pipeline *should* be the primary action — it orchestrates all stages in the correct order. But the UI gives equal visual weight to every control, encouraging users to micro-manage individual sources rather than trusting the pipeline.

Additionally, the pipeline DAG and the ingestion source list are in separate cards with duplicated status information. The DAG's "Ingestion" stage shows per-source completion, while the source list below shows the same sources with progress bars. When both are on screen, it's unclear which is the canonical view.

## Design Goals

- **Pipeline is the single primary control** — one prominent Run/Stop button for the whole pipeline. Everything else is secondary.
- **Reduce visual clutter** — fewer cards, fewer buttons, less duplication between the DAG and the source list.
- **Preserve detail access** — the detailed progress view (PRs fetched, reviews, rate limits, repo progress) is valuable when monitoring a running pipeline. It should be easy to reach, but not always visible.
- **Keep advanced controls accessible** — individual source runs and backfills are power-user features. They should exist but be visually subordinate.
- **Enable/disable sources inline** — the `enabled` field already exists on `SourceConfig` with full backend support (`UpdateSource` RPC). Currently the toggle only lives in the Admin source config view. Surfacing it on the Ingestion page lets users quickly disable a problematic source without leaving the monitoring view.

## Design

### 1. Unified Pipeline Card

Merge the pipeline DAG card and the ingestion source list into a single "Pipeline" card. The card has two visual zones:

```
┌──────────────────────────────────────────────────────────────────────┐
│  Pipeline   RUNNING  2m ago                          ▶ Run  ■ Stop  │
│─────────────────────────────────────────────────────────────────────│
│                                                                      │
│  ┌──────────┐    ┌─────────┐    ┌───────────┐    ┌─────────┐        │
│  │Ingestion │───→│ Metrics │───→│Enrichment│───→│Embedding│───→ …   │
│  └──────────┘    └─────────┘    └───────────┘    └─────────┘        │
│       └───→ ┌──────────────────┐                                     │
│             │Identity Resolution│                                    │
│             └──────────────────┘                                     │
│                                                                      │
│─────────────────────────────────────────────────────────────────────│
│  Source               Progress                         Items         │
│─────────────────────────────────────────────────────────────────────│
│ ▸ ● Github  Collecting  ████░░ 37%  288/785 repos      101          │
│   ● Canonical  Idle      2m ago                         33          │
│   ● Charmhub   Idle      2m ago                          0          │
│   ● Jira       Idle      1m ago                        442          │
│   ● Snapcraft  Idle      2m ago                         23          │
│   ● Ubuntu     Idle      2m ago                         52          │
│─────────────────────────────────────────────────────────────────────│
│   ● Enrichments  6 queued  1h ago                                    │
│   ● Embeddings   24% coverage · 7924 queued  5d ago                  │
└──────────────────────────────────────────────────────────────────────┘
```

**Key changes:**

- **One card, not three.** The pipeline DAG, source list, and AI pipeline status merge into a single card.
- **Pipeline controls in card header.** Run and Stop are the only prominent buttons. The current IngestionSummary text ("1 running · 5 idle · 101 items") moves into the card header alongside the status badge.
- **No per-row Run/Cancel buttons by default.** The source rows show status and progress only. The pipeline handles orchestration.
- **Enrichments and Embeddings are rows in the same list**, below a subtle separator. They're part of the pipeline, not a separate card.
- **The DAG stays**, but it's a compact visual indicator, not a control surface. No separate Run/Cancel on the DAG — those live only in the card header.

### 2. Simplified Card Header

```
┌─────────────────────────────────────────────────────────────────────┐
│ ⑂ Pipeline  [RUNNING]  2m ago  ·  1 running · 5 idle · 101 items  │
│                                                     ▶ Run  ■ Stop   │
└─────────────────────────────────────────────────────────────────────┘
```

- **Left:** Pipeline icon + "Pipeline" title + status badge + relative time + inline summary stats.
- **Right:** Primary action buttons.
  - When idle: single "Run Pipeline" button (replaces both "Run All" and per-pipeline Run).
  - When running: "Cancel" button (cancels the pipeline, which cascades to individual handlers).
- **No "Run All" button.** The pipeline *is* "run all". Running the pipeline triggers all sources in sequence via the backend orchestrator.

### 3. Source Rows — Status Only, Detail on Expand

Source rows keep their current compact layout but **lose the action buttons**:

```
  ● Github   Collecting   ████░░ 37%  288/785 repos    101
```

- Status dot + name + state label + inline progress + items count.
- Expand chevron appears only for active sources with detail data (same as today).
- Expanded detail shows PRs, reviews, rate limits, status message (unchanged from plan 47).

### 4. Advanced Controls via Overflow Menu

For power users who need to run/backfill individual sources, add a context menu (three-dot `MoreHorizontal` icon) that appears **on row hover** (or always visible on mobile):

```
  ● Github   Collecting   ████░░ 37%  288/785 repos    101    ⋮
                                                              ┌───────────────┐
                                                              │ Run            │
                                                              │ Backfill…      │
                                                              │ Cancel         │
                                                              │───────────────│
                                                              │ ○ Disable      │
                                                              └───────────────┘
```

- **Run** — triggers this source individually (outside the pipeline).
- **Backfill…** — opens the backfill date dialog (same as today).
- **Cancel** — cancels only this source's active run.
- **Enable/Disable** — toggles the source's `enabled` field via `UpdateSource` RPC. Disabled sources are skipped by the pipeline and shown with reduced opacity in the list.

This preserves all current functionality but moves it out of the primary visual flow. The three-dot menu is a well-understood pattern for secondary actions.

For the AI handler rows (Enrichments, Embeddings), the overflow menu offers Run and Cancel only (no backfill or enable/disable).

### Disabled Source Appearance

When a source is disabled:

- The row renders at **reduced opacity** (`opacity-50`) to visually distinguish it from active sources.
- The status dot is grey regardless of state.
- The overflow menu shows "Enable" instead of "Disable".
- Disabled sources sort **below** idle sources in the list.
- The pipeline skips disabled sources automatically (backend already handles this via the `enabled` flag on `SourceConfig`).
- The summary strip excludes disabled sources from running/idle counts but shows a separate count: "1 running · 4 idle · 1 disabled".

### 5. Collapsed DAG When Idle

When no pipeline has ever run (or the last run is old), the DAG takes up significant vertical space for no benefit. When idle:

- **Show the DAG collapsed by default** — just the card header with "Pipeline · All idle · Last run 5m ago · [Run Pipeline]".
- **Expand to show the DAG** via a chevron or "Show pipeline" link.
- **When running, the DAG is always expanded** — this is when it's most useful.

This saves ~200px of vertical space in the common "everything is idle" state.

### 6. Run History — Unchanged

The collapsible Run History panel at the bottom stays as-is. It already works well with its source filter dropdown and compact table. No changes needed.

## Component Architecture

### New Components

| Component | Location | Purpose |
|---|---|---|
| `SourceOverflowMenu` | `views/ingestion/components/source-overflow-menu.tsx` | Three-dot dropdown with Run/Backfill/Cancel/Enable/Disable per source |
| `AiHandlerRow` | Extracted from current `ai-pipeline-status.tsx` | Single row for Enrichments or Embeddings, matching source row layout |

### Modified Components

| Component | Change |
|---|---|
| `ingestion-page.tsx` | Remove separate `<AiPipelineStatus>` card. Merge pipeline + sources into single card. Pipeline controls move to card header. |
| `pipeline-graph.tsx` | Remove `PipelineActions` from inside the graph. Add collapsible behaviour (collapsed when idle, expanded when running). |
| `source-row.tsx` | Remove inline `RunButton`, `CancelButton`, and `BackfillDialog` trigger. Add overflow menu on hover. Keep expand chevron + detail. Render disabled sources at reduced opacity. |
| `source-list.tsx` | Add AI handler rows (Enrichments, Embeddings) below sources with a visual separator. Sort disabled sources last. |
| `ingestion-summary.tsx` | Remove `IngestionActions` export (Run All button). Keep `IngestionSummary` — add disabled count to summary text. |
| `pipeline-actions.tsx` | Becomes the sole pipeline control. Stays in card header. |

### Deleted Components

| Component | Reason |
|---|---|
| `ai-pipeline-status.tsx` | AI handlers move into the source list as rows. The separate card is removed. |

## Implementation Plan

### Phase 1: Merge Cards + Remove Per-Row Actions

1. **Remove per-row Run/Cancel buttons from `source-row.tsx`** — replace with overflow menu (`SourceOverflowMenu`) using shadcn `DropdownMenu`. Menu items: Run, Backfill, Cancel, separator, Enable/Disable. Show on hover via `opacity-0 group-hover:opacity-100` on the menu trigger. The Enable/Disable item calls `UpdateSource` with `{ sourceId, enabled }` — reuse the existing `useUpdateSource` hook from the admin view (or extract to `lib/hooks/` if not already shared). Disabled sources render at `opacity-50` with a grey status dot.
2. **Remove `IngestionActions` (Run All button)** from `ingestion-summary.tsx`. Keep `IngestionSummary`.
3. **Merge AI handlers into source list** — extract `AiHandlerRow` from `ai-pipeline-status.tsx` that matches the source row grid layout. Add to `source-list.tsx` below a `<Separator>`.
4. **Delete `ai-pipeline-status.tsx`** — no longer needed.
5. **Update `ingestion-page.tsx`** — single card containing: pipeline graph (top), source list with AI rows (bottom), pipeline actions in card header.

### Phase 2: Collapsible DAG

6. **Add collapsible DAG** — use shadcn `Collapsible` in `pipeline-graph.tsx`. Default open when pipeline is running, closed when idle. Chevron toggle in card header.
7. **Move pipeline status badge + summary inline** into the card header so key info is visible even when DAG is collapsed.

### Phase 3: Polish

8. **Responsive tweaks** — ensure overflow menu works on mobile (always visible, not hover-dependent). Stack layout for narrow screens.
9. **Update tests** — adjust `ingestion-page.test.tsx` for new structure (single card, no separate AI pipeline card, overflow menu instead of inline buttons).

## Wireframe — Idle State (DAG Collapsed)

```
┌──────────────────────────────────────────────────────────────────────┐
│ ⑂ Pipeline  · 5 idle · 1 disabled · Last run 2m ago  ▸ Show ▶ Run  │
│─────────────────────────────────────────────────────────────────────│
│  Source               Progress                         Items         │
│─────────────────────────────────────────────────────────────────────│
│   ● Canonical  Idle      2m ago                         33        ⋮ │
│   ● Github     Idle      2m ago                        101        ⋮ │
│   ● Jira       Idle      1m ago                        442        ⋮ │
│   ● Snapcraft  Idle      2m ago                         23        ⋮ │
│   ● Ubuntu     Idle      2m ago                         52        ⋮ │
│   ○ Charmhub   Disabled                                  0        ⋮ │  ← opacity-50
│─────────────────────────────────────────────────────────────────────│
│   ● Enrichments  6 queued  1h ago                                 ⋮ │
│   ● Embeddings   24% coverage · 7924 queued  5d ago               ⋮ │
└──────────────────────────────────────────────────────────────────────┘
```

## Wireframe — Running State (DAG Expanded)

```
┌──────────────────────────────────────────────────────────────────────┐
│ ⑂ Pipeline  RUNNING  2m ago  1 running · 5 idle · 101 items         │
│                                                         ■ Cancel     │
│─────────────────────────────────────────────────────────────────────│
│  ┌──────────┐    ┌─────────┐    ┌───────────┐    ┌─────────┐        │
│  │Ingestion │───→│ Metrics │───→│Enrichment│───→│Embedding│───→ …   │
│  └──────────┘    └─────────┘    └───────────┘    └─────────┘        │
│       └───→ ┌──────────────────┐                                     │
│             │Identity Resolution│                                    │
│             └──────────────────┘                                     │
│─────────────────────────────────────────────────────────────────────│
│  Source               Progress                         Items         │
│─────────────────────────────────────────────────────────────────────│
│ ▸ ● Github  Collecting  ████░░ 37%  288/785 repos      101       ⋮ │
│   ● Canonical  Idle      2m ago                         33        ⋮ │
│   ● Charmhub   Idle      2m ago                          0        ⋮ │
│   ● Jira       Idle      1m ago                        442        ⋮ │
│   ● Snapcraft  Idle      2m ago                         23        ⋮ │
│   ● Ubuntu     Idle      2m ago                         52        ⋮ │
│─────────────────────────────────────────────────────────────────────│
│   ● Enrichments  6 queued  1h ago                                 ⋮ │
│   ● Embeddings   24% coverage · 7924 queued  5d ago               ⋮ │
└──────────────────────────────────────────────────────────────────────┘

  ▸ Run History  93 completed · 2 failed · 1 running
```

## Non-Goals

- **Backend changes** — the pipeline orchestration already handles running all stages. The `enabled` field on `SourceConfig` and the `UpdateSource` RPC already exist. No new RPCs or schema changes needed.
- **Removing individual source runs entirely** — power users still need them (e.g., re-running a single failed source). They're just moved to an overflow menu.
- **Changing the DAG layout or stages** — the ReactFlow DAG visualisation works well. We're only changing where it sits (same card vs. separate) and when it's visible (collapsible).
- **Changing progress normalisation** — plan 47's `normaliseProgress()` and `extractDetail()` work correctly. No changes needed.
- **Changing the Run History panel** — it already uses compact select filters and works well.

## Migration Notes

This is purely additive/reorganisational frontend work. No data model changes, no API changes, no breaking changes to existing hooks. The `useIngestionStatus`, `usePipelineStatus`, `useEnrichmentPipelineStatus`, and `useEmbeddingStatus` hooks all remain unchanged — they just feed into a different component layout.
