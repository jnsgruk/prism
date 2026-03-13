# 21 — Split Teams Page: Metrics View vs Admin Management

## Problem

The `/teams` page currently serves two distinct purposes:

1. **Metrics exploration** — viewing PR throughput, review turnaround, comparing teams, drilling into team members and their contributions
2. **Team administration** — importing directories, reorganising the hierarchy, creating/editing/deleting teams

These are different audiences and workflows. An engineering manager visiting `/teams` wants to see how their team is performing, not accidentally reorganise the org chart. Conversely, an admin setting up the org structure doesn't need metrics charts cluttering the import flow.

Combining both makes the page work harder than it needs to, and couples unrelated concerns: adding richer metrics means navigating around admin controls, and improving admin workflows means touching the metrics page.

## Design Principles

- **`/teams` is for insight** — browsing the hierarchy, viewing metrics, drilling down into people and contributions. Read-only from an org-structure perspective.
- **`/admin` is for management** — directory import, team CRUD, hierarchy reorganisation, source config, system settings. Write-heavy, used infrequently.
- The team tree component is useful in both contexts but with different affordances: click-to-drill-down (metrics) vs click-to-edit (admin).

## Current State

### `/teams` page (`views/teams/pages/teams-page.tsx`)
- Left sidebar: team hierarchy tree (expand/collapse, select)
- Right sidebar: team detail panel (members + platform identities, read-only)
- Centre: PR throughput chart, review turnaround chart, metrics comparison table
- Period selector (week/month/quarter)
- **Import Directory button** ← this is admin, not metrics

### `/admin` page (`views/sources/pages/sources-page.tsx`)
- Tabbed layout: Sources, API Tokens (placeholder)
- Source CRUD, secret management, test connection
- Reset data dialog
- No team management at all

### Hooks
- `views/teams/hooks/use-teams.ts` — contains both read hooks (`useListTeams`, `useGetTeamTree`, `useGetTeam`, `useListPeople`) and write hooks (`useCreateTeam`, `useUpdateTeam`, `useDeleteTeam`, `useImportDirectory`)
- `views/sources/hooks/use-admin.ts` — `useResetData`

## Proposed Changes

### 1. Expand `/admin` into a proper admin section

Create `views/admin/` as a dedicated admin view with its own layout, tabs, and pages:

```
views/admin/
├── pages/
│   └── admin-page.tsx          # Tabbed admin layout
├── components/
│   ├── sources-tab.tsx         # Moved from views/sources/
│   ├── source-row.tsx          # Moved from views/sources/
│   ├── edit-source-dialog.tsx  # Moved from views/sources/
│   ├── create-source-dialog.tsx
│   ├── set-secret-dialog.tsx
│   ├── reset-data-dialog.tsx   # Moved from views/sources/
│   ├── api-tokens-tab.tsx      # Moved from views/sources/
│   ├── teams-tab.tsx           # NEW — team management tab
│   ├── team-editor.tsx         # NEW — create/edit team form
│   └── import-directory-dialog.tsx  # Moved from views/teams/
├── hooks/
│   └── use-admin.ts            # Consolidate all admin mutations
```

The admin page gets three tabs:
- **Sources** — existing source configuration (moved from `views/sources/`)
- **Teams** — directory import, team CRUD, hierarchy management
- **API Tokens** — future token management

### 2. Simplify `/teams` to pure metrics exploration

Remove all write/admin functionality from the teams page:

```
views/teams/
├── pages/
│   └── teams-page.tsx          # Metrics-focused layout
├── components/
│   ├── team-tree.tsx           # Read-only hierarchy browser (click = drill down)
│   ├── team-detail-panel.tsx   # Team members + metrics detail
│   └── period-selector.tsx     # Time range for metrics
├── hooks/
│   └── use-teams.ts            # Read-only hooks only
```

Changes:
- Remove the "Import Directory" button from the teams page
- The team tree becomes purely navigational — selecting a team drills into its metrics, not an edit form
- `use-teams.ts` retains only read hooks (`useListTeams`, `useGetTeamTree`, `useGetTeam`, `useListPeople`)
- Write hooks (`useCreateTeam`, `useUpdateTeam`, `useDeleteTeam`, `useImportDirectory`) move to `views/admin/hooks/use-admin.ts`

### 3. Build the Teams admin tab

The new **Teams** tab in admin provides:

- **Import Directory** — the existing dialog, relocated here
- **Team tree with edit affordances** — reuse the tree component but with edit/delete actions per node
- **Create team** — button to add a new team (with parent selector, type, name, lead)
- **Edit team** — inline or dialog editing of team name, type, lead, parent, GitHub slug
- **Delete team** — with confirmation
- **Member management** — view and manually assign/remove people from teams (future, once the hooks exist)

The tree component can be shared between teams and admin by accepting a render prop or slot for the action area (metrics view shows member count; admin view shows edit/delete buttons).

### 4. Retire `views/sources/`

Once everything is moved to `views/admin/`, delete `views/sources/` entirely. It was always a proto-admin section; now we have a proper one.

### 5. Update routing and navigation

In `app.tsx`:
```tsx
<Route path="/teams" element={<TeamsPage />} />
<Route path="/admin" element={<AdminPage />} />
```

No route changes needed — the paths stay the same, but the components behind them change.

In `app-sidebar.tsx`: no changes needed — Teams is already in the main nav, Admin is already in the user dropdown.

## Migration Steps

This is a refactor with no backend changes. The approach is:

1. **Create `views/admin/`** — new admin page with tabbed layout
2. **Move source components** from `views/sources/` → `views/admin/components/`
3. **Move admin hooks** — consolidate write mutations into `views/admin/hooks/use-admin.ts`
4. **Move import-directory-dialog** from `views/teams/` → `views/admin/components/`
5. **Build teams-tab** in admin with tree + CRUD affordances
6. **Strip admin controls** from `views/teams/` — remove import button, keep only read-only tree + metrics
7. **Delete `views/sources/`** once empty
8. **Update `app.tsx`** to lazy-import `AdminPage` from `views/admin/`

Each step is independently committable. Steps 1–4 can land first as a pure move with no behaviour change. Steps 5–6 add the new tab and simplify the metrics page. Step 7 is cleanup.

## What This Does NOT Change

- **Backend** — no proto, service, or repo changes. All RPCs already exist.
- **Routes** — `/teams` and `/admin` stay as-is.
- **Navigation** — sidebar links unchanged.
- **Shared hooks** — `lib/hooks/use-metrics.ts` and other shared hooks stay put.
- **Team tree component** — reused across both views (possibly with a variant prop for affordances).

## Future Considerations

- The admin section is a natural home for **people management** (manual identity linking, deactivating people) when that's built.
- A **settings** tab could replace the current setup flow for changing the instance name, secret key rotation, etc.
- Role-based access control could restrict `/admin` routes without affecting `/teams` visibility.
