# Data Ingestion Strategy

## Overview

Ingestion is the backbone of the system. It must reliably collect data from 6+ external platforms on a recurring schedule, handle rate limits gracefully, and support backfilling when runs are missed.

## Orchestration: Restate (confirmed)

**Restate** is the durable execution engine for ingestion. This was confirmed by the [Restate vs Temporal spike](~/code/canonical/temporal-restate-spike/evaluation.md) (scored 3.9 vs 3.1).

The ingestion service needs:

- Scheduled recurring runs (every 3–6 hours)
- Retry with backoff (especially for rate-limited APIs)
- Visibility into running/pending/failed jobs
- Ability to manually trigger a run or backfill

Restate provides all of these via:

- **Virtual objects** keyed by source name — per-source concurrency control
- **Durable side effects** (`ctx.run()`) — each step (plan, fetch, store, advance) is named and retriable
- **Durable sleep** (`ctx.sleep()`) — rate limit backoff survives restarts
- **Delayed self-invocation** (`send_after()`) — scheduling without external cron
- **Single binary** with embedded RocksDB — no extra database, 3 containers total (PostgreSQL + Restate + service)

Business logic is kept behind an `IngestionJob` trait, independent of Restate. Watermarks are stored in PostgreSQL (not Restate KV state) for queryability and auditability.

## Source Trait Design

### Relationship to the `IngestionJob` Trait

The [Restate vs Temporal spike](./09-spike-restate-vs-temporal.md) established the `IngestionJob` trait — an orchestrator-agnostic abstraction that decomposes ingestion into discrete retriable steps (`plan`, `fetch_batch`, `store_batch`, `advance_watermark`). The `Source` trait below is the **high-level interface** that each data source implements. The Restate handler calls `Source::collect()`, which internally follows the `IngestionJob` step pattern: each step is executed as a Restate durable side effect (`ctx.run()`) so it is checkpointed and retriable. In short: `Source` defines *what* to collect, the `IngestionJob` pattern defines *how* steps are executed durably.

### Interface

Every data source implements a common interface:

```
trait Source {
    /// Human-readable name for logging/UI
    fn name(&self) -> &str;

    /// Run an incremental collection from the last watermark
    /// Returns the new watermark on success
    async fn collect(&self, ctx: &IngestionContext) -> Result<Watermark>;

    /// Run a full backfill from a given point in time
    async fn backfill(&self, ctx: &IngestionContext, since: DateTime) -> Result<Watermark>;

    /// Report current rate limit status
    fn rate_limit_status(&self) -> RateLimitInfo;
}
```

The `IngestionContext` provides:
- Database connection for writing contributions
- The previous watermark (cursor) for this source
- Configuration (API keys, target repos/projects, etc.)
- A channel/callback for reporting progress to the UI

## Watermark Strategy

Each source tracks its own watermark — the point up to which data has been successfully collected.

| Source | Watermark Type | Notes |
|--------|---------------|-------|
| GitHub | `DateTime` (last event timestamp) | GitHub API supports `since` parameter on most endpoints |
| Jira | `DateTime` or JQL `updated >=` | Jira's search supports date-based filtering |
| Discourse | `Integer` (last post/topic ID) | Discourse API is ID-based |
| Launchpad | `DateTime` | Launchpad API supports date filtering |
| Google Drive | `String` (page token / change ID) | Drive API uses opaque change tokens |
| Mailing Lists | `DateTime` or `Message-ID` | Depends on archive format (pipermail, hyperkitty) |

Watermarks are persisted in the database in an `ingestion_watermarks` table:

```
ingestion_watermarks:
  source_name TEXT PRIMARY KEY
  watermark_value TEXT        -- serialized, source-specific
  last_successful_run TIMESTAMPTZ
  last_attempt TIMESTAMPTZ
  last_error TEXT             -- null if last run succeeded
  items_collected_last_run INTEGER
```

