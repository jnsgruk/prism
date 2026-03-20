# Plan 50: Case-Insensitive Platform Usernames

## Problem

Platform usernames are stored and compared **case-sensitively** throughout Prism, but most platforms (GitHub, Discourse, Launchpad, Mattermost) treat usernames as case-insensitive. This causes silent data loss: contributions fetched from APIs are dropped during identity resolution when the stored identity casing doesn't match the API response casing.

### Observed impact

A person with GitHub identity `Kayra1` (capital K) has 255 contributions visible on GitHub but **zero** GitHub contributions in Prism. The GitHub API returns their login as `kayra1` (lowercase), the `= ANY($2)` comparison in `batch_resolve_person_ids` fails, and every contribution is silently skipped.

This affects all platforms that use username-based resolution (GitHub, Discourse). Jira is unaffected because it resolves by opaque `account_id` via `batch_resolve_by_user_id`.

### Root cause

1. `org.platform_identities.platform_username` is `TEXT` with a case-sensitive UNIQUE constraint
2. `org.github_team_members.github_username` is `TEXT` with a case-sensitive PK
3. All SQL queries use `=` comparisons (case-sensitive in PostgreSQL)
4. No normalisation happens at any write or read boundary

## Strategy

**Normalise to lowercase everywhere.** Store all `platform_username` values in lowercase, normalise inputs to lowercase before insert or lookup.

This is the cleanest approach because:
- All affected platforms (GitHub, Discourse, Launchpad, Mattermost) are inherently case-insensitive
- Lowercase normalisation is idempotent and deterministic
- It avoids the `citext` extension dependency
- Display names (the human-readable form) are stored separately on `org.people.name`

Jira's `platform_username` (which stores an email address) also benefits — email addresses are case-insensitive per RFC 5321.

## Scope

### Database changes (migration)

**New migration: `NNNN_lowercase_platform_usernames.sql`**

Three concerns in order:

#### 1. Deduplicate conflicting identities

Before lowercasing, detect rows that would collide on `(platform, LOWER(platform_username))`:

```sql
-- Find duplicate groups
SELECT platform, LOWER(platform_username) AS normalised, array_agg(id ORDER BY id) AS ids
FROM org.platform_identities
GROUP BY platform, LOWER(platform_username)
HAVING COUNT(*) > 1;
```

For each group, keep the identity that has the most contributions linked (or the oldest if tied), and reassign the other identity's `person_id` references:

```sql
-- For each duplicate group, merge: update contributions pointing to the
-- loser identity's person_id, then delete the loser row.
-- This is wrapped in a DO block for safety.

DO $$
DECLARE
    dup RECORD;
    winner_id UUID;
    loser_ids UUID[];
BEGIN
    FOR dup IN
        SELECT platform, LOWER(platform_username) AS norm,
               array_agg(id ORDER BY id) AS ids,
               array_agg(person_id ORDER BY id) AS person_ids
        FROM org.platform_identities
        GROUP BY platform, LOWER(platform_username)
        HAVING COUNT(*) > 1
    LOOP
        -- Keep the first (oldest) identity as winner
        winner_id := dup.ids[1];
        loser_ids := dup.ids[2:];

        -- Reassign contributions from loser person_ids to winner's person_id
        UPDATE activity.contributions
        SET person_id = (SELECT person_id FROM org.platform_identities WHERE id = winner_id)
        WHERE person_id = ANY(
            SELECT person_id FROM org.platform_identities WHERE id = ANY(loser_ids)
        );

        -- Delete loser identities
        DELETE FROM org.platform_identities WHERE id = ANY(loser_ids);
    END LOOP;
END $$;
```

#### 2. Lowercase all platform usernames

```sql
-- Identities
UPDATE org.platform_identities
SET platform_username = LOWER(platform_username)
WHERE platform_username != LOWER(platform_username);

-- GitHub team members (separate table, separate PK)
UPDATE org.github_team_members
SET github_username = LOWER(github_username)
WHERE github_username != LOWER(github_username);

-- Discourse metadata in contributions (used by backfill_discourse_person_ids)
UPDATE activity.contributions
SET metadata = jsonb_set(metadata, '{username}', to_jsonb(LOWER(metadata->>'username')))
WHERE metadata->>'username' IS NOT NULL
  AND metadata->>'username' != LOWER(metadata->>'username');
```

#### 3. Re-link orphaned contributions

