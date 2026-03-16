# 25 — Server-Side Pagination, Sorting, and Filtering for Tables

## Problem

Every list RPC returns the full dataset — all people, all teams, all runs. The frontend loads everything into memory and does client-side filtering/sorting. This works at small scale but creates visible latency as datasets grow (the People tab with ~180 people already feels slow). There's no shared pattern for tabular data, so each table reinvents its own ad-hoc sorting/filtering.

We need a consistent, cross-cutting approach to paginated, sortable, filterable tables that spans proto definitions, the repository layer, gRPC services, and the frontend.

## Current State

### What works

| Capability | How |
|---|---|
| Basic table rendering | shadcn/ui table components (semantic HTML wrappers) |
| Client-side sorting | Teams page: manual `Array.sort()` with toggle state |
| Client-side filtering | People tab: search box + filter buttons, `Array.filter()` in `useMemo` |
| Data caching | React Query caches full responses by query key |

### What's missing

| Gap | Detail |
|---|---|
| **Server-side pagination** | No proto, backend, or frontend support. All RPCs return full datasets |
| **Server-side sorting** | All ORDER BY clauses are hardcoded (e.g. `ORDER BY name`). No dynamic sort field/direction |
| **Server-side filtering** | Backend has no search/filter params. All filtering happens client-side after full load |
| **Reusable table component** | No DataTable abstraction. Each table hand-rolls columns, sorting, pagination UI |
| **Page size control** | No UI or backend support for configurable page sizes |
| **TanStack Table** | Not installed — no headless table library for column defs, pagination state, sort state |

### RPCs that need pagination (ordered by data volume)

| RPC | Current behaviour | Expected scale |
|---|---|---|
| `ListPeople` | Returns all people, `ORDER BY name` | 200–5,000+ |
| `ListRuns` | `LIMIT 50` hardcoded, no offset | Thousands over time |
| `CompareTeams` | Returns metrics for all requested team IDs | 50–200 teams |
| `ListTeams` | Returns all teams with member counts | 50–200 teams |
| `ListApiTokens` | Returns all tokens for user | <20 (low priority) |
| `ListSources` | Returns all source configs | <20 (low priority) |

### Frontend tables that need the pattern

| Location | Needs pagination | Needs sorting | Needs filtering |
|---|---|---|---|
| People tab (admin) | Yes — primary driver | Yes (name, team, status) | Yes (search + status filter) |
| Ingestion runs table | Yes — currently capped at 50 | Yes (date, duration, status) | Yes (source, status) |
| Teams metrics table | Maybe — depends on org size | Yes (already has client-side) | Yes (search) |
| API tokens tab | No — small dataset | No | No |

## Design

### A. Proto: Shared pagination and table query messages

Add shared messages in a new `common.proto` (or inline in each service proto) that every list RPC can adopt:

```proto
// Common pagination envelope.
message PaginationRequest {
  int32 page_size = 1;    // Items per page. Default 50, max 500.
  string page_token = 2;  // Opaque cursor for next page. Empty = first page.
}

message PaginationResponse {
  string next_page_token = 1;  // Empty = no more pages.
  int32 total_count = 2;       // Total matching items (for "showing X of Y").
}

// Sorting.
message SortOrder {
  string field = 1;       // Column name (validated server-side against allowlist).
  bool descending = 2;    // true = DESC, false = ASC.
}
```

Each list request embeds these:

```proto
message ListPeopleRequest {
  optional bool active_only = 1;         // Existing filter.
  optional string search = 2;            // NEW: server-side ILIKE on name, email.
  optional string team_id = 3;           // NEW: filter by team.
  PaginationRequest pagination = 10;
  SortOrder sort = 11;
}

message ListPeopleResponse {
  repeated Person people = 1;
  PaginationResponse pagination = 2;
}
```

**Cursor strategy**: Use keyset pagination (not OFFSET) for stable pages under concurrent writes. The `page_token` encodes the last row's sort key + ID, base64-encoded. The backend decodes it into a `WHERE (sort_col, id) > ($1, $2)` clause.

### B. Repository layer: generic pagination helpers

Add a pagination module to `ps-core` with helpers that any repo method can use:

```rust
// ps-core/src/repo/pagination.rs

pub struct PageRequest {
    pub page_size: i32,       // clamped to 1..=500
    pub page_token: Option<PageToken>,
}

pub struct PageToken {
    pub sort_value: String,   // serialised sort key
    pub id: String,           // tie-breaker
}

pub struct PageResponse<T> {
    pub items: Vec<T>,
    pub next_page_token: Option<String>,
    pub total_count: i64,
}

impl PageRequest {
    pub fn from_proto(req: &PaginationRequest) -> Self { ... }
    pub fn effective_limit(&self) -> i64 { ... }  // page_size + 1 for has_next detection
}
```

