# Plan 29 — Targeted Ingestion & Run Visibility

## Problem

The GitHub ingestion plan phase discovers **every repo** across all configured orgs via `GET /orgs/{org}/repos` before filtering down to team-mapped repos. For large organisations (hundreds/thousands of repos) this is wasteful — the plan phase alone can burn hundreds of API calls and minutes of wall time fetching repo metadata we immediately discard.

The fetch phase compounds this: for every PR page fetched via REST, we make a separate `GET /repos/{owner}/{repo}/pulls/{number}/reviews` call per PR — a page of 100 PRs costs 101 REST API calls. The entire ingestion uses only REST endpoints despite GitHub's GraphQL API being dramatically more efficient for our access patterns.

Separately, ingestion runs provide almost no feedback while running. Logs are at `debug` level and only visible via Restate console or `kubectl logs`. The admin UI shows `items_collected` as a bare counter but nothing about which repos are being processed, what was skipped, or what went wrong.

## Goals

1. **Eliminate the full org repo scan** — build the repo list from team sync data already in the DB, not from the GitHub API
2. **Capture individual contributions outside team repos** — use GraphQL search to find PRs/reviews by team members in non-team repos, so cross-repo contributions aren't lost
3. **Switch to GraphQL API** — replace the REST client with GraphQL for all ingestion queries, collapsing N+1 calls into single queries and avoiding the punishing 30 req/min Search API rate limit
4. **Make ingestion runs observable** — structured progress in the DB, surfaced in the frontend, plus better tracing for log-based debugging

## Current State

### Plan phase today ([source.rs:77-161](crates/ps-workers/src/github/source.rs#L77-L161))

1. Parse `settings.orgs`, call `repos::discover_repos()` — paginates `GET /orgs/{org}/repos` for every org
2. Upserts every discovered repo to `org.repositories`
3. Loads team-mapped repos via `org.get_mapped_github_team_repos(source_id)`
4. If team mappings exist, filters discovered repos to just the mapped set
5. Loads watermark, returns `IngestionPlan`

Steps 1-2 are the waste — we discover everything just to throw most of it away at step 4.

### Fetch phase today ([source.rs:163-267](crates/ps-workers/src/github/source.rs#L163-L267))

Iterates `plan.repos`, for each:
- `GET /repos/{owner}/{repo}/pulls?state=all&sort=updated&per_page=100` (paginated)
- `GET /repos/{owner}/{repo}/pulls/{number}/reviews` **per PR** (N calls per page)
- ETag caching on first page to skip repos with no changes (304)

A page of 100 PRs with reviews costs **101 REST calls**. This is the primary API budget consumer.

### REST client today ([client.rs](crates/ps-workers/src/github/client.rs))

Six methods, all REST:
- `list_pulls()`, `list_reviews()` — ingestion
- `list_org_repos()` — repo discovery
- `list_org_teams()`, `list_team_members()`, `list_team_repos()` — team sync

### Visibility today

- `activity.ingestion_runs` stores: `id`, `source_name`, `started_at`, `completed_at`, `status`, `items_collected`, `error_message`, `handler_name`, `handler_method`
- `update_run_progress(run_id, items_collected)` updates the counter mid-run
- Frontend polls `GetStatus` which joins watermarks + active/last run data
- Tracing is mostly `debug!` level — plan summary and batch counts at `info!`, per-repo detail at `debug!`

## Design

### Part 1: GraphQL Client

Replace the REST-based `GitHubClient` ingestion methods with a single GraphQL client. The REST client methods used by team sync (`list_org_teams`, `list_team_members`, `list_team_repos`) remain for now — team sync runs infrequently and the REST methods work fine. They can migrate to GraphQL later as a low-priority cleanup.

#### Why GraphQL over REST

| | REST (current) | GraphQL |
|---|---|---|
| PRs + reviews per page | 101 calls (1 list + 100 review fetches) | 1 query |
| Rate limit (core) | 5,000 req/hr | 5,000 points/hr (1 point per simple query) |
| Search rate limit | 30 req/min (separate, punishing) | Uses core 5,000/hr budget |
| ETag / 304 support | Yes | No — but irrelevant when queries are 100x cheaper |
| Pagination | Offset-based (items shift between pages) | Cursor-based (stable) |
| N+1 problem | Structural (separate endpoint per review set) | Solved (nested fields) |

Even without ETag caching, GraphQL is dramatically cheaper. A repo with 500 PRs costs ~500 REST calls today (5 pages × 100 review calls + 5 list calls) vs 5 GraphQL queries.

#### Core Queries

**PR + Reviews query (replaces `list_pulls` + `list_reviews`):**

