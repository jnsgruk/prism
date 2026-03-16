# 23 — People Management: Editing, Reassigning, and Re-import Safety

## Problem

After a directory import, the admin has a tree of groups, teams, and squads with people assigned. But they can't _do_ anything with it:

1. **Can't edit people** — no `UpdatePerson` RPC. If someone's name is wrong, title is stale, or email changed, the only fix is to re-import the entire directory.
2. **Can't move people between teams** — no reassignment operation. The `delete_team` guard says "reassign members first" but there's no way to do it.
3. **Can't remove people** — no `DeletePerson` or deactivation. Leavers stay in the system forever.
4. **Re-import clobbers manual work** — the import upserts people by `directory_id` and matches teams by `(name, org_name)`, but hierarchy wiring only sets `lead_id` and `parent_team_id` when they're `NULL`. If an admin renames "Ben Hoyt's Team" to "Craft", a re-import creates a _new_ "Ben Hoyt's Team" alongside it. People who were manually reassigned get a second membership if the import finds them without one on the auto-named team.
5. **No visibility into who's unassigned** — senior ICs and staff who report directly to a VP don't get a team assignment. The import warns about them, but the admin has no list of "unassigned people" to work through.
6. **Teams tab edit button is a placeholder** — the pencil icon exists but does nothing.

## Current State

### What works

| Capability | How |
|---|---|
| Import people + hierarchy from directory HTML | `ImportDirectory` RPC, two-pass upsert in `OrgRepo::import_records` |
| People deduped on re-import | Matched by `directory_id` (unique), name/email/level updated |
| Teams deduped on re-import | Matched by `(name, org_name)` — reused, not duplicated |
| Memberships not duplicated | Check for active membership before inserting |
| Create/update/delete teams | RPCs exist, repo methods work, delete is guarded |
| Team tree with drill-down | `GetTeamTree` builds nested tree, frontend renders recursively |
| Admin teams tab | Tree with import button, delete button, placeholder edit button |

### What's missing

| Gap | Detail |
|---|---|
| **UpdatePerson RPC** | No way to edit name, email, level, or directory_id |
| **Reassign person to team** | No operation to end one membership and start another |
| **Remove/deactivate person** | No delete or soft-delete for leavers |
| **Unassigned people view** | No query or UI for people without an active team membership |
| **Re-import vs manual edits conflict** | Renamed teams create duplicates; hierarchy wiring skips non-NULL fields but memberships can drift |
| **Edit team dialog** | Pencil button in teams tab is non-functional |
| **Person detail in admin** | No way to view/edit a person's profile, identities, or membership from the admin tab |
| **Bulk reassignment** | Moving an entire squad or team's members requires one-by-one operations |

## Design

### Principles

1. **Import is the starting point, not the authority.** The directory HTML seeds the org structure. Manual adjustments are first-class — they're not overwritten by the next import.
2. **Re-import is additive and safe.** New people and teams are added. Existing people are updated (name, email, level). But team assignments and hierarchy that were manually changed are left alone.
3. **Every person should belong to exactly one leaf team.** Unassigned people are surfaced prominently for admin action.
4. **Membership changes are dated.** Moving someone ends the old membership (sets `end_date`) and starts a new one. Historical membership is preserved for metrics continuity.

### Re-import behaviour (the hard part)

The current import has a subtle conflict: it auto-names teams after their leader ("Ben Hoyt's Team") and matches by `(name, org_name)`. If an admin renames a team, the import can't find it and creates a duplicate.

**Solution: track the import-assigned team per person, not per team name.**

Add `import_team_name` to `org.people` — the team name the import _would_ assign this person to. On import:

1. Upsert the person (by `directory_id`) — update name, email, level, `import_team_name`.
2. If the person already has an active team membership, **don't touch it**. The admin's assignment takes precedence.
3. If the person has _no_ active membership, create one for the auto-named team (creating the team if needed).
4. Hierarchy wiring (lead_id, parent_team_id) follows the same rule: only set when `NULL`.