Each repo list method adds the pagination WHERE clause and LIMIT. The `total_count` comes from a `COUNT(*)` CTE or a separate query (depending on whether the count is expensive).

**Sort field validation**: Each repo method defines an allowlist of sortable columns (e.g., `["name", "email", "team_name", "active"]` for `list_people`). Invalid sort fields return an error rather than being injected into SQL.

### C. Service layer: thin mapping

Services map between proto pagination types and repo pagination types. No business logic change — just type conversion.

### D. Frontend: TanStack Table + shared DataTable component

Install `@tanstack/react-table` and build a reusable `DataTable` component:

```
frontend/
├── components/
│   └── data-table/
│       ├── data-table.tsx          # Generic <DataTable> with column defs
│       ├── data-table-pagination.tsx  # Page size selector + prev/next buttons
│       └── data-table-toolbar.tsx     # Search input + filter controls
```

The `DataTable` component:
- Accepts TanStack Table `ColumnDef[]` and a data-fetching hook
- Manages pagination state (`pageIndex`, `pageSize`, `pageToken`)
- Manages sort state (field + direction) synced to server
- Passes search/filter params to the query hook
- Renders the shadcn/ui `Table` components underneath

Each table page provides column definitions and a hook; the `DataTable` handles the rest.

### E. React Query integration

Pagination hooks follow the pattern:

```typescript
const usePaginatedPeople = (params: {
  search?: string;
  filter?: string;
  pageSize: number;
  pageToken?: string;
  sortField?: string;
  sortDesc?: boolean;
}) => useQuery({
  queryKey: [...orgKeys.people(), params],
  queryFn: () => orgClient.listPeople({
    search: params.search,
    pagination: { pageSize: params.pageSize, pageToken: params.pageToken ?? "" },
    sort: params.sortField ? { field: params.sortField, descending: params.sortDesc ?? false } : undefined,
  }),
  placeholderData: keepPreviousData,  // Avoid flash on page change.
});
```

`keepPreviousData` from React Query keeps the previous page visible while the next page loads, avoiding layout flash.

## Implementation Plan

### Phase 1: Proto + backend pagination for ListPeople

This is the highest-impact table since it's the one that's noticeably slow.

1. **Add common pagination messages** to proto — `PaginationRequest`, `PaginationResponse`, `SortOrder`
2. **Update `ListPeopleRequest`/`Response`** with pagination, sort, and search fields
3. **`buf generate`** to regenerate Rust + TS types
4. **Add `ps-core/src/repo/pagination.rs`** with `PageRequest`, `PageToken`, `PageResponse` types and helpers
5. **Update `OrgRepo::list_people()`** to accept pagination/sort/search params, use keyset pagination
6. **Update `OrgService::list_people()`** to map proto pagination to repo types
7. **`cargo sqlx prepare`** to update offline cache

### Phase 2: Frontend DataTable component

8. **Install `@tanstack/react-table`** — `bun add @tanstack/react-table`
9. **Create `components/data-table/`** — generic DataTable, pagination controls, toolbar
10. **Create `usePaginatedPeople` hook** with React Query + `keepPreviousData`
11. **Refactor People tab** to use DataTable with server-side pagination, sorting, search

### Phase 3: Roll out to other tables

12. **Ingestion runs** — add pagination to `ListRunsRequest`, replace hardcoded LIMIT 50, update frontend
13. **Teams metrics table** — add pagination to `CompareTeamsRequest`, update frontend
14. **Any future tables** use the DataTable pattern by default

### Non-goals (for now)

- **Infinite scroll** — pagination with explicit page controls is simpler and more predictable. Can revisit if UX demands it.
- **Virtual scrolling** — only needed if a single page has 1,000+ rows, which page size limits prevent.
- **Faceted filters** — column-specific filter dropdowns (e.g., "filter by team" dropdown). Search + status toggles are sufficient for now.
- **Column visibility toggles** — all columns always visible. Can add later if tables get wide.
- **URL-synced pagination state** — pagination state lives in component state, not URL query params. Can add for deep-linking if needed.

## Migration Strategy

Existing RPCs remain backward-compatible: if `pagination` is absent or `page_size` is 0, the backend returns all results (existing behaviour). This means the frontend can adopt pagination incrementally — tables that haven't been migrated continue to work unchanged.

## Dependencies

- `@tanstack/react-table` (MIT) — headless table library for React
- No new Rust dependencies — pagination helpers use standard library types
