# Plan 19 — Migrate Frontend from Next.js to Vite + React Router

## Motivation

Every Prism page is `"use client"`. We use zero Next.js-specific features — no SSR, no server components, no server actions, no ISR, no middleware, no API routes. The `app/` directory exists only because Next.js requires file-based routing, and our route files are already thin re-exports into `views/`. Next.js adds build complexity, a Node.js runtime server in production, and framework constraints we don't benefit from.

Vite + React Router gives us:
- **Faster dev server** — native ESM, no full-page reloads for route changes
- **Simpler mental model** — SPA with explicit routes in code, no file-convention magic
- **Lighter prod artifact** — static files served by any HTTP server (Caddy, nginx, or the existing Envoy gateway), no Node.js runtime needed in the container
- **Alignment with Connect** — the same pattern already runs at scale in `~/code/cortexlabsai/connect`

## Scope

### What changes

| Area | Current (Next.js) | Target (Vite + React Router) |
|---|---|---|
| Bundler / dev server | `next dev` / `next build` | `vite` / `vite build` |
| Routing | File-based `app/` directory | Explicit `<Routes>` in `app.tsx` |
| Navigation | `next/link`, `next/navigation` | `react-router-dom` (`Link`, `useNavigate`, `useLocation`) |
| Font loading | `next/font/google` | Google Fonts `<link>` in `index.html` |
| Metadata | `export const metadata` in layout | `<title>` / `<meta>` in `index.html` |
| `"use client"` directives | Required (21 files) | Removed (everything is client) |
| Production runtime | Node.js running `server.js` (standalone) | Static files served by Caddy |
| Container image | Ubuntu + Node.js (chisel: `nodejs_bins`) | Ubuntu + Caddy (chisel: `caddy_bins` or static layer) |
| Tailwind integration | `@tailwindcss/postcss` plugin | `@tailwindcss/vite` plugin (faster) |
| Path aliases | Next.js `paths` in tsconfig + Next plugin | `vite-tsconfig-paths` plugin |

### What stays the same

- `views/` structure, `components/`, `lib/` — untouched
- shadcn/ui components — framework-agnostic, no changes
- React Query hooks — no change
- Connect gRPC clients — no change
- Vitest config — already framework-agnostic
- `@ps/*` and `@/*` import aliases — same paths, different resolution mechanism
- Tailwind CSS v4 — same utility classes, just a different build plugin

## Implementation

### Phase 1 — Scaffold Vite + React Router

#### 1.1 Add new dependencies

```
bun add react-router-dom
bun add -d vite @vitejs/plugin-react @tailwindcss/vite vite-tsconfig-paths
```

#### 1.2 Remove Next.js dependencies

```
bun remove next next-themes @tailwindcss/postcss
```

Note: `next-themes` depends on Next.js internals. Replace with a lightweight `ThemeProvider` using `class` strategy on `<html>` (same approach Connect uses, ~30 lines). Or use the underlying `next-themes` pattern directly since it's mostly DOM manipulation.

#### 1.3 Create `vite.config.ts`

```ts
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import tsconfigPaths from "vite-tsconfig-paths";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss(), tsconfigPaths()],
  server: {
    port: 3000,
  },
});
```

#### 1.4 Create `index.html` (entry point)

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Prism</title>
    <meta name="description" content="Engineering insights platform" />
    <link rel="icon" href="/icon.svg" />
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=Geist:wght@100..900&display=swap" rel="stylesheet" />
  </head>
  <body class="min-h-screen bg-background text-foreground antialiased font-sans">
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

#### 1.5 Create `src/main.tsx` (React root)

```tsx
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";

import { Providers } from "@ps/providers";
import { App } from "./app";

import "@/globals.css";

createRoot(document.getElementById("root")!).render(
  <BrowserRouter>
    <Providers>
      <App />
    </Providers>
  </BrowserRouter>,
);
```

Note: `Providers` already exists and wraps QueryClient + TooltipProvider + Toaster. `BrowserRouter` wraps outside it.

#### 1.6 Create `src/app.tsx` (router)

```tsx
import { lazy, Suspense } from "react";
import { Routes, Route } from "react-router-dom";

import { AppShell } from "@/components/app-shell";

const DashboardPage = lazy(() => import("@/views/dashboard/pages/dashboard-page"));
const TeamsPage = lazy(() => import("@/views/teams/pages/teams-page"));
const SourcesPage = lazy(() => import("@/views/sources/pages/sources-page"));
const IngestionPage = lazy(() => import("@/views/ingestion/pages/ingestion-page"));
const LoginPage = lazy(() => import("@/views/login/pages/login-page"));
const SetupPage = lazy(() => import("@/views/setup/pages/setup-page"));

export const App = (): React.ReactElement => (
  <AppShell>
    <Suspense>
      <Routes>
        <Route path="/" element={<DashboardPage />} />
        <Route path="/teams" element={<TeamsPage />} />
        <Route path="/admin" element={<SourcesPage />} />
        <Route path="/ingestion" element={<IngestionPage />} />
        <Route path="/login" element={<LoginPage />} />
        <Route path="/setup" element={<SetupPage />} />
      </Routes>
    </Suspense>
  </AppShell>
);
```

