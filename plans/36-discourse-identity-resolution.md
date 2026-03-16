# Plan 36 — Discourse (and Jira) Identity Resolution: Stop Auto-Creating Everyone

**Status:** Proposal
**Date:** 2026-03-16

## Problem

The Discourse ingestion calls `batch_ensure_identities` in `store_batch`, which auto-creates a `org.people` record **and** a `org.platform_identities` record for every Discourse username encountered. On a busy forum this creates hundreds or thousands of people we don't care about — we only care about people already imported from the directory.

Jira already does the right thing: `store_batch` calls `batch_resolve_by_user_id` (resolve-only, no auto-create), and `import_jira_users` only creates identities for people whose email matches an existing person. But Jira relies on a manual CSV upload step to establish the mapping, which is clunky.

The goal: **only create Discourse (and Jira) platform identities for people already known in the system**, and make the mapping as automatic as possible.

## How contristat solves this

In `~/code/canonical/contristat`, Discourse identity resolution is a two-strategy approach applied to **known directory people only**:

1. **Admin API email lookup** — if an API key is configured, call `/admin/users/list/active.json?filter={email}&show_emails=true` and match by email.
2. **Username probing** — try candidate usernames (GitHub, Launchpad, Mattermost) against the public `/u/{username}.json` endpoint. First 200 response wins.

Resolution status is tracked per-person per-source: `resolved` / `unresolved` / `pending` / `manual`.

## Design for Prism

### Principle

The directory import is the source of truth for "who we care about". Platform identity resolution is about linking those known people to their accounts on external platforms. Ingestion should only attribute contributions to people it can resolve — unknown contributors get `person_id = NULL`.

### Change 1: Switch Discourse store to resolve-only

**Files:** `crates/ps-workers/src/discourse/source/store.rs`, `crates/ps-core/src/repo/org/identities.rs`

Replace the `batch_ensure_identities` call with `batch_resolve_person_ids`:

```rust
// Before (auto-creates everyone):
let person_map = ctx.repos.org.batch_ensure_identities(&platform, &users).await?;

// After (resolve-only, like Jira):
let usernames: Vec<String> = users.iter().map(|(u, _)| u.clone()).collect();
let person_map = ctx.repos.org.batch_resolve_person_ids(&platform, &usernames).await?;
```

Contributions from unknown Discourse users get `person_id = NULL`. This is the same behaviour Jira already has. The backfill query (`backfill_discourse_person_ids`) continues to work — when an identity is later created via resolution, old contributions get linked retroactively.

**Impact:** Immediate fix. All the noise stops. But contributions from real team members won't be attributed until their Discourse identity is resolved (Change 3).

### Change 2: Add a `resolution_status` tracking table

**File:** New migration

```sql
CREATE TABLE org.identity_resolutions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    person_id   UUID NOT NULL REFERENCES org.people(id) ON DELETE CASCADE,
    platform    TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',  -- pending | resolved | unresolved | manual
    resolved_at TIMESTAMPTZ,
    attempted_at TIMESTAMPTZ,
    UNIQUE (person_id, platform)
);
```

This tracks whether we've attempted to resolve each person on each platform. Statuses:

| Status | Meaning |
|--------|---------|
| `pending` | Not yet attempted — new person or new source |
| `resolved` | Successfully matched to a platform username |
| `unresolved` | Attempted but no match found |
| `manual` | Admin manually set the identity — skip auto-resolution |

When a directory import adds new people or a new Discourse/Jira source is configured, rows are inserted as `pending`. The resolution process picks these up.

### Change 3: Discourse identity resolution strategies

**File:** New module `crates/ps-workers/src/discourse/resolve.rs`

A Restate handler (or step within the ingestion flow) that resolves pending Discourse identities for known people. Runs:

- After directory import (new people to resolve)
- After adding a new Discourse source
- On-demand from admin UI

**Strategy 1: Admin API email lookup** (preferred, requires API key with admin scope)

For each pending person with a non-empty email:

```
GET {base_url}/admin/users/list/active.json?filter={email}&show_emails=true
```

If an exact email match is found, create the platform identity and mark `resolved`.

**Strategy 2: Username probing via existing identities** (fallback, no admin access needed)

For each pending person, collect their existing platform usernames (GitHub, Launchpad, etc.) from `org.platform_identities`. Probe each candidate against the public Discourse user endpoint:

```
GET {base_url}/u/{candidate_username}.json
```

First 200 response → create the platform identity, mark `resolved`. All candidates fail → mark `unresolved`.

**Strategy 3: Manual override** (always available)

Admin UI allows manually setting a Discourse username for a person. Marks status as `manual`, skips future auto-resolution.

**Rate limiting:** Discourse default is 60 req/min. For ~200 people with 3 candidates each = ~600 probes worst case = ~10 minutes at 1 req/sec. Acceptable as a one-time resolution pass. Use Restate durable sleep for backoff on 429s.

### Change 4: Jira identity resolution (replace CSV upload)

**File:** New module `crates/ps-workers/src/jira/resolve.rs`

Same pattern as Discourse but different strategy:

**Strategy 1: Jira user search by email** (primary)

Jira Cloud REST API: `GET /rest/api/3/user/search?query={email}` returns users matching email. For each pending person, search by their email. If found, create identity with `platform_user_id = accountId`.

This replaces the manual CSV upload flow in `import_jira_users`. The CSV import can remain as a fallback/manual override path but is no longer the primary mechanism.

**Strategy 2: Manual CSV upload** (existing, becomes fallback)

Keep `import_jira_users` as-is. When used, mark resolution status as `manual`.

### Change 5: Trigger resolution from directory import

**File:** `crates/ps-core/src/repo/org/import.rs` or directory import handler

