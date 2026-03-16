# Phase 2: Breadth — Detailed Implementation Plan

Phase 2 adds two new data sources (Jira, Discourse), introduces flow and DORA metrics, and delivers the individual profile view. It builds on the foundation laid in Phase 1 — the ingestion infrastructure, GitHub source, org context, team views, and basic metrics are all in place.

**Exit criteria:** Multiple data sources feeding metrics. Individual and team views with flow metrics across GitHub, Jira, and Discourse.

**Code structure:** All new code follows feature-first organisation per [18-code-structure.md](./18-code-structure.md). New source adapters go in `crates/ps-workers/src/sources/<platform>/` (following the existing GitHub pattern in `ps-workers/src/github/`). New frontend features go in `frontend/views/<feature>/` with colocated components, hooks, and pages. New metrics UI goes in `views/metrics/`. Hooks with a single consumer stay feature-local; lift to `lib/hooks/` only when a second feature needs them.

## Assumptions (Phase 1 complete)

- Rust workspace, Vite + React Router app, PostgreSQL, proto definitions, and buf pipeline are operational
- `Source` trait (in `ps-core/src/ingestion.rs`), Restate orchestration, watermark tracking, and rate limit handling are proven with GitHub
- GitHub source uses GraphQL with cursor-based pagination and a two-phase approach (TeamRepos + MemberSearch) — see [29-targeted-ingestion-and-visibility.md](./29-targeted-ingestion-and-visibility.md)
- Org context (people, teams, directory import, platform identities) is populated and queryable
- `activity.contributions` table with upsert-on-`(platform, platform_id)` is working; `metrics` and `metadata` fields are JSONB (`serde_json::Value`)
- `ContributionType` enum has `PullRequest` and `PrReview` variants; `Platform` enum has `Github`, `Launchpad`, `Mattermost` — new variants for Jira/Discourse will be added as part of Phase 2 shared groundwork
- Team comparison view with contribution drill-down, ingestion status page, and basic PR metrics are live
- Identity resolution stores unresolved contributions with `person_id = NULL`
- `metrics.team_snapshots` table exists with flow metric columns pre-defined (cycle time, WIP, flow efficiency, lead time, review depth) and is populated for GitHub data
- Handler run tracking (`activity.ingestion_runs` with `handler_name`, `handler_method`, `progress` JSONB) is in place for multi-handler visibility (migrations 0010, 0011)
- `ConfigService.SetSecret` and `ConfigService.TestConnection` RPCs are implemented
- `MetricsService.GetTeamMetrics` and `MetricsService.ListTeamContributions` RPCs are implemented

---

## Workstreams

Phase 2 breaks into four workstreams. Two are source implementations that can proceed in parallel once the shared groundwork (new `Platform`/`ContributionType` enum variants, typed metrics structs, proto messages, new DB tables) is in place. The metrics and frontend workstreams depend on at least one new source landing before they can be fully integrated.

| # | Workstream | Dependencies | Estimated effort |
|---|-----------|-------------|-----------------|
| W0 | Shared groundwork | Phase 1 complete | Small |
| W1 | Jira source | W0 | Medium |
| W2 | Discourse source(s) | W0 | Medium |
| W3 | DORA & flow metrics | W1 (Jira data needed for cycle time) | Medium |
| W4 | Individual profile UI | Phase 1 frontend, W3 metrics | Medium |

### Change radar & ETag strategy across sources

