# Plan 26 — Teams Page Redesign: Compact Navigation, Metrics Focus

## Problem

The teams page hierarchy tree dominates the viewport. With ~30+ teams across 4 levels (Org > Group > Team > Squad), the tree occupies the full left 60% of the screen, pushing metrics below the fold. Users must scroll extensively to find and select a team, and the page's primary value — metrics and comparisons — is buried.

Screenshot observations:
- The tree takes 3 of 5 grid columns; detail panel takes 2
- Every node shows name, type badge, lead name, and member count inline
- Deep nesting with indentation wastes horizontal space
- No search/filter — users must visually scan the full tree
- Metrics chart and comparison table are below the fold entirely
- The "select a team to view its members" placeholder wastes the right panel when nothing is selected

## Design Goals

1. **Metrics first** — charts and comparison table should be visible without scrolling
2. **Compact team selection** — selecting a team should take 1-2 clicks, not scrolling a giant tree
3. **Preserve hierarchy awareness** — users still need to understand Org > Group > Team > Squad relationships
4. **Fast team switching** — support keyboard-driven search for power users
5. **Compare multiple teams** — enable selecting 2+ teams for side-by-side metrics

## Proposed Design

### Layout: Full-width metrics with compact team selector

Replace the current two-column tree + detail layout with:

```
┌──────────────────────────────────────────────────────────┐
│ Teams                                                     │
│ Organisation hierarchy and team performance                │
├──────────────────────────────────────────────────────────┤
│ [Breadcrumb: Charm Engineering > Sinan Awad's Team]       │
│ [Team Selector Combobox ▾]    [Period: 1w 2w 1m 1q 1y *] │
├──────────────────────────────────────────────────────────┤
│                                                           │
│  ┌─ Metric Cards Row ──────────────────────────────────┐  │
│  │ Merged PRs │ Avg Review │ Members │ Active Contribs │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                           │
│  ┌─ Child Teams Comparison Table ──────────────────────┐  │
│  │ Name    Type   Merged PRs  Avg Review  Members      │  │
│  │ ...clickable rows to drill down...                  │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                           │
│  ┌─ Charts ────────────────────────────────────────────┐  │
│  │ Bar chart comparing child teams                     │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                           │
│  ┌─ Members ───────────────────────────────────────────┐  │
│  │ Collapsible member list for selected team           │  │
│  └─────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

### Component 1: Team Selector Combobox

Replace the full tree with a **searchable combobox** (shadcn `Combobox` / `Popover` + `Command`).

**Behaviour:**
- Closed state: shows selected team name + type badge + member count in a single line
- Default selection: top-level org (e.g. "Charm Engineering")
- Open state: filterable dropdown with the full hierarchy, indented by depth
- Type-ahead search filters by team name across all levels
- Each item shows: indented name, type badge, member count (compact)
- Selecting a team closes the dropdown and navigates to that team's view
- Keyboard: arrow keys to navigate, type to filter, Enter to select

**Why combobox over tree:**
- Takes 1 line when closed vs 30+ lines for the tree
- Search is O(1) for the user vs O(n) visual scanning
- Familiar pattern (VS Code command palette, GitHub repo switcher)

### Component 2: Breadcrumb Navigation

Show the path from root to selected team as a clickable breadcrumb:

```
Charm Engineering > Ubuntu Engineering > Matthieu Clemenceau's Team > Julian Klode's Squad
```

- Each segment is clickable to navigate up the hierarchy
- Provides spatial context without the tree's space cost
- Uses shadcn `Breadcrumb` component

### Component 3: Drill-Down Comparison Table

The main content area shows a **comparison table of the selected team's direct children**:

| Name | Type | Merged PRs | Avg Review (hrs) | Members |
|------|------|-----------|-------------------|---------|
| Ales Stimec's Squad | Squad | 42 | 3.2 | 5 |
| Vitaly Antonenko's Squad | Squad | 38 | 4.1 | 7 |

- Rows are clickable — clicking drills down into that child team
- Sortable columns (current sort logic preserved)
- If the selected team is a leaf (no children), show individual contributor metrics instead
- This replaces both the old tree browsing AND the separate metrics table

### Component 4: Summary Metric Cards

A row of cards at the top showing aggregate metrics for the currently selected team:

- Total merged PRs
- Average review turnaround
- Total members
- Active contributors (members with >0 contributions in period)

Uses shadcn `Card` components. Provides at-a-glance context for the selected scope.

### Component 5: Members Section

Move the current detail panel's member list to a **collapsible section** below the charts:

- Collapsed by default when viewing a team with children (focus on child comparison)
- Expanded by default when viewing a leaf team (squad with no children)
- Shows the same member info: name, email, platform identities

## Interaction Flow

1. **Page load** — defaults to the top-level org, showing all groups as the comparison table
2. **Drill down** — click a row in the comparison table to navigate into that team; breadcrumb extends
3. **Drill up** — click any breadcrumb segment to navigate back up
4. **Jump anywhere** — open the combobox, type a team name, select it directly
5. **Change period** — period selector updates all metrics in place

## URL State

Persist selection in the URL for shareability:

```
/teams?team=<team-id>&period=1m
```

- Use `useSearchParams` from React Router
- Default: no `team` param = top-level org
- Enables bookmarking and sharing specific team views

## Migration from Current Design

### Files to modify:
- `views/teams/pages/teams-page.tsx` — new layout, replace grid with single-column
- `views/teams/components/team-tree.tsx` — repurpose into combobox dropdown content (reuse hierarchy rendering but compact)
- `views/teams/components/team-detail-panel.tsx` — dissolve; members move to collapsible section in main page

### New components:
- `views/teams/components/team-selector.tsx` — combobox with search + hierarchy dropdown
- `views/teams/components/team-breadcrumb.tsx` — breadcrumb path from root to selected team
- `views/teams/components/team-metric-cards.tsx` — summary cards row
- `views/teams/components/team-comparison-table.tsx` — sortable child-team table (evolved from existing metrics table)

### Hooks:
- Existing `useGetTeamTree()` — still needed for combobox and breadcrumb path resolution
- Existing `useGetTeam()` — still needed for member list
- Existing `useCompareTeams()` — still needed, but scoped to children of selected team only
- New: helper to compute ancestor path from tree (for breadcrumb)

### Components preserved:
- `period-selector.tsx` — unchanged
- `team-tree.tsx` — kept for admin page (`views/admin/components/teams-tab.tsx` still uses it)

## Implementation Steps

1. **Add URL state** — wire `team` and `period` into `useSearchParams`
2. **Build `team-selector.tsx`** — combobox with hierarchy dropdown and search
3. **Build `team-breadcrumb.tsx`** — breadcrumb from tree ancestry
4. **Build `team-metric-cards.tsx`** — summary cards for selected team
5. **Evolve comparison table** — refactor existing metrics table to show children of selected team, with drill-down click handlers
6. **Integrate new layout** — rewrite `teams-page.tsx` with new single-column layout
7. **Collapsible members section** — move member list from detail panel into main page
8. **Clean up** — remove `team-detail-panel.tsx`, update any imports

## Out of Scope

- Multi-team comparison (selecting arbitrary teams to compare) — future enhancement
- Individual contributor drill-down from member list — separate feature
- Saving favourite teams — future enhancement
- Team-level alerting or thresholds — Phase 3
