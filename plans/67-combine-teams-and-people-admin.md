# Plan 67 — Combine Teams & People Admin Tabs

## Status: Draft

## Problem

The Admin page has separate **Teams** and **People** tabs that manage closely related data. Teams contain people; people belong to teams. Navigating between them to do common tasks (e.g. "see who's unassigned, then put them in a team") requires tab-switching and mental context-juggling. Combining them would reduce clicks and surface relationships more naturally.

### Current state

- **Teams tab**: Team tree (hierarchical), side sheet for team details (members, GitHub mappings, lead). Add/edit/delete dialogs.
- **People tab**: Paginated table (name + team columns), search + filter (All/Unassigned/Inactive), person detail dialog (edit name/email/level, change team, toggle active, view identities).

### Design constraints

- Must not feel crowded — the people table alone can have hundreds of rows.
- Team tree and people table serve different mental models (hierarchy vs. flat list).
- Must preserve all current actions: CRUD teams, edit people, assign/unassign, deactivate, search, filter.
- Mobile/narrow viewports: sidebar layouts collapse gracefully.

---

## Option A — Master-Detail with Team Tree Sidebar (Recommended)

A persistent left panel shows the team tree; the right panel shows the people table filtered to the selected team (or "All" by default).

### Wireframe — Desktop (>= 768px)

```
+------------------------------------------------------------------------+
| Admin > Organisation                                                    |
+------------------------------------------------------------------------+
| [+ Add Team]  [Import Directory]                                        |
+------------------------+-----------------------------------------------+
| Teams           [+][-] | [Search people...  ]  [All|Unassigned|Inactive]|
| [Filter teams...     ] |-----------------------------------------------|
|------------------------| Name          | Team      | Level              |
| * All people      147  |-----------------------------------------------|
|------------------------| Alice Chen    | Backend   | Senior             |
| v Engineering      42  | Bob Kim       | Backend   | Mid                |
|   * Backend    (12)    | Carol Wu      | Frontend  | Senior             |
|   > Frontend    (8)    | Dave Patel    | Frontend  | Mid                |
|   > Platform    (6)    | Eve Santos    | Platform  | Junior             |
|   > SRE        (16)    | ...           | ...       | ...                |
| v Design          18   |                                                |
|   > UX          (9)    |                                                |
|   > Research    (9)    |-----------------------------------------------|
| > Product         12   | 1-25 of 147                   [<] 1 2 3 [>]  |
|                        | 10 / 25 / 50 / 100 per page                   |
|  · · · · · · · · · · · +-----------------------------------------------+
| Unassigned (3)         |
+------------------------+
```

**Key visual details:**
- Tree panel is `w-64` (256px), fixed. People table fills remaining space.
- "All people" is a selectable pseudo-node at the top of the tree with total count — this is the default selection, showing everyone.
- "Unassigned" is a pseudo-node at the bottom, styled slightly differently (dashed left border or muted icon), with its own count.
- Selected tree node gets `bg-muted/50` highlight (matches existing `TeamTree` style).
- Member counts on each team node use the existing `totalMemberCount` field from `TeamTree`.
- Tree has its own vertical scroll independent of the table.

### Wireframe — Team selected, detail sheet open

Clicking a person row opens the existing `PersonDetailDialog`. Clicking the kebab on a team node opens the existing edit/delete actions. The team detail panel (GitHub mappings, members list) is accessible via a dedicated button or double-click on the team node.

```
+------------------------+-----------------------------------------------+
| Teams           [+][-] | [Search people...  ]  [All|Unassigned|Inactive]|
| [Filter teams...     ] |                                                |
|------------------------| Showing: Backend (12 members)     [x clear]   |
| v Engineering      42  |-----------------------------------------------|
|   * Backend    (12)    | Alice Chen    | Backend   | Senior     +------+|
|   > Frontend    (8)    | Bob Kim       | Backend   | Mid        |Person||
|   > Platform    (6)    | Carol Wu      | Backend   | Junior     |Detail||
|   > SRE        (16)    | ...           |           |            |Dialog||
|                        |               |           |            |      ||
|                        |               |           |            +------+|
+------------------------+-----------------------------------------------+
```

When a team is selected, a subtle breadcrumb-style label appears above the table: "Showing: Backend (12 members)" with an [x] to clear the filter back to "All people".

### Wireframe — Narrow viewport (< 768px)

The tree sidebar collapses. A team picker dropdown replaces it in the filter bar.

