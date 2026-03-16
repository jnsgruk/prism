# Plan 22: API Token Management UI

## Context

The backend infrastructure for API tokens is fully implemented:
- **Database:** `auth.sessions` table with `session_type = 'api_token'` (migration 0005)
- **Repository:** `AuthRepo` with `create_session`, `list_api_tokens`, `delete_api_token` methods
- **Proto/gRPC:** `AdminService` with `CreateApiToken`, `ListApiTokens`, `RevokeApiToken` RPCs
- **Server:** `AdminService` handler in `ps-server/src/services/admin.rs` fully implemented
- **CLI:** `psctl` already consumes API tokens via `--token` flag / `PS_API_TOKEN` env var

The frontend currently has a placeholder tab at `views/sources/components/api-tokens-tab.tsx` within the Admin page (`/admin`). Per [Plan 21](21-teams-admin-split.md), `views/sources/` is being retired in favour of a new `views/admin/` feature module. This plan targets the new location directly — the API tokens tab will live in `views/admin/components/`.

The `AdminService` Connect client is already wired up in `views/sources/hooks/use-admin.ts` (currently only exposes `useResetData`); this hook file will move to `views/admin/hooks/use-admin.ts` as part of Plan 21.

## Goal

Build a working API tokens tab for the new admin section that lets users create, copy, and revoke API tokens — enabling practical use of `psctl`.

## Design

### UX Flow

1. **Token list** — table showing all tokens: name, created date, last used date, revoke button
2. **Create token** — dialog with name input, submits `CreateApiToken`, shows the raw token **once** with a copy-to-clipboard button and a warning that it won't be shown again
3. **Revoke token** — confirmation dialog, submits `RevokeApiToken`, removes from list

### Token display after creation

The raw token is only returned once from the API. The create dialog must:
- Stay open after creation, switching to a "success" view
- Display the token in a monospace read-only field
- Provide a "Copy" button using `navigator.clipboard.writeText()`
- Show a clear warning: "This token won't be shown again"
- Only close when the user explicitly dismisses it

## Implementation

### Step 1: React Query hooks (`views/admin/hooks/use-admin.ts`)

Add to the admin hooks file (which already contains `useResetData` and the `adminClient`):

```typescript
export const adminKeys = {
  all: ["admin"] as const,
  tokens: () => [...adminKeys.all, "tokens"] as const,
};

export const useListApiTokens = () =>
  useQuery({
    queryKey: adminKeys.tokens(),
    queryFn: () => adminClient.listApiTokens({}),
    select: (data) => data.tokens,
  });

export const useCreateApiToken = () => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => adminClient.createApiToken({ name }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.tokens() });
    },
  });
};

export const useRevokeApiToken = () => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (tokenId: string) => adminClient.revokeApiToken({ tokenId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.tokens() });
    },
  });
};
```

### Step 2: Create Token Dialog (`views/admin/components/create-token-dialog.tsx`)

New component with two states:

- **Form state:** name input + "Create" button
- **Success state:** read-only token display, copy button, "Done" button

Uses shadcn `Dialog`, `Input`, `Label`, `Button`. Copy uses `navigator.clipboard.writeText()` with a brief "Copied!" feedback state.

### Step 3: Revoke Token Dialog (`views/admin/components/revoke-token-dialog.tsx`)

Simple confirmation dialog with token name displayed. Uses shadcn `Dialog` + destructive `Button`. Calls `useRevokeApiToken` on confirm.

### Step 4: Token List (`views/admin/components/api-tokens-tab.tsx`)

Replace placeholder with:
- "Create Token" button (opens create dialog)
- Table with columns: Name, Created, Last Used, Actions (revoke)
- Empty state when no tokens exist
- Relative timestamps using `date-fns` `formatDistanceToNow` (already a dependency)

Uses shadcn `Table`, `Button`, `Badge`.

### Step 5: Integration tests (`tests/integration/src/api/admin.rs`)

Add Rust integration tests for the three API token RPCs:
- Create token returns valid token that authenticates
- List tokens returns created tokens
- Revoke token removes it and invalidates authentication
- Cannot revoke another user's token (if multi-user later)

## Ordering relative to Plan 21

This plan can be implemented either before or after Plan 21's `views/sources/` → `views/admin/` migration:

- **If Plan 21 lands first:** build directly in `views/admin/`. This is the preferred path.
- **If this lands first:** build in the current `views/sources/` location, then Plan 21 moves the files as part of its migration. Either way the component code is identical.

## Files Changed

| File | Action |
|------|--------|
| `frontend/views/admin/hooks/use-admin.ts` | Add query keys + 3 hooks |
| `frontend/views/admin/components/api-tokens-tab.tsx` | Replace placeholder with token list |
| `frontend/views/admin/components/create-token-dialog.tsx` | New — create + copy flow |
| `frontend/views/admin/components/revoke-token-dialog.tsx` | New — revoke confirmation |
| `tests/integration/src/api/admin.rs` | New — API token integration tests |
| `tests/integration/src/api/mod.rs` | Add `mod admin;` |

## Non-goals

- Token expiration / rotation — tokens don't expire currently, matches design
- Token scoping / permissions — all tokens inherit the user's role
- Token prefix display (e.g., showing `ps_...abc`) — could add later but raw tokens are opaque base64url
