# Frontend

Vite + React Router SPA with shadcn/ui components, Connect gRPC clients, React Query for server state, and Recharts for charts. Bun as runtime and package manager. typescript-go for type checking. Production container serves static files via Caddy.

## Stack

| Concern | Choice |
| --- | --- |
| Build tool | Vite |
| Routing | React Router (explicit routes in `app.tsx`, lazy imports) |
| UI components | shadcn/ui (built on `@base-ui/react` primitives, not Radix) |
| Server state | React Query (custom hooks with hierarchical query keys) |
| API transport | Connect (generated from protobuf, type-safe) |
| Charts | Recharts |
| Styling | Tailwind CSS |
| Icons | Lucide React |
| Toasts | Sonner |
| Type checking | typescript-go (`tsc-go`) |
| Package manager | Bun |
| Production server | Caddy (SPA fallback) |

## Layout

The frontend follows the same feature-first principles described in [01-architecture.md](01-architecture.md). Feature UI lives in `views/<feature>/` with `components/`, `hooks/`, `pages/` subdirs. Shared components and hooks are lifted only when a concrete second consumer exists.

```
frontend/
  app.tsx              # Router — lazy imports from views/, route definitions
  main.tsx             # React root — BrowserRouter, Providers, render
  globals.css          # Tailwind + shadcn theme variables
  views/               # Feature modules
    admin/             #   components/, hooks/, lib/, pages/
    ask/               #   Agentic query interface
    contributions/     #   Contribution drill-down
    ingestion/         #   components/, hooks/, pages/
    teams/             #   components/, hooks/, pages/
    people/            #   Individual profiles
    login/             #   pages/
    setup/             #   pages/
  components/          # Service-level shared: app-shell, page-header, data-table/, ui/
  lib/                 # Service plumbing: api/, hooks/ (shared), session, providers
```

## State Management

React Query is the only state management library. No nanostores, Redux, Jotai, or other global state libraries.

| State type | Tool |
| --- | --- |
| Server data (queries, mutations) | React Query |
| Component-local UI | `useState` |
| Shared UI state within a subtree | React Context |
| Persisted client preference | Cookie / `localStorage` |

If a future feature genuinely needs cross-component client state that isn't server data, prefer Zustand — lightweight and React-idiomatic.

## Proto/API Integration

`buf generate` produces TypeScript Connect clients in `frontend/lib/api/gen/`. The Connect transport auto-discovers services. Custom hooks wrapping these clients go in `lib/hooks/` if shared across features, or in `views/<feature>/hooks/` if feature-local.

## UI Conventions

See [CLAUDE.md](../CLAUDE.md) for detailed UI conventions including DataTable usage, date/number formatting, icons, empty states, loading states, badges, dialogs, page layout, charts, and search/filter patterns.

Key principles:
- **No horizontal overflow** — all content stays within viewport width
- **shadcn/ui is the standard component library** — always use `@/components/ui/` components, never hand-roll with raw Tailwind
- **Zod at boundaries only** — form validation, file uploads, localStorage reads. Not for proto responses or internal function arguments.
