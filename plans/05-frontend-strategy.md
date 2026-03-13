# Frontend Strategy

## Stack

| Choice | Rationale |
|--------|-----------|
| Next.js (App Router) | Server components for data-heavy pages, client components for interactive dashboards |
| React | Component model, ecosystem |
| shadcn/ui + Base UI | shadcn/ui for composable owned components; `@base-ui/react` primitives for accessible, unstyled building blocks (dialogs, popovers, dropdowns, etc.) |
| nanostores | Lightweight atomic state management — simple, framework-agnostic, no boilerplate |
| React Query (TanStack) | Server state management — caching, refetching, optimistic updates for API data |
| TypeScript (strict) | Non-negotiable — full strict mode; type-checked with typescript-go (Go-based `tsc` rewrite) |
| Bun | Runtime, package manager, and test runner — fast, unified tooling, native TS execution |
| oxlint + oxfmt | Fast Rust-based linting and formatting from the oxc project; replaces ESLint + Prettier |
| Zod | Runtime schema validation at system boundaries — forms, URL params, env config, sessionStorage reads. Complements proto-generated compile-time types with runtime safety |
| Tailwind CSS v4 | shadcn/ui is built on it; utility-first keeps styles co-located |

## Authentication

All API calls require a valid session token, enforced by an async tonic interceptor on the backend. See [07-authentication.md](./07-authentication.md) for the full auth design.

### Token Handling

The session token is stored in a module-scoped variable (in-memory) with `sessionStorage` as a fallback so it survives page refreshes within a tab. A Connect transport interceptor attaches the token as `Authorization: Bearer <token>` metadata on every RPC.

If any RPC returns `UNAUTHENTICATED`, the frontend clears the token and redirects to `/login`.

### Auth Pages

| Page | Purpose |
|------|---------|
| `/setup` | First-run wizard — shown when no admin user exists. Username, display name, password form. Calls `AuthService.CompleteSetup`, stores returned token, redirects to dashboard. Only accessible when `GetSetupStatus` returns `setup_complete = false`. |
| `/login` | Username + password form. Calls `AuthService.Login`, stores token, redirects to dashboard. |

### Auth Flow on App Load

The root layout calls `AuthService.GetSetupStatus` on load:
- If `setup_complete = false` → redirect to `/setup`
- If `setup_complete = true` and no valid token → redirect to `/login`
- If valid token → render the app

## Connecting to the Rust Backend

**`@connectrpc/connect-web`** with **Buf CLI** for protobuf management and code generation.

- `buf` manages `.proto` files: linting, breaking change detection, code generation
- `@connectrpc/connect-web` generates type-safe TypeScript clients from proto definitions
- Works over standard HTTP/1.1 (no gRPC-Web proxy needed)
- The Rust server exposes services via `tonic` + `tonic-web`; Connect protocol is wire-compatible

### Buf Workflow

```sh
# Lint and check proto files
buf lint
buf breaking --against .git#branch=main

# Generate TypeScript clients + Rust server stubs
buf generate
```

`buf.gen.yaml` configures generation for both languages:
```yaml
version: v2
plugins:
  # TypeScript (frontend)
  - remote: buf.build/connectrpc/es
    out: frontend/lib/api/gen
  # Rust (backend) — or use tonic-build directly
  - remote: buf.build/community/neoeinstein-prost
    out: crates/ps-proto/src/gen
```

## Page Structure

### Auth Views

1. **Setup** (`/setup`) — first-run wizard, creates initial admin user
2. **Login** (`/login`) — username/password authentication

### Primary Views (require authentication)

1. **Dashboard / Home**
   - High-level overview: ingestion health, key metrics across orgs
   - Quick links to team comparisons, recent insights

2. **Team Comparison**
   - Side-by-side or table view of teams with key metrics
   - Selectable time period (week, month, quarter, custom)
   - Drill down into a specific team
   - Metric categories: DORA, flow, review quality, cross-platform activity

3. **Team Detail**
   - Deep view of a single team's metrics over time
   - Trend charts (are things improving or degrading?)
   - Member list with activity summaries
   - AI-generated insights for this team

4. **Individual Profile**
   - Cross-platform contribution summary for a person
   - Activity distribution (how much GitHub vs Jira vs Discourse?)
   - Peer comparison context (others at same level)
   - Time period selector

