# Plan 16 — App Shell & Layout Redesign

## Problem

The frontend has no navigation. Every page is a standalone island — users must know URLs to get around. There's no sense of "where am I" or "who am I logged in as." Login and setup pages use raw HTML elements instead of the design system. The app doesn't feel like a cohesive platform.

## Current State

**Routes:**
| Route | Purpose | Auth? |
|-------|---------|-------|
| `/` | Dashboard (just says "Welcome, {name}") | Yes |
| `/login` | Login form | No |
| `/setup` | Initial admin account creation | No |
| `/teams` | Team list + detail panel | Yes |
| `/admin` | Sources config + API tokens tabs | Yes |
| `/ingestion` | Placeholder | Yes |

**What exists:**
- 12 shadcn/ui components installed (Button, Card, Input, Label, Select, Tabs, Badge, Table, Alert, Separator, DropdownMenu, Dialog)
- Sidebar CSS variables already defined in `globals.css` (`--sidebar`, `--sidebar-foreground`, etc.)
- `useCurrentUser()` hook returns `displayName` and user info
- `useLogout()` hook exists but is unused (no logout button anywhere)
- Lucide icons available (~1300 icons)
- Dark mode CSS variables defined but no toggle exists

**What's wrong:**
- No shared layout shell — each page renders full-screen with its own `p-8` padding
- Login/setup use raw `<input>`, `<button>`, `<label>` instead of shadcn components
- No sidebar, top bar, or any navigation element
- No user menu, no logout button
- Admin page uses hand-rolled tab buttons instead of shadcn Tabs
- Dialogs use hand-rolled `fixed inset-0` overlays instead of shadcn Dialog
- No loading states or skeleton screens during auth checks (pages flash `null`)

## Design

### Layout Architecture

```
┌─────────────────────────────────────────────┐
│ (login/setup - full-screen centered, no shell)
└─────────────────────────────────────────────┘

┌──────┬──────────────────────────────────────┐
│      │ Header: breadcrumb / page title      │
│ Side │──────────────────────────────────────│
│ bar  │                                      │
│      │ Page Content                         │
│      │                                      │
│ Nav  │                                      │
│ User │                                      │
│ Menu │                                      │
└──────┴──────────────────────────────────────┘
```

**Two layout zones:**

1. **Public layout** — Login and setup pages. Full-screen centered card, Prism branding, no nav. Clean and focused.
2. **Authenticated layout** — Collapsible sidebar + header + content area. Wraps all protected routes.

### Sidebar

Collapsible left sidebar (expanded ~240px, collapsed ~48px icon-only). Persists collapse state in `localStorage`.

**Nav items (top section):**
- Dashboard (LayoutDashboard icon) → `/`
- Teams (Users icon) → `/teams`
- Ingestion (Activity icon) → `/ingestion`

**Nav items (bottom section):**
- Admin / Settings (Settings icon) → `/admin`

**Footer:**
- User avatar (initials) + display name + role
- DropdownMenu on click: "Log out" (uses existing `useLogout` hook)
- Collapse/expand toggle button

### Header Bar

Lightweight top bar within the content area:
- Page title (driven by route)
- Optional: breadcrumbs if we add nested routes later
- Keeps it simple for now — just context for where you are

### New shadcn Components Needed

Install via `bunx shadcn@latest add <name>`:
- **Sidebar** — shadcn has a full sidebar component (`bunx shadcn@latest add sidebar`). Uses the `--sidebar-*` CSS variables already in our theme. Comes with `SidebarProvider`, `SidebarTrigger`, `SidebarMenu`, `SidebarMenuItem`, `SidebarMenuButton`, etc. This is the right foundation.
- **Tooltip** — for icon-only collapsed sidebar state
- **Sheet** — for mobile responsive sidebar (slides in as drawer)
- **Skeleton** — for loading states during auth checks

### Component Breakdown

#### New Files

| File | Purpose |
|------|---------|
| `components/app-sidebar.tsx` | Sidebar nav component with menu items and user footer |
| `components/page-header.tsx` | Reusable page header with title |
| `app/(authenticated)/layout.tsx` | Route group layout wrapping all protected pages with sidebar + auth guard |
| `app/(public)/layout.tsx` | Route group layout for login/setup with centered branding |

#### Moved Files (Route Groups)

Next.js route groups `(authenticated)` and `(public)` don't affect URLs but let us apply different layouts:

