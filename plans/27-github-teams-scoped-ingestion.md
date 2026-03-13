# Plan 27 — GitHub Team Discovery & Team-Scoped Ingestion

## Problem

The GitHub ingestor currently fetches **all PRs across all repos** in every configured org. For large organisations with hundreds of repos, this means:

1. **Wasted ingestion time** — fetching data for repos and people nobody in the app cares about
2. **No GitHub team awareness** — the `github_team_slug` field on `org.teams` exists but is never populated from GitHub, and there's no way to browse/assign GitHub teams from the UI
3. **No scoping** — once you add a GitHub source, you get everything; there's no way to narrow ingestion to the teams you're actually tracking

## Current State

### What the ingestor does today

1. **Plan**: Reads `settings.orgs` (list of GitHub org names), discovers all repos via `GET /orgs/{org}/repos`, filters by `exclude_archived` and `exclude_repos`
2. **Fetch**: Iterates every discovered repo, fetches all PRs (paginated, `state=all`, `sort=updated`, `direction=asc`) plus reviews for each PR, using watermark as `since` parameter
3. **Store**: Upserts contributions into `activity.contributions`, resolves `platform_username` → `person_id` via `org.platform_identities`
4. **Advance**: Writes `max_updated_at` across all fetched PRs as the new watermark

### What exists for teams

- `org.teams` table has `github_team_slug` column (nullable, never populated from GitHub)
- `org.repositories` table has `team_id` column (nullable, never populated)
- Frontend team editing can set `github_team_slug` manually but there's no discovery/search
- No GitHub Teams API calls exist in the codebase

### GitHub Teams API

GitHub's REST API provides everything we need:

| Endpoint | Purpose | Auth Scope |
|----------|---------|------------|
| `GET /orgs/{org}/teams` | List all teams in an org | `read:org` |
| `GET /orgs/{org}/teams/{team_slug}/members` | List members of a team | `read:org` |
| `GET /orgs/{org}/teams/{team_slug}/repos` | List repos accessible to a team | `read:org` |

All paginated (100 per page), standard rate limiting applies. The PAT used for GitHub ingestion needs the `read:org` scope added (currently only needs `repo` scope for PR data).

## Design

### Prerequisite: Rename `IngestionHandler` → `GithubIngestionHandler`

The current `IngestionHandler` is generic in name but GitHub-specific in implementation. Since Restate handlers should be single-purpose and we're adding a second handler (`GithubTeamSyncHandler`), rename the existing one to `GithubIngestionHandler` to make the relationship clear. This means:

- Rename the Restate object trait and impl
- Update the Restate ingress URLs in `IngestionService` (e.g. `/GithubIngestionHandler/{source_name}/run_ingestion/send`)
- Update any existing invocation ID tracking that references the old name

This is a low-risk rename — the Restate service name is only referenced in the ingestion binary and the server's trigger/cancel RPCs.

### Four capabilities

1. **Rename handler** — `IngestionHandler` → `GithubIngestionHandler`
2. **GitHub team discovery** — a new Restate workflow that syncs GitHub teams/members for configured orgs
3. **Team assignment UI** — search/filter GitHub teams and assign them to Prism teams, with auto-suggested mappings based on member overlap
4. **Team-scoped ingestion** — narrow PR fetching to only repos belonging to assigned GitHub teams

### Phase 1: GitHub Team Discovery

#### New Restate workflow: `GithubTeamSyncHandler`

A new Restate virtual object, keyed by source name, with one method:

```
#[restate_sdk::object]
trait GithubTeamSyncHandler {
    async fn sync_teams() -> Result<(), TerminalError>;
}
```

**Execution flow:**

1. `load_config` — fetch source config, decrypt token
2. `discover_teams` — for each org in `settings.orgs`:
   - `GET /orgs/{org}/teams` (paginated) → list of `GitHubTeam { id, slug, name, description }`
   - For each team: `GET /orgs/{org}/teams/{slug}/members` → list of `GitHubUser { login, id }`
   - For each team: `GET /orgs/{org}/teams/{slug}/repos` → list of repo names
3. `store_teams` — upsert into new tables (see schema below)

#### New database tables

