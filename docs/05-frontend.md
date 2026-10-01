# Frontend

Vite + React Router SPA with shadcn/ui components, Connect gRPC clients, React Query for server state, and Recharts for charts. Bun as runtime and package manager. typescript-go for type checking. Production container serves static files via Caddy.

## Stack

| Concern | Choice |
| --- | --- |
| Build tool | Vite |
| Routing | React Router (explicit routes in `app.tsx`, lazy imports) |
| UI components | shadcn/ui (built on `@base-ui/react` primitives, not Radix) |
| Server state | React Query (custom hooks with hierarchical query keys) |
| Tables | TanStack React Table v9 via the shared `DataTable` |
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

See [AGENTS.md](../AGENTS.md) and the path-scoped [frontend instructions](../frontend/AGENTS.md) for detailed UI conventions including DataTable usage, date/number formatting, icons, empty states, loading states, badges, dialogs, page layout, charts, and search/filter patterns.

Key principles:
- **No horizontal overflow** — all content stays within viewport width
- **shadcn/ui is the standard component library** — always use `@/components/ui/` components, never hand-roll with raw Tailwind
- **Zod at boundaries only** — form validation, file uploads, localStorage reads. Not for proto responses or internal function arguments.
- **Tables use the native TanStack v9 API** — register only the required features and keep the deprecated legacy bridge out of application code.

## Manual directory management

The admin organisation **+ Add → Add person** menu opens the creation form, including while viewing a team. Every new form defaults to **No team**; administrators must deliberately choose membership. Name is required; email, title/level, accounts, and team are optional. Shared person/account form components retain local drafts after errors and disable duplicate submissions. Mutation callbacks show Sonner notifications and invalidate organisation queries, including team counts and person reads.

Account rows use saved identity UUIDs for explicit add/edit/remove operations. Unchanged and unseen accounts remain intact. Discourse accounts select a separate configured instance, retaining saved instances even if their source is disabled or removed. Jira stores the opaque account ID separately from its display username. Search uses an enabled configured Jira Cloud source and requires explicit selection, displaying account ID, name, available email, and active state to distinguish ambiguous candidates. Direct account-ID entry stays available without search permission or a configured lookup source; the form labels it as manually supplied.

Jira lookup is a short, read-only, admin-only RPC. Credentials remain encrypted in configuration and are decrypted only on the server. Cloud `/rest/api/3/user/search` lookup is bounded to 50 results per page within Jira's first 1000 users, a 10-second timeout, and a 256 KiB response. Server/Data Center mode is rejected before dispatch; Jira account identities currently share one namespace across configured sources. Lookup errors explain retry/configuration/direct-entry options without exposing provider response bodies or secrets.

Saved person details expose a separate admin **Backfill activity** action. The
dialog uses saved identities and configured source UUIDs, requires a real start
date, and displays disabled, unsupported, missing-account, ambiguous-account,
Jira-mode, and Discourse-instance eligibility reasons. It respects the server's
Person capability gate until the stage #12 integration is complete. Account
edits must be saved before launching.

Backfill queries are keyed by person and exact pipeline ID. Progress and history
use owned handler runs, including rate-limit pauses and failures, rather than
global source state. Reopening recovers recent runs from server history; another
person's pipeline cannot populate the dialog. A retained submission UUID makes
failed-launch retries repeat the same intent. Pending and cancelling pipelines
also keep the global controls active; cancellation success means requested,
with terminal cancellation displayed after the server reconciles owned work.


## Ask workspace PDF previews

PDFs render through the feature-local `WorkspacePdfPreview` in
`views/ask/components/`, backed by React-PDF and PDF.js. The renderer loads
lazily behind loading and error fallbacks, uses the authenticated workspace
download's Blob URL, and displays one selected page with selectable text.
The sidebar fits pages to its resizable width; the expanded Base UI dialog adds
zoom and fit-to-width controls, with page overflow scrolling inside the viewer.
Zoom percentages multiply the fitted width, so zooming in/out grows/shrinks the
page at every container size; reopening starts in fit-to-width.
Image and text previews continue using their existing rendering paths.

`useWorkspacePreview` owns the selected conversation/path, Blob URL, page and
page count. Expanding reuses that document and preserves navigation in both
directions. A different document/conversation, preview close, sidebar close or
unmount invalidates pending downloads and text decoding. Discarded results are
revoked; published URLs are released after React removes their consumers. The
sidebar PDF renderer unmounts while the dialog is open, and the dialog renderer
unmounts on close, retaining the parent's page and URL for the sidebar.

PDF.js and its matching worker come from the same installed dependency. Vite
bundles the worker locally and copies CMaps, standard fonts, WASM and ICC profiles
into `pdf-assets/` for both development and production static delivery. Preview
rendering does not require a CDN or the browser's built-in PDF plugin. Document
JavaScript and interactive annotation/form rendering are disabled.

Empty, invalid, unsupported and password-protected documents show a clear
fallback with Download. Page rendering failures retain page navigation and
Download. Password entry, continuous scrolling, thumbnails, search, printing
and the browser Fullscreen API are outside this single-page preview scope.

### Validation evidence (2026-10-01)

An isolated browser harness using the real workspace sidebar, preview dialog
and image caller passed in Chromium 153.0.8010.12, Firefox 155.0 and Playwright
WebKit 26.6, against both development delivery and a production build served
with the repository's Caddy configuration (Caddy 2.11.4). Workspace API hooks
were replaced with synthetic downloads; this proves preview behavior and asset
delivery, not the live RPC authentication/transport or deployment.

Fixtures covered single/multi-page, portrait/landscape, scanned image-only,
embedded Greek/Cyrillic fonts, an 80-page document, invalid/empty/password files
and missing files. Checks covered actual nonblank canvases, selectable text,
page boundaries, page preservation, one download on expansion, fit-relative
zoom, sidebar resizing, a 247-character filename at a 375×320 viewport, keyboard
focus/Escape/close, stale selections, conversation changes, close while loading,
repeated URL cleanup, sidebar reopen, image/text/inline-image previews and ZIP
download. PDF resources stayed local; production worker/CMap/font/WASM/ICC
probes matched built asset bytes rather than SPA HTML.

Playwright WebKit is engine evidence, not an actual Safari run. Actual Safari
and branded Chrome verification remain pre-merge acceptance checks; the
reported private PDF was not inspected or copied into fixtures.