```
app/
├── (public)/
│   ├── layout.tsx          # Centered card layout with Prism branding
│   ├── login/page.tsx      # Existing, upgraded to use shadcn components
│   └── setup/page.tsx      # Existing, upgraded to use shadcn components
├── (authenticated)/
│   ├── layout.tsx          # Sidebar + header + auth guard
│   ├── page.tsx            # Dashboard (existing /)
│   ├── teams/page.tsx      # Existing
│   ├── admin/page.tsx      # Existing
│   └── ingestion/page.tsx  # Existing
└── layout.tsx              # Root (unchanged — fonts, providers)
```

#### Auth Guard

Currently every protected page duplicates the same auth/setup check logic. Extract this into the `(authenticated)/layout.tsx`:

```tsx
// Pseudocode
const AuthenticatedLayout = ({ children }) => {
  const { data: setupComplete, isLoading: setupLoading } = useSetupStatus();
  const { data: user, isLoading: userLoading, isError } = useCurrentUser();

  if (setupLoading || userLoading) return <LoadingSkeleton />;
  if (!setupComplete) redirect("/login"); // or /setup
  if (isError || !user) redirect("/login");

  return (
    <SidebarProvider>
      <AppSidebar user={user} />
      <main>{children}</main>
    </SidebarProvider>
  );
};
```

This eliminates the duplicated auth checks in every page component.

### Login & Setup Page Upgrades

Both pages currently use raw HTML elements. Upgrade to shadcn:

**Changes:**
- Wrap form in `<Card>` with `<CardHeader>` (Prism logo/title) and `<CardContent>`
- Replace `<input>` → shadcn `<Input>`
- Replace `<label>` → shadcn `<Label>`
- Replace `<button>` → shadcn `<Button>`
- Replace error `<p>` → shadcn `<Alert variant="destructive">`
- Add Prism wordmark/logo above the card
- Add subtle background treatment (very light pattern or gradient) to distinguish from app chrome

**Setup page specifically:**
- Add a stepper or progress indicator feel (step 1 of 1, but designed to extend)
- Slightly more welcoming copy — this is the user's first impression

### Dashboard Page

Currently just "Welcome, {name}" centered on screen. With the sidebar providing navigation context, the dashboard becomes the main content area. For now:

- Remove the centered welcome (sidebar already shows who you are)
- Show an empty state card: "Set up your first data source to start seeing insights" with a link to Admin → Sources
- This is a placeholder until metrics/charts land in later phases

### Admin Page Cleanup

- Replace hand-rolled tab buttons with shadcn `<Tabs>` component
- Replace hand-rolled dialogs (CreateSourceDialog, SetSecretDialog) with shadcn `<Dialog>`
- Replace raw `<input>`, `<select>`, `<button>` with shadcn equivalents
- Replace raw `<label>` with shadcn `<Label>`

### Teams Page Cleanup

- Replace ImportDirectoryDialog with shadcn `<Dialog>`
- Replace raw elements with shadcn equivalents where applicable

## Implementation Order

### Step 1: Install shadcn components
```bash
cd frontend && bunx shadcn@latest add sidebar tooltip sheet skeleton
```

### Step 2: Create route groups and layouts
- Create `app/(public)/layout.tsx` — centered card layout with branding
- Create `app/(authenticated)/layout.tsx` — sidebar + auth guard
- Move pages into route groups (file moves, no code changes yet)

### Step 3: Build the sidebar
- Create `components/app-sidebar.tsx` using shadcn Sidebar primitives
- Wire up nav items with `next/link` and `usePathname()` for active state
- Add user menu footer with logout

### Step 4: Build the page header
- Create `components/page-header.tsx` — simple title + optional description
- Integrate into authenticated layout

### Step 5: Upgrade login & setup pages
- Refactor to use shadcn Card, Input, Label, Button, Alert
- Add Prism branding above form
- Improve copy and visual treatment

### Step 6: Upgrade dashboard
- Remove centered welcome
- Add getting-started empty state with links to configure sources

### Step 7: Upgrade admin page
- Swap to shadcn Tabs, Dialog, Input, Select, Label, Button
- Keep all existing logic, just swap the UI primitives

### Step 8: Upgrade teams page
- Swap ImportDirectoryDialog to shadcn Dialog
- Clean up raw elements

### Step 9: Polish
- Loading skeletons for auth checks (no more flashing null)
- Consistent page padding/spacing via the layout
- Test all routes, verify sidebar active states
- Run `prek run -av` to ensure everything passes

## Non-Goals

- Dark mode toggle (variables exist but no toggle — separate task)
- Mobile-responsive hamburger menu (nice to have, not blocking)
- Breadcrumbs (no nested routes yet)
- Notification system or alerts in header
- Refactoring gRPC hooks or backend changes — this is purely frontend layout