```
+-------------------------------------------+
| Admin > Organisation                       |
+-------------------------------------------+
| [+ Add Team]                               |
+-------------------------------------------+
| [All teams     v]  [Search people...     ] |
| [All|Unassigned|Inactive]                  |
+-------------------------------------------+
| Name            | Team       | Level       |
|-------------------------------------------+
| Alice Chen      | Backend    | Senior      |
| Bob Kim         | Backend    | Mid         |
| Carol Wu        | Frontend   | Senior      |
| ...             | ...        | ...         |
|-------------------------------------------+
| 1-25 of 147                  [<] 1 [>]    |
+-------------------------------------------+
```

The team picker uses the existing `Command` popover pattern from `people-list-page.tsx` — a searchable dropdown showing the flattened tree with depth-indented names. It includes "All teams" (default) and "Unassigned" as options.

### Interaction model

| Action | Trigger | Result |
|---|---|---|
| Filter by team | Click tree node | Table shows members of that team; URL updates `?tab=org&team=<id>` |
| Show all people | Click "All people" pseudo-node (or clear filter) | Table shows everyone; `team` param removed from URL |
| Show unassigned | Click "Unassigned" pseudo-node OR click "Unassigned" filter button | Table shows people with no team assignment |
| Edit person | Click table row | `PersonDetailDialog` opens (unchanged) |
| Add team | Click "+ Add Team" button | `AddTeamDialog` opens (unchanged) |
| Edit team | Kebab menu on tree node | `EditTeamDialog` opens (unchanged) |
| Delete team | Kebab menu on tree node | `ConfirmDialog` opens (unchanged) |
| View team details | Click info icon on selected team node | Side `Sheet` with `TeamDetailPanel` (GitHub mappings, member list) |
| Search people | Type in search input | Debounced filter on current team scope (or all) |
| Filter status | Click All/Unassigned/Inactive buttons | Combines with team filter |
| Import directory | Click "Import Directory" button | `ImportDirectoryDialog` opens (unchanged) |

### URL state

```
?tab=org                        → All people (default)
?tab=org&team=<uuid>            → Filtered to specific team
?tab=org&team=unassigned        → Unassigned people (virtual)
```

The `team` parameter replaces the current separate `?tab=teams&team=<id>` pattern. The "Unassigned" filter button and the "Unassigned" pseudo-node both route to the same state.

### Component structure

```
views/admin/components/
├── org-tab.tsx              ← NEW: layout shell (sidebar + table)
├── org-team-sidebar.tsx     ← NEW: tree + pseudo-nodes + actions
├── org-people-panel.tsx     ← NEW: search/filter bar + DataTable + pagination
├── people-tab.tsx           ← REMOVED (absorbed into org-people-panel)
├── teams-tab.tsx            ← REMOVED (absorbed into org-team-sidebar)
├── add-team-dialog.tsx      ← unchanged
├── edit-team-dialog.tsx     ← unchanged
├── people-detail-dialog.tsx ← EXTRACTED from people-tab.tsx (currently inline)
└── ...
```

**`OrgTab`** — the layout shell:
```tsx
<div className="flex gap-0 pt-4">
  {/* Sidebar — hidden on mobile, replaced by dropdown in OrgPeoplePanel */}
  <div className="hidden w-64 shrink-0 md:block">
    <OrgTeamSidebar
      selectedTeamId={teamId}
      onSelectTeam={setTeamId}
      onAddTeam={() => setAddDialogOpen(true)}
      onEditTeam={setEditingTeam}
      onDeleteTeam={setDeletingTeam}
    />
  </div>
  <div className="min-w-0 flex-1">
    <OrgPeoplePanel
      teamId={teamId}
      onSelectTeam={setTeamId}   // for mobile dropdown
      onSelectPerson={setSelectedPerson}
    />
  </div>
</div>
```

**`OrgTeamSidebar`** — wraps the existing `TeamTree` with additions:
- Renders "All people" clickable row above the tree (total count from paginated query).
- Renders existing `TeamTree` component with `onSelect`, `onEdit`, `onDelete` props.
- Renders "Unassigned" row below the tree (count from a dedicated query or the `PersonFilter.UNASSIGNED` total).
- Keeps the existing tree search/filter, expand/collapse controls.