Phase 1 established the [change radar](./03-data-ingestion-strategy.md#change-radar-events-api) and [ETag caching](./03-data-ingestion-strategy.md#etag-caching) patterns with GitHub. Phase 2 extends these to new sources where applicable:

| Source | Radar mechanism | ETag support | Notes |
|--------|----------------|-------------|-------|
| **GitHub** (Phase 1) | GraphQL cursor-based with watermark | Yes — `If-None-Match` on REST requests, 304s are free | Two-phase: TeamRepos + MemberSearch |
| **Jira Cloud** | JQL `updated >=` with `fields=key,updated,status` | Partial — useful on individual issue detail fetches | JQL radar is the primary optimisation; ETags are supplementary |
| **Discourse** | `GET /latest.json` with `bumped_at` comparison | No documented support | Radar is lightweight and effective on its own |

The `activity.etag_cache` table and the `Source` trait's support for conditional requests are reused across all sources that support them. Sources without ETag support simply skip the cache lookup — no code changes needed to the shared infrastructure.

### Suggested sequencing

```
Week 1:    W0 — shared groundwork (enums, typed metrics, migrations, proto messages, buf generate)
Week 2–3:  W1, W2 start in parallel
Week 4–5:  W1, W2 continue; W3 starts (GitHub flow metrics first)
Week 6–7:  W3 integrates Jira data; W4 starts
Week 8:    Integration, polish, identity resolution cleanup
```

---

## W0: Shared Groundwork

Everything W1–W4 depend on but that doesn't belong to any single source or feature.

### Deliverables

1. **New `Platform` enum variants** — `Jira`, `Discourse` (plus instance-qualified `discourse-*` naming for Discourse) in `ps-core/src/models/enums.rs`
2. **New `ContributionType` enum variants** — `JiraTicket`, `DiscoursePost`, `DiscourseTopic`
3. **Typed metrics layer** — `ContributionData` tagged enum with `PullRequest`, `PrReview`, `JiraTicket`, `DiscoursePost`, `DiscourseTopic` variants (formalises existing JSONB shapes for GitHub, adds new ones for Jira/Discourse). See [Typed Metrics Layer](#typed-metrics-layer-new-in-phase-2) section for full struct definitions.
4. **Migration `0012_create_snapshot_sources.sql`** — `metrics.snapshot_sources` link table for traceability
5. **Migration `0013_create_individual_profiles.sql`** — `metrics.individual_profiles` table
6. **Proto message definitions** — `Contribution`, `FlowMetricsResponse`, `IndividualProfileResponse`, `PlatformActivitySummary`, `PeerComparison`, `Percentile`, `ThroughputDataPoint`, `WipDataPoint` messages and the new RPC stubs (`GetIndividualProfile`, `ListPersonContributions`, `GetFlowMetrics`)
7. **`buf generate`** — regenerate Rust + TypeScript clients after proto changes

### Sequencing note

W0 is a prerequisite for W1 and W2, which can then proceed in parallel. Proto stubs and migrations can land before the service implementations that use them.

---

## W1: Jira Source

### Deliverables

1. **`crates/ps-workers/src/sources/jira/`** — `Source` trait implementation (following existing `github/` pattern)
2. **`Jira` variant** added to `Platform` enum; **`JiraTicket` variant** added to `ContributionType` enum; typed `JiraTicketMetrics` struct in `ps-core`
3. **Jira source config** entries in `config.source_configs`
4. **State transition tracking** for Jira ticket lifecycle
5. **Ingestion status** visible in existing UI
6. **Admin UI: Jira source form** — source-specific form component in the Data Sources admin tab for creating/editing Jira sources. Fields: base URL, project keys (multi-value input), story points custom field ID, Cloud vs Server toggle. Credentials (email, API token) via `SetSecret` through password-style inputs. Includes a "Test Connection" button that validates the Jira URL and credentials by calling `ConfigService.TestConnection`.
7. **`psctl contributions --platform jira`** — thin wrapper over `ListPersonContributions` filtered to Jira, following the existing psctl pattern
8. **Backup/restore: Jira data** — extend backup bundle to include Jira contributions (`activity.contributions WHERE platform = 'jira'`), Jira watermarks, and Jira source configs. Update `PreviewBackup` response to include Jira ticket counts.
9. **Jira user CSV import** — file upload to create `platform_identities` for Jira, mapping Jira `accountId` values to people via email address matching. See [Jira User Import](#jira-user-import) section below.

### Jira REST API approach

- **Authentication:** Jira Cloud uses OAuth 2.0 or API token + email (Basic auth). Jira Server/Data Center uses PAT. Store via `config.secrets` table (encrypted with AES-256-GCM) using `ConfigService.SetSecret`, as established in Phase 1.
- **Primary endpoint:** `/rest/api/3/search` with JQL
- **Incremental query:** `updated >= "{watermark}"` in JQL, ordered by `updated ASC`
- **Watermark type:** `DateTime` — the `updated` timestamp of the last successfully ingested issue
- **Rate limits:** Jira Cloud enforces per-user rate limits. Read `X-RateLimit-Remaining` and `Retry-After` headers. Adaptive throttling applies per the [ingestion strategy](./03-data-ingestion-strategy.md).
- **Change radar:** Jira has no dedicated events stream, but the JQL search endpoint itself doubles as a lightweight radar. At the start of each cycle, run `project IN (...) AND updated >= "{watermark}" ORDER BY updated ASC` with `fields=key,updated,status` and `maxResults=100`. This returns only issue keys and minimal metadata for recently-changed issues, which identifies which issues need full detail fetches (with `expand=changelog`). Issues not in the radar response are unchanged — skip them entirely. This is cheaper than the GitHub radar pattern because Jira's watermark-based JQL already acts as a natural filter; no separate "inactive item" ETag check is needed.

### JQL queries

The primary query pattern:

```
project IN ({configured_projects}) AND updated >= "{watermark}" ORDER BY updated ASC
```

Pagination via `startAt` + `maxResults` (default 50, max 100).

### Issue lifecycle mapping

| Jira Status Category | Mapped `state` | Notes |
|---------------------|---------------|-------|
| To Do | `open` | Backlog, To Do, New |
| In Progress | `in_progress` | In Progress, In Review, In Development |
| Done | `closed` | Done, Closed, Resolved |

Status transitions are recorded in `state_history` JSONB (same pattern as PR state changes). The Jira changelog API (`/rest/api/3/issue/{key}?expand=changelog`) provides the full transition history, which is critical for cycle time computation.

### Fields to extract

| Field | Maps to | Purpose |
|-------|---------|---------|
| `key` | `platform_id` | Unique identifier (e.g. `PROJ-123`) |
| `summary` | `title` | Display |
| `status` | `state` | Current state |
| `issuetype` | `metadata.issue_type` | Bug, Story, Task, Epic |
| `story_points` / `customfield_*` | `metrics.story_points` | Flow metrics |
| `created` | `created_at` | Timestamp |
| `updated` | `updated_at` | Watermark candidate |
| `resolutiondate` | `closed_at` | Cycle time end |
| `changelog` | `state_history` | Full transition log |
| `assignee` | Identity resolution | Person attribution |
| `priority` | `metadata.priority` | Context |
| `labels` | `metadata.labels` | Categorisation |

### Source config example

```json
{
  "source_type": "jira",
  "name": "jira-canonical",
  "settings": {
    "base_url": "https://canonical.atlassian.net",
    "projects": ["PROJ-A", "PROJ-B"],
    "story_points_field": "customfield_10016",
    "api_mode": "cloud"
  }
}
```

Credentials are stored separately in `config.secrets` (encrypted at rest):

| `secret_key` | Value |
|--------------|-------|
| `email` | Jira account email |
| `token` | Jira API token (Cloud) or PAT (Server) |

Set via `ConfigService.SetSecret(source_id, "email", ...)` and `ConfigService.SetSecret(source_id, "token", ...)` from the admin UI.

### Identity resolution

Jira identifies users by `accountId` (Cloud) or `username` (Server). The Jira Cloud REST API does not allow resolving email addresses to `accountId` values without elevated permissions. Instead, identity mapping relies on an admin-uploaded CSV export from Jira user management (see [Jira User Import](#jira-user-import) below).

The ingestion layer must:

1. Look up the Jira `accountId` against `org.platform_identities` where `platform = 'jira'` and `platform_user_id` matches
2. If no match, store the contribution with `person_id = NULL` and record the Jira display name + `accountId` in `metadata` for later manual mapping
3. Surface unresolved Jira identities in the admin UI alongside GitHub unknowns

### Jira user import

**Problem:** Jira Cloud returns `accountId` (an opaque string like `6254586fc23e5b006ab2c6d8`) on issue fields (assignee, reporter), but our API access level cannot resolve `accountId` → email. Without this mapping, Jira contributions cannot be linked to people in the org.

**Solution:** Jira Cloud's admin console allows exporting a CSV of all managed users (`Organization → Users → Export users`). This CSV contains the columns we need:

| CSV column | Use |
|------------|-----|
| `User id` | Jira `accountId` — stored as `platform_user_id` in `org.platform_identities` |
| `email` | Matched against `org.people.email` to find the person |
| `User name` | Display name — used in warnings for unmatched users |
| `User status` | Filter: only import `Active` users |

**Import flow** (follows the existing directory HTML import pattern):

1. **Frontend:** CSV file upload in the Jira source form (or a dedicated "Import Jira Users" action in the admin org tab). Reuses the same drag-and-drop upload pattern as the directory import dialog.
2. **Proto:** New RPC `OrgService.ImportJiraUsers(ImportJiraUsersRequest) returns (ImportJiraUsersResponse)` — request carries `bytes file_content` and `string source_name` (e.g. `"jira"`); response carries `identities_mapped`, `unmatched_users` count, and `warnings`.
3. **Parser (`ps-core/src/directory/`):** Parse CSV using the `csv` crate. Extract `User id`, `email`, `User name`, `User status`. Skip non-`Active` users. The CSV header names are fixed by Jira's export format.
4. **Identity mapping (`ps-core/src/repo/org/`):** For each row:
   - Look up `org.people` by `email` (case-insensitive match)
   - If a person is found: batch `UPSERT` into `org.platform_identities` with `platform = 'jira'`, `platform_username = email`, `platform_user_id = accountId`, `person_id = matched_person_id`
   - If no person match: record in warnings (`"No person found for jira user Mateo Florido <mateo.florido@canonical.com>"`)
5. **Ingestion lookup:** During Jira ingestion, resolve `accountId` from issue fields against `platform_identities WHERE platform = 'jira' AND platform_user_id = $1` — this is the reverse lookup that `batch_resolve_person_ids` already supports (extended to match on `platform_user_id` in addition to `platform_username`).

**Re-import behaviour:** Safe to re-upload. `ON CONFLICT (platform, platform_username) DO UPDATE` reassigns the `platform_user_id` and `person_id` if the mapping changes. New users in subsequent exports are added; existing mappings are updated.

**Why not automate via API?** Jira Cloud's `/rest/api/3/user/search` and `/rest/api/3/users` endpoints require `browse-users` or `manage-users` scopes, which are admin-level. The CSV export is available to any org admin and is a one-time (or infrequent) operation — the user list changes slowly.

### ETag support

Jira Cloud supports conditional requests (`If-None-Match` / `ETag`), but since the JQL radar approach already identifies exactly which issues changed, ETags provide less incremental value than they do for GitHub. Use ETags on individual issue detail fetches (`/rest/api/3/issue/{key}?expand=changelog`) to avoid re-downloading unchanged changelogs for issues that appear in the radar but whose changelog hasn't changed since the last full fetch.

### Considerations

- **`batch_resolve_person_ids` must support `platform_user_id` lookup** — Jira ingestion resolves `accountId` (stored in `platform_user_id`), not `platform_username`. Add a `batch_resolve_by_user_id` method or extend the existing one with a discriminator. GitHub continues to resolve by `platform_username`.
- **Story points field name varies** per Jira instance — make it configurable in `settings`
- **Jira Cloud pagination** uses `startAt`/`maxResults`; watch for the `total` field changing between pages (concurrent updates)
- **Sub-tasks:** Ingest as separate contributions with `metadata.parent_key` referencing the parent. Do not double-count story points from parent + children.
- **Mutable data handling:** Re-check issues in non-terminal states on each run (per [ingestion strategy](./03-data-ingestion-strategy.md)). Stop re-checking 7 days after resolution.

---

## W2: Discourse Source(s)

### Deliverables

1. **`crates/ps-workers/src/sources/discourse/`** — `Source` trait implementation (following existing `github/` pattern)
2. **`Discourse` variant** added to `Platform` enum; **`DiscoursePost` and `DiscourseTopic` variants** added to `ContributionType` enum; typed metrics structs in `ps-core`
3. **Multiple source configs** — one per Discourse instance
4. **Ingestion status** per instance in existing UI
5. **Admin UI: Discourse source form** — source-specific form component for creating/editing Discourse sources. Fields: base URL, category filter (multi-select), minimum post threshold. Credentials (API key, API username) via `SetSecret` through password-style inputs. "Test Connection" button validates the Discourse instance URL and credentials via `ConfigService.TestConnection`. The form supports adding multiple Discourse instances, each as a separate source config row.
6. **Backup/restore: Discourse data** — extend backup bundle to include Discourse contributions (`activity.contributions WHERE platform LIKE 'discourse-%'`), per-instance watermarks, and Discourse source configs. Update `PreviewBackup` response to include Discourse post/topic counts.

### Multi-instance design

Per the [resolved decision](./08-open-questions.md) (question 9), each Discourse instance is a separate entry in `config.source_configs`. This means:

- **Separate watermarks** — each instance tracks its own `last_topic_id` / `last_post_id`
- **Separate credentials** — each instance may have its own API key
- **Separate schedules** — a low-traffic instance can poll less frequently
- **Distinct `platform` values** — e.g. `discourse-ubuntu`, `discourse-snapcraft`. This is the discriminator in `activity.contributions.platform`

### Discourse API approach

- **Authentication:** API key + username header (`Api-Key`, `Api-Username`). Stored via `config.secrets` table (encrypted with AES-256-GCM) using `ConfigService.SetSecret`.
- **Primary endpoints:**
  - `/latest.json?page={n}` — topics sorted by latest activity
  - `/t/{id}.json` — full topic with posts
  - `/posts.json?before={id}` — posts in reverse chronological order
- **Incremental strategy:** Use the topic list endpoint filtered to topics updated since the last watermark. The Discourse API supports `?order=activity` and we can stop when we reach topics older than our watermark.
- **Watermark type:** `Integer` — the highest topic ID seen. Discourse IDs are monotonically increasing, making this reliable.
- **Change radar:** `GET /latest.json` is an excellent radar endpoint. It returns topic-level metadata (ID, title, `bumped_at`, `last_posted_at`, reply counts, category) without full post bodies. At the start of each cycle, fetch `/latest.json` and compare `bumped_at` timestamps against the last sync. Only do full topic fetches (`GET /t/{id}.json`) for topics that changed. Topics not in the `/latest.json` response with `bumped_at` after the watermark are unchanged — skip them. This naturally reduces API calls on low-traffic instances.

### What to ingest

**Topics** (contribution_type = `discourse_topic`):

| Field | Maps to | Purpose |
|-------|---------|---------|
| `id` | `platform_id` | Unique ID (scoped to instance) |
| `title` | `title` | Display |
| `slug` | `url` construction | Link back |
| `posts_count` | `metrics.post_count` | Activity level |
| `views` | `metrics.views` | Engagement |
| `category_id` + name | `metadata.category` | Classification |
| `accepted_answer` | `metadata.solved` | Solved topics |
| `created_at` | `created_at` | Timestamp |
| `bumped_at` | `updated_at` | Last activity |

**Posts** (contribution_type = `discourse_post`):

| Field | Maps to | Purpose |
|-------|---------|---------|
| `id` | `platform_id` | Unique post ID |
| `topic_id` | `metadata.topic_id` | Parent topic reference |
| `username` | Identity resolution | Person attribution |
| `reply_count` | `metrics.reply_count` | Engagement |
| `like_count` | `metrics.likes` | Community signal |
| `post_number` | `metadata.post_number` | Position in thread |
| `raw` or `cooked` | `content` | For enrichment (if enabled) |
| `created_at` | `created_at` | Timestamp |

### Source config example

```json
{
  "source_type": "discourse",
  "name": "discourse-ubuntu",
  "settings": {
    "base_url": "https://discourse.ubuntu.com",
    "categories": [],
    "min_posts": 2
  }
}
```

Credentials stored in `config.secrets` (encrypted at rest):

| `secret_key` | Value |
|--------------|-------|
| `api_key` | Discourse API key |
| `api_username` | Discourse API username |

Set via `ConfigService.SetSecret(source_id, "api_key", ...)` and `ConfigService.SetSecret(source_id, "api_username", ...)` from the admin UI.

A second instance would be a second row:

```json
{
  "source_type": "discourse",
  "name": "discourse-snapcraft",
  "settings": {
    "base_url": "https://forum.snapcraft.io",
    "categories": [],
    "min_posts": 2
  }
}
```

### Identity resolution

Discourse identifies users by `username`. The ingestion layer:

1. Looks up `org.platform_identities` where `platform = '{source_name}'` (e.g. `discourse-ubuntu`) and `platform_username` matches
2. If unresolved, stores with `person_id = NULL` and records `username` + `name` in `metadata`
3. A person active on multiple Discourse instances has one `platform_identity` row per instance

### ETag support

Discourse does not document ETag support on its JSON endpoints. The `/latest.json` radar approach makes ETags less important — the radar itself identifies changed topics, and full fetches are only done for those. If Discourse adds ETag support in the future, it can be layered in using the same `activity.etag_cache` infrastructure from Phase 1.

### Considerations

- **Rate limits:** Discourse has per-minute rate limits (default 60 req/min for API keys). Respect `Retry-After` headers.
- **Category filtering:** Some instances may have hundreds of categories. The `settings.categories` array (empty = all) lets admins limit scope.
- **`min_posts` filter:** Topics with very few posts (e.g. 1) may be noise. Configurable threshold.
- **Content storage:** Store `raw` markdown for posts that will be enriched (Phase 3). For others, store only structured metrics.

---

## W3: DORA & Flow Metrics

### Deliverables

1. **Flow metric computations** in `ps-metrics` — cycle time, WIP, throughput trends
2. **DORA lead time proxy** computation from PR and Jira data
3. **Populate `metrics.team_snapshots`** flow columns (cycle time, WIP, flow efficiency, lead time) with cross-source data — columns already exist from Phase 1 schema
4. **New `metrics.individual_profiles` table** with multi-source activity summaries (migration required — table does not yet exist)
5. **New `metrics.snapshot_sources` link table** mapping snapshots to contributing `contribution_id` values (migration required — table does not yet exist). Populated for all source types during metric computation
6. **Updated `/teams` comparison page** — add new columns for flow metrics (avg cycle time, WIP, throughput, lead time proxy) alongside existing Phase 1 PR metrics. Each metric cell links to the underlying contributions for traceability. Add source badges showing which platforms are feeding each team's data.
7. **Team detail page at `/teams/[teamId]`** — dedicated page for a single team showing: flow metric trends over time (Tremor charts), contribution breakdown by source, team member list with per-person summary stats, and links to individual profiles. This page is the bridge between the team comparison view and individual profiles — without it, users cannot drill down from high-level team metrics to understand what's driving them.
8. **`psctl metrics TEAM --period MONTH`** — thin wrapper over `GetFlowMetrics` / `GetTeamMetrics`, dumps flow + DORA metrics for a team and period
9. **Backup/restore: flow metric snapshots** — extend backup bundle to include new entries in `metrics.team_snapshots` and `metrics.individual_profiles` with cross-source flow data, and `metrics.snapshot_sources` link rows

### What becomes computable with multiple sources

| Metric | Sources needed | Computation |
|--------|---------------|-------------|
| **Cycle time** | Jira (primary), GitHub (supplementary) | Time from first "In Progress" to "Done" in Jira changelog |
| **WIP (Work in Progress)** | Jira, GitHub | Count of items in non-terminal states at any point in time |
| **Throughput** | All sources | Items completed per period, broken down by source |
| **Lead time (proxy)** | GitHub + Jira | Time from first commit (or Jira transition to In Progress) to PR merge |
| **Flow efficiency** | Jira | Active time / total cycle time (requires Jira status category mapping) |
| **Review turnaround** | GitHub (already in Phase 1) | Extended: cross-reference with Jira ticket to understand review in context of overall cycle |
| **Cross-platform activity** | All sources | Distribution of a person's or team's work across platforms |
| **Discourse engagement** | Discourse | Topics created, posts authored, reply ratios, solved topics |

### DORA metrics approach

Per the [resolved decision](./08-open-questions.md) (question 3), deployment-dependent metrics (deployment frequency, change failure rate, MTTR) are skipped. The Phase 2 DORA metric is **lead time**, computed as a proxy:

- **Lead time proxy:** Time from the first commit on a PR branch (or the Jira ticket moving to "In Progress") to the PR being merged. This is not true deployment lead time, but it captures the development + review cycle, which is the part the team controls.
- **Computation:** For each merged PR, look at `created_at` (or first commit timestamp if available) and `closed_at`. If linked to a Jira ticket (via branch naming convention or PR body), use the earlier of the two start timestamps.

### Flow metric computation

Cycle time and WIP require the Jira changelog data, which is why W3 depends on W1.

**Cycle time:**
1. For each completed Jira ticket, extract state transitions from `state_history`
2. Find the first transition to an "In Progress" status category
3. Find the transition to "Done" status category
4. Cycle time = Done timestamp - In Progress timestamp

**WIP:**
1. For a given point in time, count items that have entered "In Progress" but not yet reached "Done"
2. Compute daily WIP snapshots, average over the reporting period

**Throughput:**
1. Count items reaching terminal state per period
2. Slice by source, contribution type, and team

### Traceability

Every metric in `metrics.team_snapshots` and `metrics.individual_profiles` must be auditable back to the source contributions. The `metrics.snapshot_sources` link table (created as part of Phase 2 shared groundwork — see Migrations section) tracks which `contribution_id` values fed into each snapshot. This is populated during metric computation for all source types, not just GitHub.

---

## W4: Individual Profile UI

### Deliverables

1. **Individual profile page** at `/people/[personId]` (per [frontend strategy](./05-frontend-strategy.md))
2. **Cross-platform activity summary** — visual breakdown of contributions across GitHub, Jira, and Discourse
3. **Peer comparison context** — how this person's patterns compare to others at the same level
4. **Period selector** — reusable from team views
5. **Proto service methods** for individual profile queries
6. **`psctl people [--team TEAM] [--unresolved]`** — list people, optionally filtered to a team or showing only unresolved identities. The `--unresolved` flag is particularly useful during initial setup to surface Jira and Discourse identities that haven't been mapped yet.
7. **`psctl contributions --person PERSON [--platform PLATFORM] [--since DATE]`** — query contributions with person/source/date filters

### Page layout

The individual profile view shows:

- **Header:** Person name, team, level, linked platform identities
- **Activity distribution chart:** Stacked bar or donut showing contribution counts by platform over the selected period (Tremor chart)
- **Timeline:** Activity over time across all platforms (area chart or heatmap)
- **Contribution breakdown:** Tabbed or accordion sections per platform with key metrics
  - GitHub: PRs authored, reviews given, avg review turnaround
  - Jira: Tickets completed, avg cycle time, story points
  - Discourse: Posts, topics, solved topics
- **Peer context panel:** Anonymised comparison to others at the same level — "You're in the Xth percentile for review turnaround among Senior Engineers". Not a ranking, but a contextual reference.
- **Source links:** Every metric row links back to the contributions that produced it (traceability)

### Navigation

The full drill-down path must be navigable through the UI:

1. **`/teams`** (comparison page) → click a team row → **`/teams/[teamId]`** (team detail, W3 deliverable 7)
2. **`/teams/[teamId]`** → click a team member row → **`/people/[personId]`** (individual profile, this workstream)
3. **`/people/[personId]`** → breadcrumb or "Back to team" link → **`/teams/[teamId]`**
4. **`/people/[personId]`** → click any metric row → source contribution detail (traceability)

Each page includes breadcrumb navigation showing the full path (Teams → Team Name → Person Name). The team detail page (W3) is a prerequisite for this navigation to work — W4 must ensure the individual profile page integrates with it.

---

## New Database Schemas & Tables

Phase 2 does not introduce new schemas — it adds data to the existing `activity`, `metrics`, `config`, and `org` schemas. The key changes:

### Migrations

Phase 1 ends at migration `0011_add_run_progress.sql`. Phase 2 migrations continue the sequence:

| Migration | Description |
|-----------|-------------|
| `0012_create_snapshot_sources.sql` | **New table** `metrics.snapshot_sources` — link table mapping `snapshot_id` (FK to `team_snapshots`) and `contribution_id` (FK to `contributions`). Required for traceability. |
| `0013_create_individual_profiles.sql` | **New table** `metrics.individual_profiles` — per-person period snapshots with `activity_summary` JSONB and `peer_comparison` JSONB. |
| `00XX_add_jira_contribution_indexes.sql` | Optional partial indexes for Jira-specific queries (e.g. WIP by state) |
| `00XX_add_discourse_contribution_indexes.sql` | Optional partial indexes for Discourse-specific queries (e.g. per-instance metrics) |

The `activity.contributions` table itself needs no structural changes — new source types use the existing `platform`, `contribution_type`, `metrics` JSONB, and `metadata` JSONB columns. This is the payoff of the [single-table-with-typed-Rust-layer decision](./08-open-questions.md) (question 2). The `metrics.team_snapshots` table already has flow metric columns (cycle time, WIP, flow efficiency, lead time, review depth) from Phase 1 — Phase 2 populates them with cross-source data.

### New index considerations

```sql
-- Jira tickets by status (for WIP queries)
CREATE INDEX idx_contributions_jira_status
    ON activity.contributions(state, created_at)
    WHERE platform = 'jira';

-- Discourse posts by instance (for per-instance metrics)
CREATE INDEX idx_contributions_discourse_platform
    ON activity.contributions(platform, contribution_type, created_at)
    WHERE platform LIKE 'discourse-%';
```

These are partial indexes — they only cover the relevant subset of rows and are cheap to maintain.

### New config.source_configs rows

Phase 2 adds rows for each new source (Jira, each Discourse instance). No table changes; this is runtime configuration.

### New watermark rows

One `activity.ingestion_watermarks` row per new source name (e.g. `jira-canonical`, `discourse-ubuntu`, `discourse-snapcraft`).

---

## New Proto Definitions

### New service methods

`MetricsService.GetTeamMetrics` and `MetricsService.ListTeamContributions` already exist from Phase 1. The following new RPCs are needed:

```protobuf
// Individual profile queries — add to MetricsService or OrgService
rpc GetIndividualProfile(GetIndividualProfileRequest)
    returns (IndividualProfileResponse);

rpc ListPersonContributions(ListPersonContributionsRequest)
    returns (ListPersonContributionsResponse);

// Flow metrics — add to MetricsService
rpc GetFlowMetrics(GetFlowMetricsRequest)
    returns (FlowMetricsResponse);
```

Phase 2 also extends the existing `GetTeamMetrics` response to include flow metric fields (cycle time, WIP, flow efficiency, lead time) that are already defined in the `metrics.team_snapshots` schema but not yet populated or exposed via the proto response.

### New messages

```protobuf
message IndividualProfileResponse {
  string person_id = 1;
  string name = 2;
  string team_name = 3;
  string level = 4;
  repeated PlatformIdentity identities = 5;
  repeated PlatformActivitySummary activity_by_platform = 6;
  PeerComparison peer_context = 7;
}

message PlatformActivitySummary {
  string platform = 1;          // "github", "jira", "discourse-ubuntu", etc.
  int32 contribution_count = 2;
  map<string, double> metrics = 3;  // flexible key-value metrics per platform
}

message PeerComparison {
  string level = 1;
  int32 peer_count = 2;
  map<string, Percentile> metrics = 3;
}

message Percentile {
  double value = 1;
  double percentile = 2;  // 0.0–1.0
}

message FlowMetricsResponse {
  double avg_cycle_time_hours = 1;
  double wip_average = 2;
  int32 throughput = 3;
  double flow_efficiency = 4;
  double lead_time_hours = 5;
  repeated ThroughputDataPoint throughput_trend = 6;
  repeated WipDataPoint wip_trend = 7;
}

message ThroughputDataPoint {
  string date = 1;
  int32 count = 2;
  string source = 3;     // optional: break down by source
}

message WipDataPoint {
  string date = 1;
  double wip = 2;
}

message Contribution {
  string id = 1;
  string platform = 2;
  string contribution_type = 3;
  string title = 4;
  string url = 5;
  string state = 6;
  string created_at = 7;
  map<string, string> metrics = 8;
}
```

---

## Typed Metrics Layer (New in Phase 2)

Phase 1 stores `metrics` and `metadata` as raw `serde_json::Value` (JSONB) on the `Contribution` struct. Per the [single-table-with-typed-Rust-layer decision](./08-open-questions.md) (question 2), Phase 2 introduces typed Rust structs that serialize to/from this JSONB, providing compile-time safety while keeping the database schema flexible.

This is a **new enum** created in Phase 2 shared groundwork, not an expansion of an existing one:

```rust
/// Typed metrics layer over the JSONB `metrics` column.
/// Serializes to/from serde_json::Value for DB storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ContributionData {
    PullRequest(PullRequestMetrics),      // Phase 2 groundwork (formalises existing JSONB shape)
    PrReview(PrReviewMetrics),            // Phase 2 groundwork (formalises existing JSONB shape)
    JiraTicket(JiraTicketMetrics),        // Phase 2 — W1
    DiscoursePost(DiscoursePostMetrics),  // Phase 2 — W2
    DiscourseTopic(DiscourseTopicMetrics),// Phase 2 — W2
}
```

Each variant has a typed struct. The `PullRequest` and `PrReview` variants formalise the JSONB shapes already produced by the GitHub source. New variants:

```rust
struct JiraTicketMetrics {
    issue_type: String,
    story_points: Option<f64>,
    cycle_time_hours: Option<f64>,
    priority: Option<String>,
    labels: Vec<String>,
}

struct DiscoursePostMetrics {
    topic_id: i64,
    reply_count: i32,
    likes: i32,
    post_number: i32,
}
```

---

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Jira Cloud vs Server | Support both; `base_url` in config distinguishes them | Canonical uses Jira Cloud, but the code should not assume it |
| Discourse platform naming | `discourse-{instance}` (e.g. `discourse-ubuntu`) | Matches the source config `name`; distinguishes instances in queries and metrics |
| Story points field | Configurable per Jira instance | Field name varies; no safe default |
| Lead time proxy | Earliest of (PR created, Jira In Progress) to PR merge | Best approximation without deployment data |
| Individual profile: ranking vs context | Peer context (percentiles), not ranked leaderboards | This affects people's careers; percentile context is informative without being punitive |

---

## `psctl` & Backup/Restore Extensions

The psctl commands and backup/restore extensions are distributed across workstreams as deliverables rather than tracked separately:

| Deliverable | Workstream | Backing RPC |
|-------------|------------|-------------|
| `psctl contributions --platform jira` | W1 | `ListPersonContributions` |
| Backup/restore: Jira data | W1 | `PreviewBackup` / `RestoreBackup` |
| Backup/restore: Discourse data | W2 | `PreviewBackup` / `RestoreBackup` |
| `psctl metrics TEAM --period MONTH` | W3 | `GetFlowMetrics` / `GetTeamMetrics` |
| Backup/restore: flow metric snapshots | W3 | `PreviewBackup` / `RestoreBackup` |
| `psctl people [--team TEAM] [--unresolved]` | W4 | `GetIndividualProfile` / `ListPeople` |
| `psctl contributions --person PERSON` | W4 | `ListPersonContributions` |

All psctl commands are thin wrappers over the gRPC API, following the same pattern as Phase 1's `psctl status`, `psctl backup`, and `psctl trigger` commands. The `psctl` crate depends only on `ps-proto`.

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| **Jira API rate limits** are stricter than GitHub | Slow initial backfill | Adaptive throttling; JQL radar minimises calls to changed issues only; ETag conditional requests on detail fetches; configure reasonable project scope; backfill at lower priority |
| **Jira custom field IDs** vary between instances | Story points and other fields may not map correctly | Make field IDs configurable in `settings`; validate during first ingestion run; surface warnings in admin UI |
| **Discourse API pagination** is inconsistent across versions | Missing data or duplicate ingestion | Test against actual target instances; `/latest.json` radar reduces pagination exposure; defensive deduplication via `UNIQUE (platform, platform_id)` |
| **Identity resolution across 3+ platforms** becomes unwieldy | Growing pool of unresolved contributions | Surface unresolved identity counts prominently in admin UI; provide bulk-mapping tools |
| **Cycle time requires clean Jira workflows** | Teams with non-standard workflows produce garbage metrics | Document expected status category mappings; allow per-project overrides; show "insufficient data" rather than misleading numbers |
| **Individual profiles may be perceived as surveillance** | Team resistance to adoption | Frame as self-service and peer context, not management ranking; make profiles visible to the person and their manager only (access control is a Phase 2 concern to resolve) |

---

## Testing Strategy

### Per-workstream automated tests

**W0 — Shared groundwork:**
- Typed metrics layer: round-trip serialization tests for each `ContributionData` variant (serialize to `serde_json::Value`, deserialize back, assert equality)
- Existing GitHub JSONB data deserializes correctly into the new `PullRequest` and `PrReview` typed variants (backwards compatibility)
- New enum variants (`Platform::Jira`, `ContributionType::JiraTicket`, etc.) round-trip through `FromStr`/`Display` and sqlx `TEXT` encode/decode
- Migrations apply cleanly: `snapshot_sources` and `individual_profiles` tables exist with correct schemas
- Proto generation: `buf lint` passes, `buf breaking` passes against main

**W1 — Jira source:**
- `wiremock` integration tests against recorded Jira API responses (search, issue detail with changelog)
- Full pipeline test: JQL response → `JiraTicket` contribution → DB upsert with correct `state_history`
- Identity resolution: matched Jira `accountId` → `person_id`; unmatched → `person_id = NULL` with metadata preserved
- Watermark advancement: verify `ingestion_watermarks` updates correctly after each page
- Admin UI form: component test for Jira source creation form, `SetSecret` calls, and `TestConnection` flow
- Error handling: invalid project key, expired token, rate limit response
- `psctl contributions --platform jira`: verify CLI output matches expected format with test fixture data
- Backup/restore: round-trip test — backup with Jira contributions, restore to clean DB, verify Jira data intact
- Jira user CSV import: parse test CSV → match emails to existing people → verify `platform_identities` rows created with correct `platform_user_id` (accountId)
- Jira user CSV import: unmatched emails produce warnings, not errors
- Jira user CSV import: re-import updates existing identity mappings without duplicates

**W2 — Discourse source:**
- `wiremock` integration tests against recorded Discourse API responses (`/latest.json`, `/t/{id}.json`)
- Multi-instance test: two Discourse sources with distinct `platform` values produce separate contribution rows
- Identity resolution: `username` → `platform_identity` lookup per instance
- Watermark: verify topic ID watermark advances correctly
- Admin UI form: component test for Discourse source creation form, multi-instance add flow, `SetSecret`, `TestConnection`
- Category filtering: verify `settings.categories` limits ingestion scope
- Backup/restore: round-trip test — backup with Discourse contributions from two instances, restore, verify both instances' data intact

**W3 — DORA & flow metrics:**
- Unit tests in `ps-metrics` with known Jira contribution datasets: assert correct cycle time, WIP, throughput
- Lead time proxy: test with linked PR + Jira ticket, verify earliest start timestamp is used
- Flow efficiency: test with known active/wait state durations
- `snapshot_sources` population: verify every computed metric links back to contributing `contribution_id` values
- `/teams` page integration test: verify new flow metric columns render with correct values
- Team detail page (`/teams/[teamId]`): component tests for metric trend charts, member list, drill-down links
- `psctl metrics`: verify CLI output renders flow + DORA metrics correctly for a team/period
- Backup/restore: round-trip test — backup with `snapshot_sources` link rows and `individual_profiles`, restore, verify traceability links intact

**W4 — Individual profile UI:**
- Component tests for profile page layout, activity distribution chart, peer comparison panel
- Proto contract test: `GetIndividualProfile` response matches frontend expectations
- Navigation test: verify breadcrumb trail `/teams → /teams/[teamId] → /people/[personId]` works end-to-end
- Traceability: click a metric row → correct source contributions are displayed
- Cross-platform summary: verify contributions from GitHub, Jira, and Discourse all appear correctly
- `psctl people`: verify `--team` filter, `--unresolved` flag, and default output
- `psctl contributions --person`: verify person/platform/date filters produce correct output

### Per-workstream manual testing

**After W1 (Jira source):**
1. Open admin UI → Data Sources → click "Add Source" → select "Jira"
2. Fill in base URL, project keys, story points field; enter email and API token
3. Click "Test Connection" — verify success message
4. Save the source → verify it appears in the source list
5. Trigger ingestion (or wait for scheduled run) → check Ingestion Status page for Jira progress
6. Query `activity.contributions WHERE platform = 'jira'` to verify data landed
7. Check admin UI for any unresolved Jira identities
8. Upload Jira user CSV export → verify identities mapped count matches expected
9. Re-trigger ingestion → verify Jira contributions now resolve to people via `accountId` lookup
10. Re-upload the same CSV → verify no duplicates created, counts reflect updates

**After W2 (Discourse source):**
1. Open admin UI → Data Sources → click "Add Source" → select "Discourse"
2. Fill in base URL (e.g. `https://discourse.ubuntu.com`), optional category filter, min_posts
3. Enter API key and API username → click "Test Connection"
4. Save → add a second Discourse instance (e.g. Snapcraft) with different credentials
5. Trigger ingestion → check Ingestion Status page shows both Discourse instances with separate progress
6. Query `activity.contributions WHERE platform LIKE 'discourse-%'` to verify both instances produced data
7. Verify unresolved Discourse identities appear in admin UI

**After W3 (DORA & flow metrics):**
1. Open `/teams` comparison page → verify new columns: cycle time, WIP, throughput, lead time
2. Verify source badges show which platforms feed each team's data
3. Click a team row → navigate to `/teams/[teamId]` detail page
4. Verify flow metric trend charts render with real data
5. Verify team member list shows per-person summary stats
6. Click a metric value → verify it links to the underlying contributions (traceability)
7. Query `metrics.snapshot_sources` to verify all contribution types are linked

**After W4 (Individual profile):**
1. From `/teams/[teamId]`, click a team member → navigate to `/people/[personId]`
2. Verify header shows name, team, level, linked platform identities
3. Verify activity distribution chart shows contributions across GitHub, Jira, Discourse
4. Verify peer comparison panel shows percentile context
5. Click a metric row → verify it links to specific source contributions
6. Use breadcrumb to navigate back to team detail → back to team comparison
7. Test period selector changes update all charts and metrics

### Cross-cutting

- **End-to-end:** With all sources configured, verify the full navigation path: `/teams` → `/teams/[teamId]` → `/people/[personId]` → source contribution detail. Every displayed metric must be auditable back to source data.
- **Identity resolution:** Verify the admin UI surfaces unresolved identities across all three platforms (GitHub, Jira, Discourse) and that manually resolving an identity retroactively assigns `person_id` to existing contributions.