After identities are normalised, backfill `person_id` on any GitHub/Discourse contributions that were previously skipped due to case mismatch. These contributions wouldn't exist if they were skipped during `store_batch` (GitHub drops unresolved items), so this only applies to Discourse contributions (which store with `person_id = NULL` when unresolved):

```sql
-- Re-link Discourse contributions that now match after normalisation
UPDATE activity.contributions c
SET person_id = pi.person_id
FROM org.platform_identities pi
WHERE c.person_id IS NULL
  AND pi.platform = c.platform
  AND pi.platform_username = LOWER(c.metadata->>'username')
  AND c.platform LIKE 'discourse-%';
```

GitHub contributions that were silently skipped **do not exist in the database** — they need to be re-ingested via backfill (see Post-deploy section).

### Application code changes

#### Normalise on write (6 locations)

| File | Function | Change |
|------|----------|--------|
| `ps-core/src/repo/org/identities.rs` | `batch_ensure_identities` | `.to_lowercase()` on all usernames before insert |
| `ps-core/src/repo/org/identities.rs` | `import_jira_users` | `.to_lowercase()` on email used as `platform_username` |
| `ps-core/src/repo/org/resolutions.rs` | `resolve_identity` | `.to_lowercase()` on `platform_username` param |
| `ps-core/src/repo/org/resolutions.rs` | `manual_resolve_identity` | `.to_lowercase()` on `platform_username` param |
| `ps-core/src/repo/org/import.rs` | `upsert_identities` | `.to_lowercase()` on usernames from CSV import |
| `ps-core/src/repo/org/github_teams.rs` | `replace_github_team_members` | `.to_lowercase()` on GitHub usernames from team sync |

#### Normalise on read/lookup (4 locations)

| File | Function | Change |
|------|----------|--------|
| `ps-core/src/repo/org/identities.rs` | `batch_resolve_person_ids` | `.to_lowercase()` on input usernames before `= ANY($2)` |
| `ps-core/src/repo/org/github_teams.rs` | `get_team_mapping_suggestions` | No SQL change needed — both sides normalised at write |
| `ps-core/src/repo/activity/contributions.rs` | `backfill_discourse_person_ids` | Use `LOWER(c.metadata->>'username')` in the join |
| `ps-workers/src/github/source/store.rs` | `store_batch_impl` | `.to_lowercase()` on `platform_username` when building `person_map` key lookup |

#### Normalise at ingestion fetch boundary (3 sources)

| File | Function | Change |
|------|----------|--------|
| `ps-workers/src/github/source/fetch.rs` | PR/review construction (L490, L580) | `.to_lowercase()` on `author`/`reviewer` login |
| `ps-workers/src/discourse/source/fetch.rs` | Topic/post/like construction (L195, L335, L423) | `.to_lowercase()` on usernames from Discourse API |
| `ps-workers/src/handlers/github_team_sync.rs` | Member collection (L306) | `.to_lowercase()` on `login` from GitHub API |

### Files changed (summary)

```
migrations/NNNN_lowercase_platform_usernames.sql  (new)
crates/ps-core/src/repo/org/identities.rs          (4 functions)
crates/ps-core/src/repo/org/resolutions.rs          (2 functions)
crates/ps-core/src/repo/org/import.rs               (1 function)
crates/ps-core/src/repo/org/github_teams.rs         (1 function)
crates/ps-core/src/repo/activity/contributions.rs   (1 function)
crates/ps-workers/src/github/source/fetch.rs        (2 sites)
crates/ps-workers/src/github/source/store.rs        (1 function)
crates/ps-workers/src/discourse/source/fetch.rs     (3 sites)
crates/ps-workers/src/handlers/github_team_sync.rs  (1 site)
```

## Live data remediation

There are two categories of affected data, each requiring a different remediation path:

### Category A: Discourse contributions (stored but unlinked)

Discourse's `store_batch` stores contributions with `person_id = NULL` when the identity can't be resolved. These rows **exist** in `activity.contributions` and can be fixed purely in the migration — no re-ingestion needed.

**Handled in migration** (step 3 above): the `UPDATE ... SET person_id = pi.person_id` query re-links these rows now that both sides are normalised.

### Category B: GitHub contributions (silently dropped)

GitHub's `store_batch` **skips** contributions entirely when the identity can't be resolved (lines 37-39 of `store.rs`). These rows **do not exist** in the database. The watermark has already advanced past them, so normal ingestion won't re-fetch them. They must be re-ingested via backfill.