**`OrgPeoplePanel`** — the right side:
- On `< md` viewports: renders the team `Command` dropdown (reuses pattern from `people-list-page.tsx`).
- Shows "Showing: {team name} ({count})" breadcrumb when a team is selected, with clear button.
- Search input + filter buttons (All/Unassigned/Inactive).
- `DataTable` with `personNameColumn`, `personTeamColumn` columns (team column can be hidden when filtered to a specific team — stretch goal).
- `DataTablePagination`.
- Passes `teamId` to `usePaginatedPeople` — already supported by the `ListPeopleRequest.team_id` proto field.

### What already exists vs. what's new

| Piece | Status | Notes |
|---|---|---|
| `TeamTree` component | Exists | Reuse as-is with `onSelect`/`onEdit`/`onDelete` |
| `TeamDetailPanel` (sheet) | Exists | Opened from tree context menu or info button |
| `PersonDetailDialog` | Exists (inline in `people-tab.tsx`) | Extract to own file |
| `ListPeople` with `team_id` filter | Exists in proto + backend | Already wired in `usePaginatedPeople` |
| `usePaginatedPeople` hook | Exists | Already accepts `teamId` param |
| `flattenTeams` utility | Exists | Used for mobile dropdown |
| `Command` team picker dropdown | Exists in `people-list-page.tsx` | Reuse pattern for mobile fallback |
| "All people" pseudo-node | New | Simple clickable row above tree |
| "Unassigned" pseudo-node with count | New | Needs a count query (can use `ListPeople` with `UNASSIGNED` filter, `pageSize: 0`) |
| `OrgTab` layout shell | New | ~60 lines: flex layout + state + dialog wiring |
| `OrgTeamSidebar` wrapper | New | ~80 lines: pseudo-nodes + `TeamTree` |
| `OrgPeoplePanel` | New | ~120 lines: mostly moved from `PeopleTab` |
| Responsive breakpoint logic | New | `hidden md:block` on sidebar, dropdown on mobile |

### Migration path

1. Extract `PersonDetailDialog` from `people-tab.tsx` into its own file.
2. Create `OrgTeamSidebar`, `OrgPeoplePanel`, `OrgTab`.
3. Update `admin-page.tsx`: replace `teams` + `people` tabs with single `org` tab. Update `VALID_TABS`.
4. Remove `teams-tab.tsx` and `people-tab.tsx`.
5. Redirect `?tab=teams` and `?tab=people` to `?tab=org` for bookmark compat (one-liner in the tab resolver).

### Edge cases

- **Empty org (no teams, no people):** Show the existing `TeamTree` empty state ("No teams yet — import a directory") spanning the full width. No sidebar/table split.
- **Teams but no people:** Sidebar shows tree, table shows "No people found" empty state.
- **Hundreds of teams:** Tree already supports filtering and expand/collapse. Sidebar scrolls independently.
- **Deep nesting:** Tree indentation already handles depth with `paddingLeft: depth * 1.25rem`. At depth 5+ the sidebar width is snug but functional — team names truncate.
- **Unassigned filter + team selection:** These are mutually exclusive — clicking "Unassigned" in the filter bar should clear the team selection (and vice versa). The "Unassigned" pseudo-node in the tree and the "Unassigned" filter button should sync.

### Pros/cons summary

**Pros:**
- Both datasets visible simultaneously — no tab-switching.
- Filtering is spatial and intuitive (click team, see members).
- Tree sidebar is compact (~256px) — doesn't crowd the table.
- Reuses all existing components and queries with minimal new code.
- Degrades cleanly on narrow screens (tree becomes dropdown).
- URL-driven state survives refresh and sharing.

**Cons:**
- Two independent scroll regions side by side (manageable with `overflow-y-auto`).
- Sidebar takes 256px — on 768-1024px screens, the table has ~500-700px which is adequate but not generous.
- Slightly more complex state management than separate tabs (team selection + filters + pagination must coordinate).

---

## Option B — People Table with Inline Team Grouping

A single flat table with an optional "Group by Team" toggle that clusters rows under collapsible team headings.

```
+-------------------------------------------------------+
| Admin > Organisation                                   |
+-------------------------------------------------------+
| [Search people...]  [All|Unassigned|Inactive]          |
| [+ Add Team]  [Group by team: on/off]                  |
+-------------------------------------------------------+
| v Engineering > Backend (2)                            |
|   Alice Chen         Senior    alice@...               |
|   Bob Kim            Mid       bob@...                 |
| v Engineering > Frontend (1)                           |
|   Carol Wu           Senior    carol@...               |
| v Unassigned (3)                                       |
|   Dave Patel         —         dave@...                |
|   Eve Santos         —         eve@...                 |
|   Frank Li           —         frank@...               |
|-------------------------------------------------------+
| 1-25 of 47                         < 1 2 3 >          |
+-------------------------------------------------------+
```