```graphql
query ($owner: String!, $repo: String!, $cursor: String) {
  repository(owner: $owner, name: $repo) {
    pullRequests(
      first: 100
      after: $cursor
      orderBy: { field: UPDATED_AT, direction: ASC }
    ) {
      pageInfo { hasNextPage endCursor }
      nodes {
        number title state url isDraft
        createdAt updatedAt closedAt mergedAt
        additions deletions changedFiles
        author { login }
        labels(first: 10) { nodes { name } }
        headRefName baseRefName
        reviews(first: 100) {
          nodes {
            databaseId state body submittedAt
            author { login }
          }
        }
      }
    }
  }
}
```

One query per page of 100 PRs, reviews included inline. Cursor-based pagination via `pageInfo.endCursor`.

**Note on watermark filtering:** GraphQL's `pullRequests` connection does not support a `since`/`updatedSince` filter. To achieve incremental fetching, we sort by `UPDATED_AT ASC` and stop paginating once we see a PR with `updatedAt <= watermark` (all subsequent PRs are older). On first encounter of an already-seen timestamp, we skip items until we pass the watermark, then collect from there. This is slightly less efficient than REST's `since` parameter for repos with many old PRs, but the savings from eliminating N+1 review calls far outweigh this.

**Member search query (replaces REST Search API):**

```graphql
query ($query: String!, $cursor: String) {
  search(query: $query, type: ISSUE, first: 100, after: $cursor) {
    pageInfo { hasNextPage endCursor }
    issueCount
    nodes {
      ... on PullRequest {
        number title state url isDraft
        createdAt updatedAt closedAt mergedAt
        additions deletions changedFiles
        author { login }
        repository { nameWithOwner owner { login } name }
        labels(first: 10) { nodes { name } }
        headRefName baseRefName
        reviews(first: 100) {
          nodes {
            databaseId state body submittedAt
            author { login }
          }
        }
      }
    }
  }
}
```

Search query string: `author:{user} type:pr org:{org} updated:>{watermark}`

This uses the **core 5,000/hr rate limit**, not the REST Search API's 30/min. The search rate limit problem disappears entirely.

#### Client Structure

```rust
pub struct GitHubGraphQLClient {
    http: reqwest::Client,
    endpoint: String,  // https://api.github.com/graphql (or GHE equivalent)
    token: String,
}

impl GitHubGraphQLClient {
    /// Fetch a page of PRs with inline reviews for a repo.
    /// Returns parsed PRs and the cursor for the next page (if any).
    pub async fn fetch_pull_requests(
        &self,
        owner: &str,
        repo: &str,
        cursor: Option<&str>,
    ) -> Result<GraphQLPage<GitHubPr>, anyhow::Error>

    /// Search for PRs by team members across an org.
    /// Query string is pre-built by the caller.
    pub async fn search_pull_requests(
        &self,
        query: &str,
        cursor: Option<&str>,
    ) -> Result<GraphQLPage<GitHubSearchPr>, anyhow::Error>

    /// Execute a raw GraphQL query and parse the response.
    /// Handles error responses, rate limit headers, and retryable errors.
    async fn execute<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<GraphQLResponse<T>, anyhow::Error>
}

pub struct GraphQLPage<T> {
    pub items: Vec<T>,
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
    pub rate_limit: RateLimit,
}
```

#### Rate Limit Handling

GraphQL rate limits come from response headers (same as REST):
```
x-ratelimit-remaining: 4950
x-ratelimit-reset: 1700000000
```

Plus GraphQL-specific cost info in the response body:
```json
{
  "data": { ... },
  "extensions": {
    "rateLimit": {
      "cost": 1,
      "remaining": 4950,
      "resetAt": "2026-03-16T15:00:00Z"
    }
  }
}
```

Parse both. Log a warning when remaining drops below 200. Use Restate `ctx.sleep()` for durable backoff if we hit the limit.

### Part 2: Targeted Repo List (No Org Scan)

Replace `repos::discover_repos()` in the plan phase with a DB-driven approach:

```
Plan phase (new):
1. Load team-mapped repos from DB (already synced by team sync handler)
   → org.get_mapped_github_team_repos(source_id)
2. These become the "team repos" set for full PR ingestion
3. Load watermark — if none exists and this is not an explicit backfill,
   default to 7 days ago (UTC) rather than open-ended
4. No GitHub API calls in the plan phase at all
```

The team sync handler (`GithubTeamSyncHandler`) already discovers repos per team via `GET /orgs/{org}/teams/{slug}/repos` and stores them. The ingestion plan just reads that result.

**Fallback when no teams are mapped:** If no team mappings exist (fresh setup, or teams not yet configured), fall back to the current full org discovery. This preserves backwards compatibility and the "just add a source and go" experience before teams are configured.

