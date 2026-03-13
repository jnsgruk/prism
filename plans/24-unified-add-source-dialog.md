# Plan 24 — Unified Add Source Dialog

## Problem

Adding a source is a two-step process: open the "Add Source" dialog (name + type only), close it, then open the "Edit Source" dialog from the source row to configure settings and credentials. This is clunky — users should be able to fully configure a source in a single flow.

## Goal

When adding a source, show the settings form and credentials section inline in the same dialog, so the source is fully configured before creation. Share the settings form components (`GitHubSettingsForm`, `SecretForm`, etc.) with the edit dialog rather than duplicating them.

## Current State

- **`CreateSourceDialog`** — minimal: name + type select, calls `createSource({ sourceType, name })`. No settings, no secrets.
- **`EditSourceDialog`** — full config: type-specific settings form (e.g. `GitHubSettingsForm`) + `SecretForm` for credentials. Calls `updateSource` and `setSecret` separately.
- Settings forms and `SecretForm` are defined inside `edit-source-dialog.tsx` as module-private components.
- The `CreateSourceRequest` proto already supports optional `settings` and `schedule_cron` fields — settings can be passed at creation time.
- Secrets cannot be set at creation time (requires source ID) — they must be set after creation via `SetSecret`.

## Design

### Multi-step dialog within `CreateSourceDialog`

Convert the add dialog into a two-step flow within a single dialog:

**Step 1 — Name & Type** (current behavior)
- Name input
- Source type select
- "Next" button (replaces "Create")

**Step 2 — Configure** (new)
- Shows the type-specific settings form (e.g. `GitHubSettingsForm`)
- Shows the credentials section (`SecretForm`-like, but buffered — values held in local state since no source ID yet)
- "Back" button to return to step 1
- "Create" button that:
  1. Creates the source with settings included: `createSource({ sourceType, name, settings })`
  2. On success, sets each buffered secret via `setSecret({ sourceId, secretKey, secretValue })`
  3. Closes dialog on full success

For source types with no settings form and no secrets (none currently, but defensive), skip step 2 and create immediately.

### Extract shared components

Move the following out of `edit-source-dialog.tsx` into their own files under `views/admin/components/`:

| Component | New file | Why |
|---|---|---|
| `GitHubSettingsForm` | `source-settings-forms.tsx` | Used by both create and edit dialogs |
| `SecretForm` | `secret-form.tsx` | Reused in edit dialog as-is |
| `settingsForms` registry | `source-settings-forms.tsx` | Lookup table used by both dialogs |
| `toStringArray` helper | `source-settings-forms.tsx` | Used by settings forms |

The edit dialog imports these instead of defining them inline.

### Buffered secret input for create flow

Since `SetSecret` requires a source ID, the create dialog needs a local-state version of the credentials form:

- Same UI as `SecretForm` (key selector, password input, save button replaced with inline status)
- Stores `Map<string, string>` of key→value pairs in component state
- After `createSource` succeeds, iterates over buffered secrets and calls `setSecret` for each
- Shows progress: "Creating source... Setting credentials..."
- If source creation succeeds but a secret call fails, the source still exists — show an error with guidance to set the secret from the edit dialog

A `BufferedSecretForm` component (or a `mode` prop on `SecretForm`) handles this. The simplest approach: create a new `BufferedSecretForm` that shares the same visual layout but manages local state instead of calling `setSecret` immediately. It exposes a `getSecrets(): Map<string, string>` via a ref or callback, which the parent reads at submit time.

Alternatively, `SecretForm` could accept an optional `onBuffer` callback prop — when provided, it buffers locally instead of calling the mutation. This avoids a second component but adds a mode branch. Either approach works; prefer whichever reads cleaner during implementation.

### Dialog sizing

The dialog grows with the settings form content. Use `max-w-lg` (same as edit dialog) and `max-h-[60vh] overflow-y-auto` on the content area to handle long forms.

## File Changes

| File | Change |
|---|---|
| `views/admin/components/source-settings-forms.tsx` | **New.** Extract `GitHubSettingsForm`, `settingsForms` registry, `toStringArray` |
| `views/admin/components/secret-form.tsx` | **New.** Extract `SecretForm` from edit dialog. Add buffered mode or create `BufferedSecretForm` alongside it |
| `views/admin/components/create-source-dialog.tsx` | Multi-step flow: step 1 (name+type), step 2 (settings+secrets). Import shared components. Handle creation + secret-setting sequence |
| `views/admin/components/edit-source-dialog.tsx` | Remove extracted components, import from new files. No behavior change |

## API Considerations

- `CreateSourceRequest` already has an optional `settings` field — no proto changes needed.
- Secrets are set post-creation via existing `SetSecret` RPC — no backend changes needed.
- If all secret calls fail after creation, the source row will show "Needs secret" status, which is the existing UX for unconfigured secrets.

## UX Details

- Step indicator: simple "Step 1 of 2" / "Step 2 of 2" text in the dialog header, or a subtle dot indicator. Keep it minimal.
- The "Back" button should preserve all form state (name, type, settings, secrets).
- Type change in step 1 should reset settings and buffered secrets (since they're type-specific).
- For source types with no settings form (e.g. mailing list currently), step 2 still shows the credentials section if the type has secret keys. If neither settings nor secrets exist, skip step 2.
- Loading state during creation: disable both Back and Create buttons, show spinner/text.

## Out of Scope

- Adding settings forms for non-GitHub source types (Jira, Discourse, etc.) — those are separate tasks.
- Schedule/cron configuration in the create flow.
- Changing the edit dialog's behavior or layout.