**Interaction model:**
- Default: flat table (current People tab, unchanged).
- Toggle "Group by team" to cluster people under collapsible team headings.
- Team heading rows have a kebab menu (edit team, delete team).
- Click person row to open detail dialog.
- "Unassigned" group always appears last when grouped.
- Collapsible groups remember open/closed state in the session.

**Pros:** Single scroll region, no split layout. Familiar table UX. Gracefully degrades — flat mode is identical to today.

**Cons:** Loses the spatial team hierarchy (tree shows parent-child nesting; grouped table shows flattened breadcrumb paths). Grouped mode needs server support to return people ordered by team, or client-side grouping (fine for <500 people, expensive beyond). Team CRUD is secondary — tucked into group headers.

---

## Option C — Teams-First with Expandable Member Lists

The team tree is primary. Each team node expands to show its members inline. A secondary "All People" flat view is available via toggle.

```
+-------------------------------------------------------+
| Admin > Organisation                                   |
+-------------------------------------------------------+
| [+ Add Team]  [+ Import Directory]  [All People | Tree]|
+-------------------------------------------------------+
| v Engineering                    [edit] [delete]       |
|   v Backend (2)                  [edit] [delete]       |
|     +- Alice Chen    Senior                            |
|     +- Bob Kim       Mid                               |
|   > Frontend (1)                 [edit] [delete]       |
|   > Platform (0)                 [edit] [delete]       |
| v Design                        [edit] [delete]       |
|   > UX (3)                       [edit] [delete]       |
| v Unassigned (3)                                       |
|   +- Dave Patel                                        |
|   +- Eve Santos                                        |
|   +- Frank Li                                          |
+-------------------------------------------------------+
```

**Interaction model:**
- Expand a team to see its direct members listed beneath it.
- Click a person to open the detail dialog.
- Toggle to "All People" switches to the flat paginated table (current People tab).
- Team actions (edit/delete) on each row.
- URL tracks view mode: `?tab=org&view=tree` vs `?tab=org&view=people`.

**Pros:** Hierarchy is front and centre. Works well for orgs with clear team structures. Natural grouping without needing a separate panel.

**Cons:** Deep trees with many members get very long (lots of scrolling). Doesn't support search/filter/sort as naturally — those are table affordances. The "All People" toggle feels like two tabs in a trenchcoat.

---

## Option D — Resizable Split with Linked Selection

Similar to Option A but with a resizable divider and bidirectional linking: selecting a person highlights their team, selecting a team filters people.

```
+-------------------------------------------------------+
| Admin > Organisation                                   |
+-------------------------------------------------------+
| [+ Add Team]                                           |
+------------------||-----------------------------------+|
| Teams            || People           [Search...    ]   |
|                  || [All|Unassigned|Inactive]           |
| v Engineering    ||                                    |
|   * Backend  (2) || Name          Level    Email       |
|   > Frontend (1) || ---------------------------------- |
|   > Platform     || Alice Chen    Senior   alice@...   |
| v Design         || Bob Kim       Mid      bob@...     |
|   > UX       (3) ||                                    |
|                  || 1-2 of 2                            |
| Unassigned (3)   ||                                    |
+------------------||------------------------------------+
      ^^ drag to resize
```

**Interaction model:**
- Identical to Option A but the divider is draggable.
- Clicking a person in the table highlights/scrolls to their team in the tree.
- Member count badges on each team node.
- Persists split ratio in localStorage.

**Pros:** Power-user friendly. Both views fully functional simultaneously. Bidirectional linking is very discoverable.

**Cons:** Most complex to implement. Resizable split adds UX weight. Overkill if the org only has a few teams.

---

## Recommendation

**Option A (Master-Detail)** is the best balance of clarity and capability. See the detailed design above. The tab would be renamed from "Teams" / "People" to **"Organisation"** (single tab replacing two, reducing the Admin tab count from 5 to 4).

Options B-D are documented above for reference but are not recommended:
- **B** loses team hierarchy and makes team CRUD secondary.
- **C** is effectively two tabs in a trenchcoat.
- **D** adds resizable-split complexity for little gain over A.