**Default watermark cap:** When no watermark exists (first run or backfill), default to 7 days ago rather than fetching the entire history. This bounds the initial ingestion to a reasonable window and prevents hour-long first runs against repos with years of PR history. The 7-day default applies to both team repo ingestion and member search. Explicit backfills via `backfill(since_date)` override this cap.

**Repo metadata upsert:** Currently `discover_repos()` upserts repo metadata (default branch, language) to `org.repositories`. Move this responsibility to the team sync handler — when it fetches team repos, it should upsert the same metadata. The ingestion handler should not be writing repo metadata at all.

### Part 3: Individual Contributions via GraphQL Search

Team members contribute to repos outside their team's set (upstream dependencies, shared libraries, other teams' repos within the org). We want to capture these contributions without ingesting every repo in the org.

After ingesting team repos, run a second pass using the GraphQL search query to find PRs authored by known team members in repos we didn't already cover.

#### Flow

```
After team repo ingestion completes:

1. Collect all platform_usernames from team members
   → org.get_team_member_usernames(source_id)

2. For each username (or small batch), search via GraphQL:
   search(query: "author:{user} type:pr org:{org} updated:>{watermark}")

3. Filter out PRs in repos already ingested (team repos set)

4. Reviews are already included inline in the GraphQL search response —
   no additional API calls needed

5. Store as ContributionInput, same as team repo contributions

6. Upsert any newly-discovered repos to org.repositories (lazy discovery)
```

#### Rate Limit Strategy

With GraphQL, the search uses the **core 5,000 points/hr** budget — no separate 30/min limit. Mitigations are simpler:

- **Batch usernames**: GraphQL search supports `author:{u1} author:{u2}` OR syntax, batch up to ~5 users per query
- **Durable sleep**: Use Restate `ctx.sleep()` for backoff if we approach the rate limit
- **Cap per run**: Limit to N search queries per run (configurable, default 100) to avoid monopolising the rate limit budget. The next run picks up where we left off
- **Skip when budget is low**: If `rate_limit.remaining < 200` after the team repos phase, skip the member search phase entirely and log a warning. Team repo data is the priority

#### Cursor Extension