After a directory import completes:

1. For each active person, ensure `identity_resolutions` rows exist for all configured source platforms (insert `pending` where missing).
2. Trigger the resolution handler for each platform that has pending people.

This means: add a person to the directory → their identities get auto-resolved on all platforms → next ingestion run attributes their contributions. No manual CSV step needed.

### Change 6: Admin UI for resolution management

**File:** Frontend views (follow-up)

On the People detail page, show resolution status per platform:

- Green check: resolved (shows username)
- Yellow clock: pending (resolution not yet attempted)
- Red X: unresolved (attempted, no match)
- Blue pencil: manual (admin override)

Allow manual override: text input for username, "Resolve" button, marks as `manual`.

Bulk actions on People list: "Re-resolve unresolved" button per platform.

## Migration Path: Clean Break

This is a breaking change for Discourse — existing auto-created people/identities will be orphaned.

1. Deploy the resolve-only change.
2. Run a cleanup migration: delete people who have **only** Discourse platform identities and no other identities, no team memberships, and were auto-created (not from directory import).
3. Run resolution for remaining people.

How to identify auto-created noise: people with no `email`, no `directory_id`, and whose only identity is on a `discourse-*` platform. The cleanup migration deletes the platform identities first (FK), then the orphaned people. Contributions from deleted identities retain their data but get `person_id` set to NULL — they can be re-linked later if someone manually adds that person.

## Implementation Order

1. **Switch Discourse to resolve-only** (Change 1) — immediate noise fix, smallest diff
2. **Add resolution tracking table** (Change 2) — foundation for resolution system
3. **Discourse email resolution** (Change 3, strategy 1) — highest-value automation
4. **Discourse username probing** (Change 3, strategy 2) — fallback for instances without admin API
5. **Wire resolution into directory import** (Change 5) — makes the flow automatic
6. **Jira email resolution** (Change 4) — replaces CSV upload ceremony
7. **Admin UI** (Change 6) — visibility and manual override
8. **Cleanup migration** (Migration Path) — remove noise records

Steps 1–2 can ship together. Steps 3–5 are the core automation. Steps 6–8 are polish.

## Files Modified

| File | Change |
|------|--------|
| `crates/ps-workers/src/discourse/source/store.rs` | Replace `batch_ensure_identities` → `batch_resolve_person_ids` |
| `migrations/NNNN_identity_resolutions.sql` | New `org.identity_resolutions` table |
| `crates/ps-core/src/repo/org/identities.rs` | Add resolution status CRUD methods |
| `crates/ps-workers/src/discourse/resolve.rs` | New: email lookup + username probing strategies |
| `crates/ps-workers/src/discourse/client.rs` | Add `admin_user_search` and `get_user` methods |
| `crates/ps-workers/src/jira/resolve.rs` | New: Jira user search by email |
| `crates/ps-core/src/repo/org/import.rs` | Trigger resolution after directory import |

## Multi-Instance Deduplication

A single person will typically have accounts on multiple Discourse instances (e.g., `discourse-ubuntu`, `discourse-snapcraft`, `discourse-devel`). Each instance is a separate `Platform::Discourse(instance)` value, so resolution runs independently per instance.

**Risk:** Without care, resolution could create duplicate people — or worse, the same username on different instances could belong to different real people.

**Safeguards:**

1. **Resolution links to existing people, never creates new ones.** Since we start from directory-imported people (Change 1), there's exactly one `org.people` record per real person. Resolution adds platform identities pointing at that same person across all instances. No duplication possible.

2. **Resolution tracks per person × per platform.** The `identity_resolutions` unique constraint is `(person_id, platform)` where platform includes the instance qualifier (e.g., `discourse-ubuntu`). So we track resolution status independently per instance — "resolved on Ubuntu forums but pending on Snapcraft" is a valid state.

3. **Username probing must be instance-aware.** When probing candidate usernames, we hit each Discourse instance's `/u/{username}.json` endpoint separately. The same person might use `jsmith` on one instance and `john.smith` on another. Each resolved username becomes a separate platform identity row, all pointing to the same `person_id`.

4. **Email lookup is the strongest signal.** If the admin API is available, email-based resolution avoids the username ambiguity entirely — same email on multiple instances reliably identifies the same person.

5. **Backfill runs per-platform.** The existing `backfill_discourse_person_ids` query already scopes by platform string, so contributions on `discourse-ubuntu` only match identities on `discourse-ubuntu`. No cross-instance contamination.

## Decisions

1. **Restate virtual object handler**, following the same pattern as `GithubTeamSyncHandler`. Defined as a `#[restate_sdk::object]` with a `resolve_identities()` method, keyed by platform string (e.g., `discourse-ubuntu`). Triggered via fire-and-forget `/send` semantics — both automatically (Discourse ingestion handler fires it after store completes, like team sync) and manually (gRPC `TriggerIdentityResolution` RPC → Restate `/IdentityResolutionHandler/{platform}/resolve_identities/send`). Uses durable `ctx.sleep()` for rate-limit backoff.

2. **Resolution runs after ingestion and on directory import, not on every run.** The `pending` status in `identity_resolutions` ensures only unresolved people are attempted. Ingestion triggers resolution because new contributions may reveal that resolution should be re-attempted (e.g., new source configured). Directory import triggers it because new people need resolving. The admin UI can also trigger it manually via the gRPC endpoint.

3. **Mailing list sources (future)?** Same resolution approach applies — resolve by email. The resolution framework is generic enough to support this, but mailing list resolution is out of scope for this work.

4. **What about Discourse users who genuinely aren't in the directory but we want to track?** Manually add the person and link their Discourse username via the admin UI. The `manual` resolution status covers this. Should be rare — the directory is the canonical source.
