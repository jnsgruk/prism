# 25 — Repo Module Splitting

## Problem

`org.rs` has grown to 1,250 lines and required a `#[allow(clippy::too_many_lines)]` suppression on `import_records`. As more org functionality lands (person creation, team management, membership operations), it will only get worse. The other repos (250–484 lines) are fine today but will face the same pressure eventually.

We need a pattern for splitting large repo files that:

- Keeps the public API unchanged (`OrgRepo` stays one type, callers don't care about internal file layout)
- Groups methods by domain concern, not alphabetically
- Scales to other repos when they grow
- Doesn't introduce new crates or traits — this is just file organisation

## Approach: Directory Modules with Split `impl` Blocks

Rust allows `impl` blocks to be split across files in the same crate. We turn a single `org.rs` file into an `org/` directory module, with each file adding methods to the same `OrgRepo` struct.

### Target layout for `org`

```
repo/
├── mod.rs              # Repos bundle (unchanged)
├── org/
│   ├── mod.rs          # OrgRepo struct, types, re-exports
│   ├── teams.rs        # Team CRUD: list, get, get_all, create, update, delete
│   ├── people.rs       # Person CRUD: list, get, update, deactivate, reactivate
│   ├── memberships.rs  # Membership ops: get_team_members, assign, remove, list_unassigned
│   ├── identities.rs   # Platform identities: get_for_people, batch_resolve
│   ├── import.rs       # Directory import (import_records + its helper types)
│   └── export.rs       # Backup: count_*, export_*, reset_all, upsert_repository
├── activity.rs
├── auth.rs
├── config.rs
└── metrics.rs
```

### How it works

Each subfile uses `use super::*` (or explicit imports) to access `OrgRepo` and shared types from `org/mod.rs`, then adds an `impl OrgRepo { ... }` block with just its methods.

`org/mod.rs` contains:
- The `OrgRepo` struct definition and `new()` / `pool()`
- All shared types (`TeamWithCount`, `PersonRow`, `IdentityRow`, `ImportRecord`, `ImportIdentity`, `ImportResult`)
- `mod` declarations for each subfile

The parent `repo/mod.rs` continues to `pub use org::OrgRepo` — nothing changes for callers.

### Method grouping

| File | Methods | ~Lines |
|---|---|---|
| `mod.rs` | `OrgRepo` struct, `new`, `pool`, all type definitions | ~90 |
| `teams.rs` | `list_teams`, `get_team`, `get_all_teams`, `create_team`, `update_team`, `delete_team` | ~240 |
| `people.rs` | `list_people`, `get_person`, `update_person`, `deactivate_person`, `reactivate_person` | ~170 |
| `memberships.rs` | `get_team_members`, `assign_person_to_team`, `remove_person_from_team`, `list_unassigned_people` | ~130 |
| `identities.rs` | `get_identities_for_people`, `batch_resolve_person_ids` | ~60 |
| `import.rs` | `import_records` | ~440 |
| `export.rs` | `count_people`, `count_teams`, `export_people`, `export_teams`, `reset_all`, `upsert_repository` | ~120 |

### When to split a repo

Don't split preemptively. Split when:

1. The file exceeds ~500 lines **and** has 3+ distinct domain concerns, or
2. You need to suppress a length lint, or
3. Two people regularly conflict on the same file

Today only `org.rs` (1,250 lines) qualifies. `activity.rs` (484 lines) is borderline but has fewer distinct concerns — leave it until it grows.

## Implementation Steps

1. Create `repo/org/` directory
2. Move type definitions and `OrgRepo` struct into `org/mod.rs`
3. Create each subfile with its `impl OrgRepo` block
4. Update `repo/mod.rs` to `pub mod org` (already works — Rust resolves `org/mod.rs` the same as `org.rs`)
5. Remove the `#[allow(clippy::too_many_lines)]` from `import_records`
6. Run `prek run -av` to verify nothing broke

No public API changes. No new types or traits. Just file reorganisation.