The `Cursor` struct gains a new phase field:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
enum IngestionPhase {
    TeamRepos,       // iterate team repos via GraphQL
    MemberSearch,    // search for cross-repo contributions via GraphQL
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cursor {
    phase: IngestionPhase,
    // TeamRepos phase fields (existing, adapted for GraphQL cursors):
    repo_index: usize,
    graphql_cursor: Option<String>,  // replaces page number
    watermark: Option<String>,
    repos: Vec<RepoTarget>,
    max_updated_at: Option<String>,
    // MemberSearch phase fields:
    search_user_index: usize,
    search_graphql_cursor: Option<String>,
    search_users: Vec<String>,
    ingested_repos: HashSet<(String, String)>,
}
```

When the `TeamRepos` phase exhausts all repos, `fetch_batch` transitions the cursor to `MemberSearch` and begins the search pass. This keeps the existing fetch-store loop unchanged.

### Part 4: Ingestion Run Visibility

#### 4a: Structured Progress in DB

Add a `progress` JSONB column to `activity.ingestion_runs`:

```sql
ALTER TABLE activity.ingestion_runs
ADD COLUMN progress jsonb;
```

Schema:

```json
{
  "phase": "team_repos | member_search | complete",
  "repos_total": 42,
  "repos_completed": 17,
  "repos_skipped": 5,
  "current_repo": "canonical/prism",
  "prs_fetched": 230,
  "reviews_fetched": 89,
  "identities_skipped": 3,
  "search_users_total": 15,
  "search_users_completed": 8,
  "rate_limit_remaining": 4200,
  "rate_limit_reset": "2026-03-16T14:30:00Z"
}
```

New repo method:

```rust
pub async fn update_run_progress_detail(
    &self,
    id: Uuid,
    items_collected: i32,
    progress: &serde_json::Value,
) -> Result<(), Error>
```

Update this after every batch in the fetch-store loop. The handler builds the progress JSON from cursor state + counters.

#### 4b: Enhanced Tracing

Upgrade key log points from `debug!` to `info!` and add structured fields:

| Location | Current | New |
|----------|---------|-----|
| Plan complete | `info!(repos = N, watermark)` | Add `team_repos`, `fallback_discovery` (bool), `default_watermark_applied` |
| Repo start | (none) | `info!(repo, repo_index, repos_total, "starting repo")` |
| Repo complete | (none) | `info!(repo, prs, reviews, "completed repo")` |
| Batch fetched | `debug!(items)` | `info!(repo, prs, reviews, graphql_cost, "fetched batch")` |
| Batch stored | `info!(batch_stored, total_items)` | Add `skipped_identities`, `repo` |
| Search phase start | (none) | `info!(users_count, "starting member search phase")` |
| Search query | (none) | `info!(users, results, cross_repo_prs, "searched for member PRs")` |
| Search phase skipped | (none) | `warn!(remaining, "skipping member search — rate limit budget low")` |
| Rate limit low | (none) | `warn!(remaining, reset, cost, "GitHub rate limit low")` when remaining < 200 |
| Watermark advance | `info!(watermark, items)` | Add `old_watermark` |

#### 4c: Frontend Progress Panel

Extend the source status display on the admin ingestion page to show structured progress when a run is active:

- Current phase (Team Repos / Member Search)
- Repo progress bar: "17/42 repos (5 skipped — unchanged)"
- Current repo name
- Counters: PRs fetched, reviews fetched, contributions stored, identities skipped
- Rate limit gauge (remaining / total)
- Search phase: "8/15 users searched"

This reads from the `progress` JSONB column via the existing `GetStatus` RPC. Extend the `SourceStatus` proto message with a `progress` field (JSON string or structured sub-message).

## Implementation Order

### Phase 1: GraphQL Client
- Add `GitHubGraphQLClient` alongside existing REST client
- Implement `fetch_pull_requests()` — PRs + inline reviews in one query
- Implement `search_pull_requests()` — member search via GraphQL search
- Add GraphQL response types (`GitHubPr`, `GitHubSearchPr`, `GraphQLPage`, etc.)
- Implement rate limit parsing (headers + response body `extensions.rateLimit`)
- Integration tests with wiremock (mock the `/graphql` endpoint)

### Phase 2: Targeted Repo List + GraphQL Ingestion
- Move repo metadata upsert into team sync handler
- Replace `discover_repos()` call in `plan_impl()` with DB lookup
- Keep fallback to full discovery when no teams mapped
- Default watermark to 7 days ago when no watermark exists (non-backfill runs)
- Switch `fetch_batch_impl` from REST `list_pulls`/`list_reviews` to GraphQL `fetch_pull_requests`
- Adapt cursor from offset-based pagination to GraphQL cursor-based pagination
- Implement watermark-based early termination (stop when `updatedAt <= watermark`)
- Remove unused REST methods (`list_pulls`, `list_reviews`, `list_org_repos`) and `repos.rs`

### Phase 3: Enhanced Tracing (quick win, can be done in parallel with Phase 1-2)
- Upgrade log levels and add structured fields per the table above
- No schema changes, no frontend changes
- Immediate benefit for debugging current runs

### Phase 4: Structured Progress
- Migration: add `progress` JSONB column to `activity.ingestion_runs`
- New repo method `update_run_progress_detail()`
- Update handler to build and persist progress JSON each batch
- Extend `SourceStatus` proto with progress field
- Frontend progress panel component

### Phase 5: Member Search
- Extend `Cursor` with `IngestionPhase` and search-related fields
- New repo method `org.get_team_member_usernames(source_id)`
- Implement `MemberSearch` phase in `fetch_batch_impl` using GraphQL `search_pull_requests`
- Filter out already-ingested repos
- Lazy repo upsert for newly-discovered repos
- Rate limit budget check — skip search phase if remaining < 200
- Integration tests with wiremock

### Future: Migrate Team Sync to GraphQL
- Low priority — team sync runs infrequently and REST works fine
- Replace `list_org_teams`, `list_team_members`, `list_team_repos` with equivalent GraphQL queries
- Remove remaining REST client code

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| GraphQL `pullRequests` has no `since` filter | May re-fetch old PRs on first pages before reaching watermark | Sort by `UPDATED_AT ASC` and stop paginating once past watermark; savings from eliminating N+1 review calls far outweigh this |
| GraphQL query cost > 1 point for nested fields | Burns rate limit faster than expected | Parse `extensions.rateLimit.cost` from every response; adjust batch size if needed |
| Removing org discovery breaks sources with no teams configured | New users can't ingest anything until teams are set up | Fallback to full discovery when no team mappings exist |
| GraphQL response schema changes | Type deserialization breaks silently | Strong Rust types for response parsing; integration tests with realistic fixtures |
| JSONB progress column adds write load per batch | Extra UPDATE per batch (~every 100 PRs) | Negligible — one small UPDATE vs the API calls per batch |
| Reviews > 100 per PR truncated | Lose some reviews on highly-reviewed PRs | Add `pageInfo` to reviews field; if `hasNextPage`, fetch remaining via a follow-up query (rare edge case — most PRs have < 10 reviews) |