As the app grows, views can own nested `<Routes>` for sub-routes (e.g. `/teams/:id`), matching the Connect pattern.

### Phase 2 — Migrate source code

#### 2.1 Replace Next.js navigation APIs

| File | Change |
|---|---|
| `components/app-shell.tsx` | `usePathname` → `useLocation().pathname`, `useRouter().replace` → `useNavigate()` |
| `components/app-sidebar.tsx` | `Link` from `react-router-dom`, `usePathname` → `useLocation().pathname` |
| `app/page.tsx` → `views/dashboard/` | `Link` from `react-router-dom` |
| `app/login/page.tsx` → `views/login/` | `useRouter().replace` → `useNavigate({ replace: true })` |
| `app/setup/page.tsx` → `views/setup/` | Same as login |

#### 2.2 Move remaining `app/` pages into `views/`

The `app/` directory is deleted entirely. Pages that are still single files move into views:

| Current location | New location |
|---|---|
| `app/page.tsx` (dashboard, 54 LOC) | `views/dashboard/pages/dashboard-page.tsx` |
| `app/login/page.tsx` (93 LOC) | `views/login/pages/login-page.tsx` |
| `app/setup/page.tsx` (106 LOC) | `views/setup/pages/setup-page.tsx` |
| `app/ingestion/page.tsx` (23 LOC) | `views/ingestion/pages/ingestion-page.tsx` |
| `app/admin/page.tsx` (re-export) | Deleted — `app.tsx` imports directly |
| `app/teams/page.tsx` (re-export) | Deleted — `app.tsx` imports directly |

#### 2.3 Remove `"use client"` directives

Remove from all 21 files. In a Vite SPA, everything is client code.

#### 2.4 Replace `next-themes`

Write a minimal `ThemeProvider` (~30 lines) that:
- Reads preference from `localStorage`
- Applies `class="dark"` to `<html>`
- Provides `useTheme()` hook

Or simply inline the logic if we're not currently using theme switching (check if we are).

#### 2.5 Handle font loading

Replace `next/font/google` (Geist) with a `<link>` tag in `index.html`. Set `--font-sans` CSS variable in `globals.css`:

```css
:root {
  --font-sans: "Geist", sans-serif;
}
```

### Phase 3 — Update build & deployment

#### 3.1 Update `package.json` scripts

```json
{
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview",
    "typecheck": "tsgo --build tsconfig.build.json",
    "lint": "oxlint .",
    "test": "vitest run"
  }
}
```

#### 3.2 Update `tsconfig.json`

Remove Next.js plugin and `.next/` type includes:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["dom", "dom.iterable", "ES2022"],
    "strict": true,
    "noEmit": true,
    "module": "esnext",
    "moduleResolution": "bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "jsx": "react-jsx",
    "noFallthroughCasesInSwitch": true,
    "noUncheckedIndexedAccess": true,
    "paths": {
      "@ps/*": ["./lib/*"],
      "@/*": ["./*"]
    }
  },
  "include": ["src", "**/*.ts", "**/*.tsx"],
  "exclude": ["node_modules", "dist"]
}
```

Add a `src/vite-env.d.ts`:
```ts
/// <reference types="vite/client" />
```

#### 3.3 Update `tsconfig.build.json`

Same changes — remove Next.js plugin reference.

#### 3.4 Rewrite `Dockerfile`

The production image no longer needs Node.js. Vite produces static files in `dist/`.

**Option A: Caddy** (recommended — simple, automatic HTTPS, SPA fallback built-in)

```dockerfile
# syntax=docker/dockerfile:1

# Builder — install deps, build static SPA
FROM oven/bun:1 AS builder
WORKDIR /app
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile
COPY . .
RUN bun run build

# Dev runtime
FROM caddy:2-alpine AS ps-frontend-dev
COPY Caddyfile /etc/caddy/Caddyfile
COPY --from=builder /app/dist /srv
EXPOSE 3000