Note: ETag caching is stored separately in `activity.etag_cache` (see [Rate Limit Handling](#etag-caching)) — watermarks track *where* we are in the data, ETags track *whether* the data has changed since we last looked.

## Rate Limit Handling

Rate limits are a first-class concern, not an afterthought.

### Strategy
1. **Respect headers** — read `X-RateLimit-Remaining`, `Retry-After`, etc.
2. **Adaptive throttling** — slow down as we approach limits, don't wait until we hit them
3. **Transparent waiting** — when we must wait, report it clearly (how long, for which source)
4. **Per-source isolation** — one source hitting a rate limit doesn't block others
5. **Conditional requests (ETags)** — send `If-None-Match` / `If-Modified-Since` headers on every request. Responses returning `304 Not Modified` do **not** count against the rate limit. This is the single most impactful optimization: on a 6-hour cycle, most repos will have no new activity, so the majority of requests become free.

### ETag Caching

Sources that support conditional requests (GitHub, potentially others) should cache ETags to minimise rate limit consumption.

- **Per-endpoint ETag storage** — store the last ETag for each API endpoint fetched (e.g. per-repo PR listing). This lives in a dedicated `activity.etag_cache` table keyed by `(source_name, endpoint_url)`.
- **Request flow:** attach `If-None-Match: <etag>` on every request. On `304`, skip processing entirely. On `200`, process normally and update the stored ETag.
- **Pagination:** cache the ETag for the first page of paginated results. If page 1 returns 304, the entire collection is unchanged — skip all subsequent pages.
- **Cache lifetime:** ETags are cheap to store. Prune entries for endpoints not fetched in the last 30 days (e.g. when a repo is removed from config).

```sql
CREATE TABLE activity.etag_cache (
    source_name TEXT NOT NULL,
    endpoint_url TEXT NOT NULL,       -- normalised URL (without query params that change per-run like 'since')
    etag TEXT NOT NULL,
    last_used TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (source_name, endpoint_url)
);
```

### Change Radar (Events API)

For sources that provide an activity/events stream (notably GitHub's Events API), use it as a **change radar** to prioritise which repositories need full fetches vs. which can rely on cheap ETag-only checks.

**How it works:**
1. At the start of each ingestion cycle, fetch the org-level events stream (`GET /orgs/{org}/events`) filtered to relevant event types (`PullRequestEvent`, `PullRequestReviewEvent`, `IssuesEvent`).
2. Build a set of repos that had activity since the last run.
3. **Active repos** (appeared in events): fetch with full incremental logic (watermark + `since`).
4. **Inactive repos** (not in events): still fetch, but rely on ETag conditional requests. These almost always return 304 (free), confirming nothing was missed.

**Why this is safe:** inactive repos are never skipped — they still get an ETag-conditional check every cycle. If the Events API missed something (due to its 300-event cap or eventual consistency), the ETag check catches it. The radar only controls fetch *priority*, not whether we fetch at all.

**Constraints of the Events API:**
- Maximum 300 events returned, only the last 90 days
- Events may be delayed by a few minutes
- Not all event types are included (but PR and review events are)

**Fallback:** if the events fetch itself fails or returns an error, fall back to the non-radar path (full incremental fetch for all repos with ETags). The radar is an optimisation, not a correctness requirement.

This pattern is GitHub-specific in Phase 1 but the concept generalises: any source with a lightweight "what changed?" endpoint can implement a radar to reduce API calls.

### Visibility
The ingestion service exposes status that the frontend can query:
- Current state per source (idle, collecting, waiting for rate limit, error)
- If waiting: how long, and when the next attempt will be
- Queue depth: how many items remain to process
- Historical: last N runs with duration, items collected, errors
- ETag hit rate: percentage of requests that returned 304 (useful for monitoring optimisation effectiveness)
- Radar summary: how many repos were active vs. inactive per cycle

## What to Store vs What to Link

This is a spectrum, and the right answer varies by source and content type.

### Store inline (in the database)
- All structured metrics (lines changed, timestamps, states, counts)
- Short text that will be used for enrichment (review comments, post bodies up to a reasonable size)
- Metadata needed for metric computation without re-fetching

### Store as reference (link only)
- Large content (full PR diffs, entire documents)
- Binary content (Drive files, attachments)
- Anything that would bloat the database without clear analytical value

### Guideline
Ask: "Will we need this data to compute metrics or generate insights without calling the external API again?" If yes, store it. If it's only needed for occasional deep-dives, store a link.

## Handling Mutable Data

Some data changes between ingestion runs (PRs go from open to merged, Jira tickets change status).

### Approach
1. **Upsert on platform ID** — each contribution has a unique `(source, platform_id)`. On re-ingestion, update the existing row.
2. **Track state transitions** — for key entities (PRs, Jira tickets), maintain a `state_history` JSONB column or a separate `state_transitions` table with timestamps.
3. **Snapshot metrics at state change** — when a PR merges, compute and freeze `time_to_merge`, `review_rounds`, etc. These become immutable.
4. **Periodic reconciliation** — for sources where we might miss updates (webhooks aren't available), periodically re-scan recently active items.

### What counts as "recently active"?
- Open PRs: always re-check on each run
- Merged/closed PRs: stop re-checking after 7 days post-close
- Jira tickets in active states: re-check each run
- Jira tickets in terminal states (Done, Closed): stop after 7 days

## Backfill Strategy

When the ingestion service misses runs or is deployed fresh:

1. Check the watermark for each source
2. If no watermark exists, do a full historical backfill (configurable lookback, e.g. 6 months)
3. If watermark exists but is stale (> 1 scheduled interval), run normally — the incremental logic will catch up, possibly across multiple rate-limit-throttled batches
4. Report backfill progress clearly in the UI
5. Backfill runs at lower priority than regular incremental runs if both are needed

## Ingestion Pipeline Flow

```
Schedule triggers
  │
  ▼
For each enabled source (parallel where possible):
  │
  ├── Read watermark from DB
  ├── If source supports radar (e.g. GitHub Events API):
  │     ├── Fetch recent events for the org
  │     └── Partition repos into active (had events) vs inactive
  ├── Call source.collect(ctx)
  │     ├── Active repos: full incremental fetch (watermark + since)
  │     ├── Inactive repos: ETag-conditional fetch (expect 304, free)
  │     ├── All requests: send If-None-Match with cached ETag
  │     ├── On 304: skip processing, move to next endpoint
  │     ├── On 200: process normally, update cached ETag
  │     ├── Map to domain Contribution types
  │     ├── Resolve platform identity → Person
  │     ├── Upsert contributions to DB
  │     ├── Optionally: generate embeddings, run enrichment
  │     └── Return new watermark
  ├── Persist new watermark
  ├── Update ETag cache (prune stale entries)
  └── Record job run metadata (duration, count, errors, etag_hits, radar_active_repos)
```

## Configuration

Configuration lives in the **database**, not in files. This means:
- Sources can be added, enabled/disabled, and reconfigured from the UI at runtime
- No service restart or file reload needed for config changes
- The ingestion service reads its config from the DB at the start of each run
- Config changes are auditable (who changed what, when)

**Exception:** The encryption key (`PS_SECRET_KEY`) is the only secret stored as an environment variable. All other secrets (API tokens, credentials) are stored encrypted in the `config.secrets` table using AES-256-GCM, managed through the admin UI via `ConfigService.SetSecret`.

### Bootstrap
On first run (empty database), a CLI command or admin UI flow seeds initial configuration. A TOML/YAML file can be used for initial import only:

```sh
ps-server config import --file initial-config.toml
```

### Database Schema

```sql
CREATE TABLE config.source_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_type TEXT NOT NULL,              -- 'github', 'jira', 'discourse', etc.
    name TEXT NOT NULL UNIQUE,              -- 'github-canonical', 'discourse-ubuntu', etc.
    enabled BOOLEAN NOT NULL DEFAULT true,
    settings JSONB NOT NULL DEFAULT '{}',   -- source-specific: orgs, projects, base_url, etc.
    schedule_cron TEXT,                     -- override per-source schedule, null = use default
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE config.secrets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID REFERENCES config.source_configs(id) ON DELETE CASCADE, -- NULL for global secrets (e.g. AI provider keys)
    secret_key TEXT NOT NULL,               -- e.g. 'github_token', 'jira_api_key', 'openrouter_api_key'
    encrypted_value BYTEA NOT NULL,         -- AES-256-GCM encrypted, includes nonce prefix
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_id, secret_key)
);

CREATE TABLE config.global_settings (
    key TEXT PRIMARY KEY,
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- e.g. key='default_schedule', value='"0 */6 * * *"'
-- e.g. key='ai.tasks', value='{"enrichment": {"provider": "openrouter", "model": "..."}}'
```

Source credentials (API tokens) are stored encrypted in `config.secrets` using AES-256-GCM. The encryption key is provided via `PS_SECRET_KEY` env var — the only secret that must exist outside the database. All other credentials are managed through the admin UI via `ConfigService.SetSecret`.

### Example Source Config (as stored in DB)

```json
{
  "source_type": "github",
  "name": "github-canonical",
  "enabled": true,
  "settings": {
    "orgs": ["canonical"],
    "base_url": "https://api.github.com",
    "api_mode": "rest+graphql",
    "exclude_archived": true,
    "exclude_repos": []
  }
}
```

The GitHub token for this source is stored separately in `config.secrets` with `secret_key = 'github_token'`, encrypted at rest.