```sql
-- New table: discovered GitHub teams (not the same as org.teams)
CREATE TABLE org.github_teams (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id       UUID NOT NULL REFERENCES config.source_configs(id) ON DELETE CASCADE,
    github_org      TEXT NOT NULL,
    github_team_id  BIGINT NOT NULL,          -- GitHub's numeric team ID
    slug            TEXT NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT,
    last_synced_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_id, github_org, slug)
);

-- Members of discovered GitHub teams
CREATE TABLE org.github_team_members (
    github_team_id  UUID NOT NULL REFERENCES org.github_teams(id) ON DELETE CASCADE,
    github_username TEXT NOT NULL,
    last_synced_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (github_team_id, github_username)
);

-- Repos associated with discovered GitHub teams
CREATE TABLE org.github_team_repos (
    github_team_id  UUID NOT NULL REFERENCES org.github_teams(id) ON DELETE CASCADE,
    github_org      TEXT NOT NULL,
    github_repo     TEXT NOT NULL,
    last_synced_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (github_team_id, github_org, github_repo)
);

-- Junction: which Prism teams map to which GitHub teams (many-to-many)
CREATE TABLE org.team_github_team_mappings (
    team_id         UUID NOT NULL REFERENCES org.teams(id) ON DELETE CASCADE,
    github_team_id  UUID NOT NULL REFERENCES org.github_teams(id) ON DELETE CASCADE,
    PRIMARY KEY (team_id, github_team_id)
);
```

**Why separate `org.github_teams` instead of reusing `org.teams`?** GitHub teams and Prism teams are different concepts. A Prism team might map to multiple GitHub teams (e.g. a "Platform" Prism team could include GitHub teams `platform-backend` and `platform-infra`). Keeping them separate avoids polluting the user's team hierarchy with GitHub's org structure.

#### Triggering

- **On source creation/update**: When a GitHub source is created or its settings change, fire `GithubTeamSyncHandler/{source_name}/sync_teams/send` to Restate
- **On schedule**: Reuse the same `schedule_cron` field from `source_configs`, or default to daily. The team sync is lightweight (a few API calls) so running it frequently is fine
- **Manual**: Add a `TriggerTeamSync` RPC to the ingestion service

#### GitHub client additions

Add to `GitHubClient` in `crates/ps-ingestion/src/github/client.rs`:

```rust
pub async fn list_org_teams(&self, org: &str, page: u32) -> Result<PageResult<GitHubTeam>, SourceError>
pub async fn list_team_members(&self, org: &str, team_slug: &str, page: u32) -> Result<PageResult<GitHubUser>, SourceError>
pub async fn list_team_repos(&self, org: &str, team_slug: &str, page: u32) -> Result<PageResult<GitHubTeamRepo>, SourceError>
```

New types in `crates/ps-ingestion/src/github/types.rs`:

```rust
#[derive(Debug, Deserialize)]
pub struct GitHubTeam {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GitHubTeamRepo {
    pub name: String,
    pub owner: GitHubOwner,
    pub archived: bool,
}
```

### Phase 1b: Auto-Suggest GitHub Team → Prism Team Mappings

After team sync completes, we have `org.github_team_members` (GitHub usernames per GitHub team) and we already have `org.platform_identities` (GitHub username → person) + `org.team_memberships` (person → Prism team). We can join these to compute overlap and suggest mappings automatically.

#### How it works

For each GitHub team, compute which Prism teams its members belong to:

```sql
-- For each (github_team, prism_team) pair, count how many members overlap
SELECT
    gt.id AS github_team_id,
    gt.name AS github_team_name,
    t.id AS prism_team_id,
    t.name AS prism_team_name,
    COUNT(*) AS overlap_count,
    -- what fraction of the GitHub team's members are in this Prism team
    COUNT(*)::float / gt_total.total AS github_coverage,
    -- what fraction of the Prism team's members are in this GitHub team
    COUNT(*)::float / pt_total.total AS prism_coverage
FROM org.github_team_members gtm
JOIN org.platform_identities pi
    ON pi.platform = 'github' AND pi.platform_username = gtm.github_username
JOIN org.team_memberships tm
    ON tm.person_id = pi.person_id AND tm.end_date IS NULL
JOIN org.teams t ON t.id = tm.team_id
JOIN org.github_teams gt ON gt.id = gtm.github_team_id
CROSS JOIN LATERAL (
    SELECT COUNT(*) AS total FROM org.github_team_members WHERE github_team_id = gt.id
) gt_total
CROSS JOIN LATERAL (
    SELECT COUNT(*) AS total FROM org.team_memberships WHERE team_id = t.id AND end_date IS NULL
) pt_total
GROUP BY gt.id, gt.name, t.id, t.name, gt_total.total, pt_total.total;
```

This produces rows like:

| GitHub Team | Prism Team | Overlap | GH Coverage | Prism Coverage |
|-------------|------------|---------|-------------|----------------|
| platform-backend | Platform | 8/10 | 80% | 67% |
| platform-infra | Platform | 4/10 | 40% | 33% |
| platform-backend | Platform – API Squad | 5/10 | 50% | 100% |

#### Handling the hierarchy problem

Real orgs often have GitHub teams that map to multiple levels of the Prism hierarchy. For example, a GitHub team `platform-backend` might have 10 members, where 5 are in the "API Squad" Prism team and 3 are in the "Data Squad" — but all 8 also appear in the parent "Platform" Prism team (because squad members are also team members).

The suggestion algorithm should:

1. **Compute overlap for all (GitHub team, Prism team) pairs** as above
2. **Prefer leaf-level matches** — if a GitHub team has high coverage of a Squad-level Prism team, suggest that mapping rather than the parent Team-level one. The parent relationship is already captured by the Prism hierarchy
3. **Suggest when either coverage exceeds a threshold** (e.g. ≥60%) — a GitHub team where 80% of members are in one Prism team is a strong signal, even if those members are only 30% of the Prism team (the Prism team may span multiple GitHub teams)
4. **Surface multiple suggestions per GitHub team** — don't force 1:1. Show ranked suggestions with coverage percentages so the user can decide
5. **Never auto-apply** — these are suggestions shown in the UI, not automatic assignments. The user confirms or dismisses each one

#### UI presentation

In the team assignment UI (Phase 2), add a "Suggested Mappings" section:

- After team sync completes, show a banner: "We found N possible mappings based on team membership overlap"
- Each suggestion shows: GitHub team → Prism team, with overlap count and coverage percentages
- "Apply" button to accept a suggestion (creates the mapping)
- "Dismiss" button to hide a suggestion (stores dismissal so it doesn't resurface)
- Suggestions grouped by Prism team for easy review

#### Dismissed suggestions table

```sql
CREATE TABLE org.dismissed_github_team_suggestions (
    team_id         UUID NOT NULL REFERENCES org.teams(id) ON DELETE CASCADE,
    github_team_id  UUID NOT NULL REFERENCES org.github_teams(id) ON DELETE CASCADE,
    dismissed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (team_id, github_team_id)
);
```

This prevents the same suggestion from reappearing after the user explicitly dismisses it.

### Phase 2: Team Assignment UI

#### Proto additions

```protobuf
// In org.proto
message GitHubTeam {
    string id = 1;
    string github_org = 2;
    string slug = 3;
    string name = 4;
    optional string description = 5;
    int32 member_count = 6;
    int32 repo_count = 7;
}

message ListGitHubTeamsRequest {
    optional string search = 1;       // filter by name/slug
    optional string github_org = 2;   // filter by org
}

message ListGitHubTeamsResponse {
    repeated GitHubTeam teams = 1;
}

message AssignGitHubTeamRequest {
    string team_id = 1;               // Prism team UUID
    string github_team_id = 2;        // org.github_teams UUID
}

message UnassignGitHubTeamRequest {
    string team_id = 1;
    string github_team_id = 2;
}

message ListTeamGitHubTeamsRequest {
    string team_id = 1;
}

message ListTeamGitHubTeamsResponse {
    repeated GitHubTeam github_teams = 1;
}
```

Add RPCs to `OrgService`:
- `ListGitHubTeams` — searchable list of all discovered GitHub teams
- `AssignGitHubTeam` / `UnassignGitHubTeam` — manage the mapping junction table
- `ListTeamGitHubTeams` — list GitHub teams assigned to a Prism team

#### Frontend: Team edit panel

In the team detail/edit view (`views/teams/`), add a "GitHub Teams" section:

- Shows currently assigned GitHub teams as removable badges/chips
- "Add GitHub Team" button opens a searchable dropdown/combobox
- Dropdown fetches from `ListGitHubTeams` with debounced search
- Each result shows: `org/team-slug` — description — N members, N repos
- Selecting a result calls `AssignGitHubTeam`
- Removing a badge calls `UnassignGitHubTeam`

This replaces the current manual `github_team_slug` text field with a proper discovery-backed picker that supports multiple GitHub teams per Prism team.

### Phase 3: Team-Scoped Ingestion

Once GitHub teams are discovered and mapped to Prism teams, we can narrow the ingestion scope.

#### Repo filtering in `plan()`

Currently `plan()` discovers all repos for the org. Change it to:

1. Discover all repos (same as today) — still needed for the full picture
2. Check if **any** Prism teams have GitHub team mappings for this source
3. If yes: compute the **union** of repos across all mapped GitHub teams (from `org.github_team_repos`)
4. Filter discovered repos to only those in the union set
5. If no mappings exist: fall back to current behavior (all repos) — this preserves backward compatibility

```rust
// Pseudocode for plan()
let all_repos = discover_repos(&client, &orgs, &settings).await?;
let scoped_repos = repos.org.get_mapped_github_team_repos(source_id).await?;

let target_repos = if scoped_repos.is_empty() {
    all_repos  // no team mappings yet, fetch everything
} else {
    all_repos.into_iter()
        .filter(|r| scoped_repos.contains(&(r.owner.clone(), r.repo.clone())))
        .collect()
};
```

#### Benefits

- Ingestion runs faster (fewer repos to scan)
- Rate limit budget spent on relevant data only
- Watermarks still work per-source (unchanged)
- Adding/removing GitHub team mappings automatically changes what gets fetched on next run
- No data loss — existing contributions stay in the database; we just stop fetching new ones for out-of-scope repos

#### Identity resolution bonus

With `org.github_team_members` populated, we could also auto-create platform identities for team members, reducing manual identity mapping. This is optional and could be a follow-up.

## File Changes & Directory Structure

### Current `ps-ingestion` structure

```
crates/ps-ingestion/src/
├── main.rs                  # Binary entry point, Restate endpoint + registration
├── lib.rs                   # pub mod declarations
├── handler.rs               # IngestionHandler trait + impl (single Restate object)
├── registry.rs              # source_type → Box<dyn Source> factory
└── github/
    ├── mod.rs
    ├── client.rs            # GitHubClient (reqwest, auth, rate limits, pagination)
    ├── source.rs            # GitHubSource implements Source trait
    ├── repos.rs             # discover_repos() for org repo listing
    ├── types.rs             # GitHubPullRequest, GitHubReview, etc.
    └── etag.rs              # ETag key normalization
```

### Proposed `ps-ingestion` structure

The key structural decision: **handlers are organized by concern, not by source**. We'll end up with multiple handler types:

- **Source-specific handlers** — one ingestion handler per source type (GitHub now, Jira/Discourse/etc. later)
- **Source-specific auxiliary handlers** — team sync for GitHub, board sync for Jira, etc.
- **Cross-cutting handlers** — metrics recomputation, stale data cleanup, etc.

Each handler gets its own file inside `handlers/`. The shared state (`Repos`, `secret_key`, `http_client`) is extracted into a `SharedState` struct that all handlers reference. Source-specific logic stays in the source module (e.g. `github/`); handlers are thin Restate wrappers that delegate to it.

```
crates/ps-ingestion/src/
├── main.rs                  # Builds SharedState, binds ALL handlers to one Endpoint
├── lib.rs                   # pub mod declarations
├── handlers/
│   ├── mod.rs               # SharedState struct, re-exports all handlers
│   ├── github_ingestion.rs  # GithubIngestionHandler (renamed from handler.rs)
│   └── github_team_sync.rs  # GithubTeamSyncHandler (NEW)
│   # Future:
│   # ├── jira_ingestion.rs
│   # ├── metrics_refresh.rs
│   # └── stale_cleanup.rs
├── registry.rs              # source_type → Box<dyn Source> factory (unchanged)
└── github/
    ├── mod.rs
    ├── client.rs            # + list_org_teams(), list_team_members(), list_team_repos()
    ├── source.rs            # + repo scoping in plan() (Phase 3)
    ├── repos.rs             # (unchanged)
    ├── types.rs             # + GitHubTeam, GitHubTeamRepo types
    ├── teams.rs             # NEW — team sync logic (fetch teams/members/repos, store)
    └── etag.rs              # (unchanged)
```

#### `handlers/mod.rs` — Shared state

```rust
pub mod github_ingestion;
pub mod github_team_sync;

/// Shared state available to all Restate handlers.
/// Constructed once in main.rs, cloned into each handler.
pub struct SharedState {
    pub repos: Repos,
    pub secret_key: [u8; 32],
    pub http_client: reqwest::Client,
}
```

#### `main.rs` — N handlers, one Restate endpoint

```rust
// All handlers share the same state, bound to one Restate endpoint.
// Adding a new handler = one new .bind() call.
let state = SharedState { repos, secret_key, http_client };
let ingestion = GithubIngestionHandlerImpl { state: state.clone() };
let team_sync = GithubTeamSyncHandlerImpl { state };

HttpServer::new(
    Endpoint::builder()
        .bind(ingestion.serve())      // /GithubIngestionHandler/{key}/run_ingestion
        .bind(team_sync.serve())      // /GithubTeamSyncHandler/{key}/sync_teams
        // Future:
        // .bind(jira_ingestion.serve())
        // .bind(metrics_refresh.serve())
        .build()
)
.listen_and_serve(restate_addr)
.await;
```

All handlers register under the same deployment URL, so Restate discovers all of them from a single `/deployments` registration call. Adding a new handler is: write the file in `handlers/`, add a `.bind()` call in `main.rs`.

### Files changed across the codebase

#### `ps-ingestion` (ingestion binary)

| File | Action | What changes |
|------|--------|-------------|
| `handlers/mod.rs` | **NEW** | `SharedState` struct, re-exports both handlers |
| `handlers/ingestion.rs` | **RENAMED** from `handler.rs` | Trait renamed to `GithubIngestionHandler`, impl uses `SharedState` |
| `handlers/team_sync.rs` | **NEW** | `GithubTeamSyncHandler` trait + impl — calls GitHub team/member/repo APIs, stores to DB |
| `github/client.rs` | MODIFIED | Add `list_org_teams()`, `list_team_members()`, `list_team_repos()` methods |
| `github/types.rs` | MODIFIED | Add `GitHubTeam`, `GitHubTeamRepo` structs |
| `github/teams.rs` | **NEW** | Team sync orchestration logic (discover + store), used by `team_sync.rs` handler |
| `github/source.rs` | MODIFIED | Phase 3: `plan()` filters repos by team mappings |
| `github/mod.rs` | MODIFIED | Add `pub mod teams;` |
| `main.rs` | MODIFIED | Construct `SharedState`, bind both handlers, remove old `IngestionHandlerImpl` |
| `lib.rs` | MODIFIED | Replace `pub mod handler;` with `pub mod handlers;` |

#### `ps-core` (shared domain layer)

| File | Action | What changes |
|------|--------|-------------|
| `repo/org/github_teams.rs` | **NEW** | CRUD for `org.github_teams`, `github_team_members`, `github_team_repos`, `team_github_team_mappings`, `dismissed_github_team_suggestions`. Overlap query for suggestions. |
| `repo/org/mod.rs` | MODIFIED | Add `pub mod github_teams;` |
| `repo/org/teams.rs` | MODIFIED | Remove `github_team_slug` from team queries (after migration) |

#### `ps-server` (API server)

| File | Action | What changes |
|------|--------|-------------|
| `services/ingestion.rs` | MODIFIED | Rename URLs from `/IngestionHandler/` to `/GithubIngestionHandler/`, add `TriggerTeamSync` RPC handler |
| `services/org.rs` | MODIFIED | Add `ListGitHubTeams`, `AssignGitHubTeam`, `UnassignGitHubTeam`, `ListTeamGitHubTeams`, `GetTeamMappingSuggestions`, `DismissSuggestion` handlers |

#### Proto

| File | Action | What changes |
|------|--------|-------------|
| `proto/prism/v1/org.proto` | MODIFIED | Add `GitHubTeam` message, mapping RPCs, suggestion RPCs |
| `proto/prism/v1/ingestion.proto` | MODIFIED | Add `TriggerTeamSync` RPC |

#### Migrations

| File | Action | What changes |
|------|--------|-------------|
| `migrations/NNNN_github_teams.sql` | **NEW** | Create `org.github_teams`, `github_team_members`, `github_team_repos`, `team_github_team_mappings`, `dismissed_github_team_suggestions` |
| `migrations/NNNN_drop_github_team_slug.sql` | **NEW** | Migrate existing `github_team_slug` values → mappings, drop column |

#### Frontend

| File | Action | What changes |
|------|--------|-------------|
| `views/teams/components/github-team-picker.tsx` | **NEW** | Searchable multi-select combobox for assigning GitHub teams to a Prism team |
| `views/teams/components/team-mapping-suggestions.tsx` | **NEW** | Banner/card showing suggested mappings with apply/dismiss actions |
| `views/teams/components/team-detail-panel.tsx` | MODIFIED | Add "GitHub Teams" section with picker + suggestions |
| `views/teams/hooks/use-teams.ts` | MODIFIED | Add hooks: `useListGitHubTeams`, `useAssignGitHubTeam`, `useUnassignGitHubTeam`, `useTeamMappingSuggestions`, `useDismissSuggestion` |
| `views/admin/components/edit-team-dialog.tsx` | MODIFIED | Remove `github_team_slug` text field |
| `views/admin/components/source-row.tsx` | MODIFIED | Add "Sync Teams" button for GitHub sources |
| `views/admin/hooks/use-admin.ts` | MODIFIED | Add `useTriggerTeamSync` mutation hook |
| `lib/api/gen/` | REGENERATED | `buf generate` output |

## Implementation Order

### Step 0: Rename IngestionHandler → GithubIngestionHandler
- [x] Rename trait + impl in `crates/ps-ingestion/src/handler.rs`
- [x] Update Restate ingress URLs in `crates/ps-server/src/services/ingestion.rs`
- [x] Update registration in `crates/ps-ingestion/src/main.rs`
- [x] Verify existing invocation cancellation still works with new name

### Step 1: Migration + GitHub client methods
- [x] Write migration for `org.github_teams`, `org.github_team_members`, `org.github_team_repos`, `org.team_github_team_mappings`
- [x] Add `list_org_teams`, `list_team_members`, `list_team_repos` to `GitHubClient`
- [x] Add `GitHubTeam`, `GitHubTeamRepo` types
- [x] Add repo methods for the new tables in `OrgRepo`
- [x] `cargo sqlx prepare --workspace`

### Step 1b: Suggestion infrastructure
- [x] Add `org.dismissed_github_team_suggestions` to the migration
- [x] Add repo method to compute overlap between GitHub teams and Prism teams
- [x] Add repo method to store/query dismissed suggestions

### Step 2: Team sync workflow
- [x] Implement `GithubTeamSyncHandler` Restate virtual object
- [x] Register it alongside `IngestionHandler` in ingestion `main.rs`
- [x] Add `TriggerTeamSync` RPC to ingestion service
- [x] Trigger team sync on source create/update
- [x] Add proto definitions for team sync triggering

### Step 3: Team assignment UI
- [x] Add proto messages and RPCs for GitHub team listing/assignment
- [x] Implement `OrgService` handlers for the new RPCs
- [x] `buf generate`
- [x] Build frontend GitHub team picker component
- [x] Integrate into team detail view, replacing `github_team_slug` text field
- [x] Build "Suggested Mappings" UI with apply/dismiss actions
- [x] Add proto RPCs for fetching suggestions and dismissing them
- [x] Write migration to drop `github_team_slug` column from `org.teams` (migrate any existing values into mappings first)

### Step 4: Scoped ingestion
- [x] Modify `plan()` to compute repo scope from team mappings
- [x] Add `OrgRepo::get_mapped_github_team_repos(source_id)` query
- [x] Test with no mappings (full fetch) and with mappings (scoped fetch)
- [x] Consider logging/metrics for scope reduction (e.g. "fetching 12/187 repos based on team mappings")

### Step 5: Polish
- [x] Auto-trigger team sync when GitHub source is first created
- [ ] Schedule periodic team sync (daily by default)
- [ ] Consider auto-creating platform identities from GitHub team members
- [x] Update source setup UI to mention `read:org` scope requirement
- [ ] Documentation / README updates

## Scope Notes

- **Token scope**: Users will need to add `read:org` to their GitHub PAT. The source setup UI should mention this. Existing tokens without this scope will get 404s from the teams endpoints — the team sync should handle this gracefully and surface the error.
- **Backward compatibility**: If no GitHub team mappings exist for any Prism team, ingestion behaves exactly as it does today. This is a purely additive change.
- **GitHub Enterprise**: The custom `base_url` in `GitHubClient` already supports GHE. Team endpoints use the same base URL, so this works out of the box.
- **Nested GitHub teams**: GitHub supports nested teams. For now we'll flatten them — a nested team's members and repos are already included in the parent team's listings. We can add hierarchy awareness later if needed.

## Open Questions

1. ~~**Should team sync be a separate Restate service?**~~ **Decided: yes.** `GithubTeamSyncHandler` is a separate virtual object alongside `GithubIngestionHandler`. Handlers should be single-purpose.
2. ~~**Should we drop the `github_team_slug` column on `org.teams`?**~~ **Decided: yes.** Drop it. Migrate any existing values into the many-to-many mapping table, then remove the column. The mapping table is the source of truth.
3. ~~**Rate limit budget**~~ **Decided: independent.** Team sync and ingestion run at different times and have different cadences. Keeping rate limit tracking independent is simpler and avoids coupling.
