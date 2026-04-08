---
paths:
  - "frontend/**"
---

# Frontend UI Rules

## Stack

Vite + React Router SPA + shadcn/ui (`@base-ui/react` primitives) + TypeScript (strict, typescript-go) + Connect clients + React Query + Recharts + Bun + Caddy. Components use `@ps/cn` for the `cn` helper.

## State Management

React Query is the **only** state management library. No Redux, Jotai, nanostores.

| State type | Tool |
| --- | --- |
| Server data | React Query (custom hooks with hierarchical query keys) |
| Component-local UI | `useState` |
| Shared UI within subtree | React Context |
| Persisted preference | Cookie / `localStorage` |

If cross-component client state is genuinely needed in future, prefer Zustand.

## Zod — Boundaries Only

Use Zod for: form validation (multi-field, cross-field rules), file uploads, localStorage/cookie reads.
Do **not** use for: proto responses (Connect handles this), simple `required` checks, internal function args.

## Tables — DataTable

All tables use the shared `DataTable` (`components/data-table/`) on TanStack React Table v8. Never build from raw `<Table>` primitives.

- Manual sorting (`manualSorting: true`) — server-driven via gRPC `sort_field`/`sort_ascending`
- Pagination via `DataTablePagination` — "1–10 of 47" (en-dash), page size 10/25/50/100
- Empty state: "No results." centered
- Overflow: wrap in `<div className="overflow-x-auto rounded-md border">`
- Filters reset page index to 0

## Date & Time

- **24-hour clock only** — never 12-hour or AM/PM
- **Short format**: `toLocaleDateString(undefined, { month: "short", day: "numeric" })` + `toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false })` → "Mar 16 14:30"
- **Relative time** for recent events: "5m ago", "2h ago" — full date for older
- **ISO 8601** for API values and period selectors

## Numbers

- Whole numbers: `String(n)` or `.toLocaleString()` for large values
- Decimals: `.toFixed(1)` + suffix — `"2.5h"`, `"1.2d"`
- Percentages: `Math.round(percent)` + `%`
- Tabular: `className="tabular-nums"`
- No data: em-dash `"—"` (not "N/A" or "0")

## Icons — Lucide React

Only icon library. `size-4` for buttons/tables, `size-3`/`size-3.5` for badges, `size-6` for headings, `size-10` for empty states. Spinner: `Loader2` + `animate-spin`. Secondary: `text-muted-foreground`.

## Empty States

```jsx
<div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
  <Icon className="size-10 text-muted-foreground" />
  <p className="mb-1 font-medium">Title</p>
  <p className="text-sm text-muted-foreground">Description</p>
</div>
```

## Loading States

- Full-page: centered `<Loader2 className="size-6 animate-spin text-muted-foreground" />`
- Skeletons: `<Skeleton className="h-10 w-full" />`
- Buttons: `<Loader2 className="mr-1.5 size-3.5 animate-spin" />` + "Saving...", disabled

## Toasts — Sonner

`toast.success("Created")`, `toast.error("Failed")`. Fire in mutation callbacks. Extract error: `err instanceof Error ? err.message : "Default"`.

## Badges

| State | Variant |
| --- | --- |
| Active, merged, approved | `default` |
| Counts, secondary info | `secondary` |
| Error, closed, inactive | `destructive` |
| Neutral, open | `outline` |

State text: `text-[10px] uppercase`. Icon badge: `className="gap-1"`.

## Search & Filter

- Search: `<Input>` with `Search` icon (size-3.5), `pl-8`. Debounced 300ms.
- Filter toggles: `variant="default"` active, `variant="outline"` inactive. `flex items-center gap-1`.
- Select dropdowns for categorical filters.

## Dialogs & Forms

- Structure: `DialogHeader` → form body → `DialogFooter` (Cancel + action)
- Scrollable: `max-h-[60vh] overflow-y-auto`
- Fields: `space-y-4` between fields, `space-y-2` label-to-input
- Validation: HTML5 `required` for simple; Zod + react-hook-form for complex
- Submit: `type="submit"`, `disabled={isPending}`, "Saving..."/"Creating..."
- Errors: `Alert variant="destructive"` above footer

## Page Layout

- `PageHeader`: `h-14` bar with `SidebarTrigger | Separator | Title + Description | Actions`
- Content: `<div className="min-w-0 flex-1 space-y-6 overflow-y-auto p-6">`
- Metric grids: `grid grid-cols-2 lg:grid-cols-4 gap-4`
- Section spacing: `space-y-6` top-level, `space-y-3`/`space-y-4` within cards

## Collapsible Sections

shadcn `Collapsible` in cards. Header is trigger with `ChevronDown`/`ChevronRight`. Count badge: `<Badge variant="secondary" className="ml-1">{count}</Badge>`.

## Links & Navigation

- Internal: React Router `<Link>`, wrapped in Button via `render={<Link to="/path" />}`
- External: `target="_blank" rel="noopener noreferrer"` + `ExternalLink` icon (size-3)
- URL state: `useSearchParams()` for filter/pagination
- Back: `ArrowLeft` icon button + `history.back()`

## Charts — Recharts

- `<ResponsiveContainer width="100%" height={300}>`
- Grid: `strokeDasharray="3 3" className="stroke-border"`
- Axes: `tick={{ fontSize: 12 }} className="fill-muted-foreground"`
- Bars: `radius={[4, 4, 0, 0]}`
- Colors: HSL CSS variables — `hsl(var(--primary))`