# Prod runtime (chisel-slimmed Ubuntu + Caddy, or just caddy:2-alpine)
FROM caddy:2-alpine AS ps-frontend
COPY Caddyfile /etc/caddy/Caddyfile
COPY --from=builder /app/dist /srv
EXPOSE 3000
```

**Caddyfile:**
```
:3000 {
  root * /srv
  file_server
  try_files {path} /index.html
  encode gzip
}
```

Note: If we want to keep the Ubuntu-based chisel approach for consistency with backend images, we can install Caddy (or even just a Go static file server) into the chisel rootfs instead. The key point is we no longer need Node.js.

**Option B: Serve via Envoy Gateway directly** — if the k8s Envoy gateway can serve static files or we mount the dist as a volume, we might not even need a container-level web server. Worth evaluating but Caddy is the safe default.

#### 3.5 Remove Next.js artifacts

Delete:
- `next.config.ts`
- `next-env.d.ts`
- `postcss.config.mjs` (Tailwind now via Vite plugin, not PostCSS)
- `.next/` directory (add to `.gitignore` cleanup)
- `app/` directory (fully replaced by `src/app.tsx` + `views/`)
- `app/globals.css` → move to `globals.css` at frontend root (or keep in `app/` — wherever `index.html` references it)

### Phase 4 — Update documentation

#### 4.1 Plans to update

| File | Changes needed |
|---|---|
| `plans/01-architecture-overview.md` | Replace "Next.js App Router" with "Vite + React Router SPA" |
| `plans/05-frontend-strategy.md` | Major rewrite — remove App Router, server components, route groups. Describe Vite SPA architecture, `app.tsx` router, lazy loading |
| `plans/11-implementation-plan.md` | Update stack summary |
| `plans/12-phase1-foundation.md` | Update W3 Frontend Scaffolding to reflect Vite setup |
| `plans/13-phase2-breadth.md` | Update any Next.js references |
| `plans/16-app-shell-layout.md` | Remove route group references, update to describe `AppShell` wrapping `<Routes>` |

#### 4.2 Project docs

| File | Changes needed |
|---|---|
| `CLAUDE.md` | Replace all Next.js references. Update build commands (`bun dev` → `vite`), frontend structure (remove `app/`), framework description |
| `README.md` | Update tech stack line, directory structure |

#### 4.3 CI pipeline

Update `.github/workflows/ci.yml` frontend job if it references any Next.js-specific commands (currently just uses `bun run` which delegates to package.json scripts — should work after script updates).

### Phase 5 — Lock file & verification

#### 5.1 Regenerate lock file

```bash
cd frontend && rm bun.lock && bun install
```

This ensures the entire Next.js dependency tree (50+ transitive deps) is purged.

#### 5.2 Verify

- `bun run dev` — dev server starts, all routes work
- `bun run build` — produces `dist/` with correct chunks
- `bun run test` — vitest still passes (already framework-agnostic)
- `bun run typecheck` — no type errors
- `bun run lint` — oxlint clean
- `prek run -av` — full check passes
- Docker build — `docker build --target ps-frontend frontend/` produces working image
- Test SPA routing — direct navigation to `/teams`, `/admin` etc. returns `index.html` (Caddy `try_files`)

## File inventory

### Files to create
- `frontend/vite.config.ts`
- `frontend/index.html`
- `frontend/src/main.tsx`
- `frontend/src/app.tsx`
- `frontend/src/vite-env.d.ts`
- `frontend/Caddyfile`
- `frontend/views/dashboard/pages/dashboard-page.tsx` (moved from `app/page.tsx`)
- `frontend/views/login/pages/login-page.tsx` (moved from `app/login/page.tsx`)
- `frontend/views/setup/pages/setup-page.tsx` (moved from `app/setup/page.tsx`)
- `frontend/views/ingestion/pages/ingestion-page.tsx` (moved from `app/ingestion/page.tsx`)

### Files to delete
- `frontend/next.config.ts`
- `frontend/next-env.d.ts`
- `frontend/postcss.config.mjs`
- `frontend/app/` (entire directory)

### Files to modify
- `frontend/package.json` — swap deps and scripts
- `frontend/tsconfig.json` — remove Next plugin and `.next` includes
- `frontend/tsconfig.build.json` — same
- `frontend/Dockerfile` — static file serving, no Node.js
- `frontend/components/app-shell.tsx` — react-router-dom hooks
- `frontend/components/app-sidebar.tsx` — react-router-dom Link + useLocation
- `frontend/lib/providers.tsx` — remove `"use client"`, possibly add ThemeProvider
- `frontend/app/layout.tsx` → logic absorbed into `index.html` + `src/main.tsx`
- 21 files — remove `"use client"` directive
- `frontend/.gitignore` — replace `.next` with `dist`
- All plan/doc files listed in Phase 4

## Risks & mitigations

| Risk | Mitigation |
|---|---|
| SPA routing returns 404 on direct navigation | Caddy `try_files` / Envoy rewrite rule — standard SPA pattern |
| Font loading flash (FOUT) | `font-display: swap` in Google Fonts URL + preconnect hints |
| Theme flash on load | Inline `<script>` in `index.html` that reads localStorage and sets class before React hydrates (same pattern as Connect) |
| shadcn/ui components break | They don't depend on Next.js — they use `@base-ui/react` primitives and `cn()`. No risk. |
| Envoy Gateway routing changes needed | Current setup likely proxies to the frontend container on port 3000 — Caddy listens on 3000, so no gateway changes needed |

## Ordering

This migration can be done atomically in a single branch. The total changeset is modest:
- ~10 new files (mostly small: config, entry point, router, moved pages)
- ~10 deleted files
- ~25 modified files (mostly removing `"use client"` one-liners)
- ~8 doc files updated

Estimated effort: single session. No breaking changes to backend, gRPC, or data layer.