**Requires post-deploy backfill** — see deployment runbook below.

### Deployment runbook

Execute these steps in order after the migration has run and the normalised code is deployed:

#### Step 1: Run GitHub team sync (all sources)

Trigger `GithubTeamSyncHandler` for each GitHub source via the Ingestion UI. This normalises `github_team_members.github_username` values from the live API, ensuring Phase 2 (member search) uses lowercase usernames going forward.

#### Step 2: Backfill GitHub ingestion

For **each GitHub source**, trigger a backfill via the Ingestion UI or directly via Restate:

```
# Via Restate CLI (per source, keyed by source name e.g. "github"):
restate invocations create GithubIngestionHandler/github backfill --argument '"2025-01-01"'
```

Choose a `since_date` that pre-dates the earliest expected contribution for affected people. The backfill overrides the watermark with this date, re-fetches all PRs/reviews updated since then, and stores them — this time with correct lowercase identity resolution.

The `bulk_upsert_contributions` query uses `ON CONFLICT (platform, platform_id) DO UPDATE SET person_id = COALESCE(EXCLUDED.person_id, ...)`, so contributions that already exist will have their `person_id` updated rather than duplicated. Contributions that were previously dropped will be inserted as new rows.

**Rate limit consideration**: a broad backfill (`since_date` = 1 year ago) may consume significant GitHub API quota. Consider staggering across sources or running during off-peak hours.

#### Step 3: Backfill Discourse ingestion (if needed)

The migration's re-link query should handle most Discourse cases. However, if any Discourse sources have contributions where `metadata->>'username'` is NULL (older ingestion code paths), trigger a Discourse backfill as well:

```
restate invocations create DiscourseIngestionHandler/discourse-canonical backfill --argument '"2025-01-01"'
```

#### Step 4: Re-compute metrics

After all backfills complete, trigger metrics recomputation to recalculate team and individual snapshots with the newly linked contributions:

```
restate invocations create MetricsComputeHandler compute_current_periods
```

#### Step 5: Verify

Spot-check people who were known to have missing contributions:

1. **Kayra Gemalmaz** — should now show GitHub contributions (canonical/notary, canonical/pylego, canonical/vault-k8s-operator)
2. Run a diagnostic query to check for remaining orphaned contributions:
   ```sql
   -- GitHub contributions with no person_id (should be near zero for tracked people)
   SELECT platform, COUNT(*)
   FROM activity.contributions
   WHERE person_id IS NULL
     AND platform = 'github'
   GROUP BY platform;

   -- Identities that are still mixed-case (should be zero)
   SELECT * FROM org.platform_identities
   WHERE platform_username != LOWER(platform_username);

   -- GitHub team members still mixed-case (should be zero)
   SELECT * FROM org.github_team_members
   WHERE github_username != LOWER(github_username);
   ```
3. Confirm the person detail page in the UI shows the expected contribution counts

## What we're NOT doing

- **Not using `citext`** — adds an extension dependency, doesn't compose well with `= ANY()`, and we control all write paths so explicit normalisation is sufficient.
- **Not adding a `CHECK` constraint** — `CHECK (platform_username = LOWER(platform_username))` would be a safety net but risks breaking the migration if any edge case is missed. Can be added as a follow-up once the codebase is proven clean.
- **Not normalising `platform_user_id`** — these are opaque identifiers (Jira account IDs) that may be case-sensitive by design.
- **Not normalising `platform_id` on contributions** — these are structured identifiers like `owner/repo/pull/123` that follow their own conventions.

## Testing

- **Unit test**: add a test to `batch_resolve_person_ids` that stores an identity with mixed case and resolves it with different casing
- **Integration test**: end-to-end test in `define_source_test!` that stores a GitHub identity as `UserName`, fetches a contribution authored by `username`, and verifies it links correctly
- **Migration test**: run migration against a test database seeded with duplicate-case identities and verify deduplication + re-linking

## Risk

- **Low risk** on the application code — all changes are mechanical `.to_lowercase()` calls at well-defined boundaries
- **Medium risk** on the migration — the deduplication logic must handle edge cases (person with no contributions, identity referenced by resolution rows, etc.). The `DO` block should be tested against a staging database first
- **No data loss** — we never delete contributions; we only reassign `person_id` references and delete duplicate identity rows after merging