This means:
- First import: builds the full tree and assigns everyone. Works exactly as today.
- Admin renames "Ben Hoyt's Team" → "Craft": the rename sticks. The team's `id` is unchanged.
- Re-import: Ben's `import_team_name` updates to "Ben Hoyt's Team" but his active membership is on the "Craft" team (same UUID), so no new membership is created. No duplicate team either — the import sees Ben already has a membership and skips team creation for him.
- New person appears in directory under Ben: they have no membership, so the import creates one. If "Ben Hoyt's Team" doesn't exist (it was renamed to "Craft"), we need to resolve this — either by matching the leader's team or by creating a new one. **Better approach:** when the person's manager is known and the manager has a team, assign the person to the manager's team directly, regardless of auto-generated name.

**Revised import logic:**

```
For each person in directory:
  1. Upsert person by directory_id
  2. If person has active membership → skip team assignment
  3. If person has no membership:
     a. If person has_reports (is a leader):
        - Find existing team where lead_id = this person → use it
        - Otherwise create auto-named team
     b. If person has no reports (IC):
        - Find their manager's person_id → find team where lead_id = manager
        - Assign to that team
        - If manager has no team yet → defer (second pass)
  4. Wire hierarchy: set parent_team_id only when NULL
```

This eliminates the dependency on team names for matching. A team is identified by its leader, not its auto-generated name.

### People who left

When a directory is re-imported and someone is _missing_ from the new file:

- **Don't auto-delete.** The person may have been removed from the directory but still has historical contributions.
- **Flag them.** Add a `GetStalepeople` query: people with a `directory_id` who weren't seen in the latest import. Surface this in the admin UI as a "review leavers" list.
- **Manual deactivation.** The admin can end their team membership and optionally mark them inactive.

This requires tracking "last seen in import" — a simple `last_import_at` timestamp on `org.people`.

## Proposed Changes

### Phase A: Backend — Person and Membership Operations

#### A1. Migration: add tracking columns

```sql
ALTER TABLE org.people ADD COLUMN last_import_at TIMESTAMPTZ;
ALTER TABLE org.people ADD COLUMN active BOOLEAN NOT NULL DEFAULT true;
```

`last_import_at` is set to `NOW()` during import for every person matched by `directory_id`. `active` defaults to `true`; admins can set it to `false` for leavers. Inactive people are excluded from team member counts and metrics but retained for historical queries.

#### A2. New repo methods

| Method | Purpose |
|---|---|
| `update_person(id, name?, email?, level?)` | Edit person fields. COALESCE pattern like `update_team`. |
| `deactivate_person(id)` | Set `active = false`, end all active team memberships (set `end_date = today`). |
| `reactivate_person(id)` | Set `active = true`. Does not restore memberships — admin must reassign. |
| `assign_person_to_team(person_id, team_id)` | End any active membership, create new membership with `start_date = today`. Single transaction. |
| `remove_person_from_team(person_id, team_id)` | End the specific membership (set `end_date = today`). |
| `list_unassigned_people()` | Active people with no active team membership. |
| `list_stale_people(since)` | Active people whose `last_import_at < since` (or NULL). |
| `bulk_assign_people(person_ids[], team_id)` | Assign multiple people to one team in a single transaction. |

#### A3. New proto RPCs

```protobuf
// In OrgService:
rpc UpdatePerson(UpdatePersonRequest) returns (UpdatePersonResponse);
rpc DeactivatePerson(DeactivatePersonRequest) returns (DeactivatePersonResponse);
rpc ReactivatePerson(ReactivatePersonRequest) returns (ReactivatePersonResponse);
rpc AssignPersonToTeam(AssignPersonToTeamRequest) returns (AssignPersonToTeamResponse);
rpc RemovePersonFromTeam(RemovePersonFromTeamRequest) returns (RemovePersonFromTeamResponse);
rpc ListUnassignedPeople(ListUnassignedPeopleRequest) returns (ListUnassignedPeopleResponse);

message UpdatePersonRequest {
  string person_id = 1;
  optional string name = 2;
  optional string email = 3;
  optional string level = 4;
}

message AssignPersonToTeamRequest {
  string person_id = 1;
  string team_id = 2;
}

message RemovePersonFromTeamRequest {
  string person_id = 1;
  string team_id = 2;
}
```

#### A4. Update import logic

Modify `OrgRepo::import_records` to:

1. Set `last_import_at = NOW()` for every upserted person.
2. Skip team assignment for people who already have an active membership.
3. For new people without memberships, resolve team by leader match (not name match):
   - If the person has reports → find team where `lead_id` = person, or create one.
   - If IC → find manager's person_id → find team where `lead_id` = manager → assign there.
4. Return new warning category: `stale_people_count` — number of previously-imported people not seen in this import.

#### A5. Update existing queries

- `get_team_members` — filter by `active = true` (already filters by membership end_date).
- `list_people` — add optional `active` filter parameter.
- Member count queries — only count active people.

### Phase B: Frontend — Edit Team Dialog

#### B1. Wire up the edit team pencil button

The button exists in [teams-tab.tsx](frontend/views/admin/components/teams-tab.tsx) but does nothing. Create an `EditTeamDialog` component:

- Fields: name, team_type (read-only or selectable), parent team (dropdown), lead (person dropdown), GitHub team slug
- Pre-populated from the selected team
- Uses `useUpdateTeam` mutation (already exists in `use-admin.ts`)
- Parent dropdown: filtered to valid parents (can't reparent a group under a squad, etc.)
- Lead dropdown: shows people, searchable

#### B2. Create team dialog

Add a "Create Team" button to the teams tab header (next to Import Directory). Dialog with:

- Name, team type, parent team, lead, GitHub slug, org_name
- Parent team dropdown filters by valid hierarchy (org→group, group→team, team→squad)

### Phase C: Frontend — People Management in Admin

#### C1. People tab in admin

Add a fourth tab to the admin page: **People**. This is the home for person-level admin operations.

Layout:
- Filterable, searchable table of all people
- Columns: name, email, level, team, platform identities, status (active/inactive)
- Filter chips: "Unassigned", "Inactive", "Stale" (not seen in latest import)
- Bulk select for bulk reassignment

#### C2. Person actions

Each person row has actions:
- **Edit** — inline or dialog to update name, email, level
- **Assign to team** — dropdown/dialog to pick a team, calls `AssignPersonToTeam`
- **Remove from team** — if assigned, end their membership
- **Deactivate** — for leavers, ends all memberships and hides from active views
- **Reactivate** — brings someone back (e.g. rehire)

#### C3. Unassigned people alert

After import, if there are unassigned people, show a banner on the Teams tab:

> "12 people are not assigned to any team. [Review →]"

Clicking navigates to the People tab filtered to unassigned.

#### C4. Stale people alert

After import, if there are people whose `last_import_at` is older than the current import:

> "5 people were not found in the latest directory import. [Review →]"

Links to People tab filtered to stale.

### Phase D: Team Member Management within Team Detail

#### D1. Admin team detail panel

When a team is selected in the admin Teams tab, show a detail panel (right side or below) with:

- Team info (name, type, lead, parent) with edit button
- Member list with remove button per member
- "Add member" button → person search/select dialog
- Drag-and-drop reordering is not needed — just add/remove

#### D2. Bulk reassign

When multiple people are selected (in People tab or team detail), offer "Move to team..." action:

- Single dropdown to pick destination team
- Calls `bulk_assign_people` (ends old memberships, creates new ones)
- Confirmation: "Move 8 people from Juju Core to Juju Plugins?"

## Re-import Scenarios (worked examples)

### Scenario 1: Clean re-import, no manual changes

1. Import directory v1 → 180 people, 25 teams created
2. Import directory v2 (same people, maybe updated titles) → 0 new people, 0 new teams. People's name/email/level updated. All `last_import_at` refreshed.

**Behaviour:** Fully idempotent. No membership changes.

### Scenario 2: Admin renames a team, then re-imports

1. Import → "Ben Hoyt's Team" created (team_id = X, lead_id = Ben)
2. Admin renames team X to "Craft"
3. Re-import same directory:
   - Ben matched by `directory_id` → already has active membership on team X → skip team assignment
   - Ben's reports (Alice, Bob) → already have active memberships on team X → skip
   - No "Ben Hoyt's Team" is created because all people who would be assigned to it already have memberships

**Behaviour:** Rename preserved. No duplicates.

### Scenario 3: New person joins Ben's team in directory v2

1. Initial import: team X = "Craft" (was "Ben Hoyt's Team"), led by Ben
2. Re-import with new person Carol (depth 3, manager = Ben Hoyt):
   - Carol has no `directory_id` match → new person inserted
   - Carol has no active membership → needs assignment
   - Carol's manager = Ben → find team where `lead_id` = Ben's person_id → team X ("Craft")
   - Assign Carol to team X

**Behaviour:** Carol joins "Craft" correctly, despite the rename.

### Scenario 4: Admin moves Alice from Craft to Data, then re-imports

1. Alice was on team X (Craft), admin moves her to team Y (Data)
2. Re-import: Alice matched by `directory_id` → has active membership on team Y → skip

**Behaviour:** Manual reassignment preserved.

### Scenario 5: Someone leaves (missing from directory v2)

1. Import v1: Dave present, assigned to team
2. Import v2: Dave not in file
3. Dave's `last_import_at` stays at v1's timestamp
4. Import response includes `stale_people_count: 1`
5. Admin sees "1 person not found in latest import" banner
6. Admin reviews, confirms Dave left, clicks Deactivate

**Behaviour:** No auto-deletion. Admin decides.

### Scenario 6: Re-import after bulk manual setup

1. Admin imports directory (flat groups only, before hierarchy parsing existed)
2. Admin manually creates teams/squads, assigns people
3. New directory import with hierarchy parsing:
   - People all have active memberships → no reassignment
   - Auto-named teams not created (no one needs them)
   - Hierarchy wiring skips non-NULL parent_team_id

**Behaviour:** Manual hierarchy fully preserved.

## Migration Path

1. Add `last_import_at` and `active` columns (A1) — non-breaking, defaults handle existing data
2. Add person RPCs (A2–A3) — new endpoints, no existing behaviour changes
3. Update import logic (A4) — changes re-import behaviour to be more conservative (skip assigned people)
4. Build edit team dialog (B1–B2) — fills the placeholder
5. Build people tab (C1–C4) — new admin capability
6. Build team member management (D1–D2) — completes the admin story

Each phase is independently deployable. Phase A is the most important — it unblocks both the UI work and safe re-imports.

## Proto Changes Summary

```protobuf
// New RPCs in OrgService
rpc UpdatePerson(UpdatePersonRequest) returns (UpdatePersonResponse);
rpc DeactivatePerson(DeactivatePersonRequest) returns (DeactivatePersonResponse);
rpc ReactivatePerson(ReactivatePersonRequest) returns (ReactivatePersonResponse);
rpc AssignPersonToTeam(AssignPersonToTeamRequest) returns (AssignPersonToTeamResponse);
rpc RemovePersonFromTeam(RemovePersonFromTeamRequest) returns (RemovePersonFromTeamResponse);
rpc ListUnassignedPeople(ListUnassignedPeopleRequest) returns (ListUnassignedPeopleResponse);

// Updated message
message Person {
  string id = 1;
  string name = 2;
  optional string email = 3;
  optional string level = 4;
  repeated PlatformIdentity identities = 5;
  bool active = 6;                          // NEW
  optional string team_name = 7;            // NEW: current team name (convenience)
  optional string team_id = 8;              // NEW: current team id (convenience)
}

// Updated response
message ImportDirectoryResponse {
  int32 people_imported = 1;
  int32 teams_created = 2;
  int32 identities_mapped = 3;
  repeated string warnings = 4;
  int32 people_updated = 5;                 // NEW: existing people refreshed
  int32 stale_people_count = 6;             // NEW: people not seen in this import
}
```

## Decisions

1. **Import doesn't move people.** If someone already has a team, import leaves them there. Only new/unassigned people get auto-assigned.
2. **Teams identified by leader, not name.** Re-import resolves team assignment via `lead_id` match, not auto-generated name match. This makes renames safe.
3. **No auto-deactivation.** People missing from a directory import are flagged, not deactivated. The admin decides.
4. **Single active membership per person.** `assign_person_to_team` ends any existing membership before creating the new one. A person belongs to exactly one team at a time.
5. **Historical memberships preserved.** Ended memberships (with `end_date`) stay in the table for metrics continuity — "Alice was on Craft from Jan–Mar, then Data from Mar onward."
6. **`active` is a soft delete.** Deactivated people are hidden from member counts, team views, and metrics computation, but their historical data remains queryable.