5. **Ingestion Status**
   - Per-source status: last run, next run, current state
   - Rate limit visibility: are we waiting? how long?
   - Historical run log with durations and error counts
   - Manual trigger controls (backfill, force re-run)

6. **Configuration / Admin**
   - Team/people management (directory import trigger)
   - Source configuration
   - Schedule management

### Future Views
- AI Insights feed (cross-team observations)
- Custom query / exploration interface (agentic)

## Component Architecture

> **Note:** The structure below predates the feature-first reorganisation. Frontend code now uses `views/<feature>/` for feature-specific UI (components, hooks, pages) with `app/` routes as thin re-exports. See [18-code-structure.md](./18-code-structure.md) for current conventions.

```
app/
├── layout.tsx              # Root layout — auth check, nav (when authenticated)
├── page.tsx                # Dashboard/home
├── setup/
│   └── page.tsx            # First-run wizard (unauthenticated)
├── login/
│   └── page.tsx            # Login form (unauthenticated)
├── teams/
│   ├── page.tsx            # Team comparison view
│   └── [teamId]/
│       └── page.tsx        # Team detail
├── people/
│   └── [personId]/
│       └── page.tsx        # Individual profile
├── ingestion/
│   └── page.tsx            # Ingestion status
└── admin/
    └── page.tsx            # Configuration

components/
├── ui/                     # shadcn/ui components (Button, Card, Dialog, etc.)
├── charts/                 # Chart components (likely recharts or similar)
├── metrics/                # Metric display components
│   ├── MetricCard.tsx
│   ├── DORAMetrics.tsx
│   ├── FlowMetrics.tsx
│   └── ReviewMetrics.tsx
├── team/
│   ├── TeamTable.tsx
│   └── TeamComparisonChart.tsx
├── ingestion/
│   ├── SourceStatusCard.tsx
│   └── RunHistoryTable.tsx
└── layout/
    ├── Nav.tsx
    └── PeriodSelector.tsx

lib/
├── api/                    # Generated Connect/gRPC clients (from buf generate)
├── session.ts              # Token storage (in-memory + sessionStorage fallback)
├── stores/                 # nanostores — app-level state (selected period, active team, UI state)
├── hooks/                  # React Query hooks wrapping Connect clients
└── utils/                  # Formatting, date helpers
```

## State Management

Two complementary layers:

### nanostores — Client/UI State
Lightweight atomic stores for state that doesn't come from the server:
- Selected time period, active filters
- UI state (sidebar open, comparison selections)
- Authentication state (setup complete, logged in)

```typescript
import { atom } from 'nanostores'

export const $selectedPeriod = atom<Period>({ type: 'month', start: '2026-02-01' })
export const $comparisonTeamIds = atom<string[]>([])
```

### React Query (TanStack Query) — Server State
All API data flows through React Query:
- Caching, background refetching, stale-while-revalidate
- Connect client calls wrapped in query hooks
- Optimistic updates where appropriate (e.g. toggling a source enabled/disabled)

```typescript
import { useQuery } from '@tanstack/react-query'
import { createConnectTransport } from '@connectrpc/connect-web'

export function useTeamMetrics(teamId: string, period: Period) {
  return useQuery({
    queryKey: ['team-metrics', teamId, period],
    queryFn: () => client.getTeamMetrics({ teamId, ...period }),
  })
}
```

## Data Fetching Strategy

- **Server Components** for initial page loads — fetch data on the server, render HTML
- **Client Components** with React Query for interactive features — period changes, drill-downs, live ingestion status
- **Streaming** for ingestion status page — server-sent events or polling for real-time job status

## Charting

**Tremor** — built on Recharts with a shadcn/ui-like philosophy, purpose-built for metrics dashboards. Use this as the primary charting library. It provides bar charts, line charts, area charts, and KPI cards that align well with the shadcn/ui aesthetic out of the box.

## Tooling Configuration

```json
// oxlint: strict rules
{
  "rules": {
    "no-explicit-any": "error",
    "no-unused-vars": "error",
    "prefer-const": "error"
  }
}
```

TypeScript `tsconfig.json`:
```json
{
  "compilerOptions": {
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true
  }
}
```
