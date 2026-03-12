# Database Design

## Decision: Single PostgreSQL Instance

PostgreSQL with extensions covers all our needs:

- Relational data (org structure, contributions, metrics)
- JSONB for flexible/semi-structured fields (platform-specific metadata, state history)
- `pgvector` for embedding storage and similarity search
- Full-text search for content when needed

No need for a separate vector DB, document store, or cache layer at this scale.

## Schema Design

Organised by bounded context. Each context gets its own PostgreSQL schema for logical separation while keeping joins possible. Six schemas total: `auth`, `config`, `org`, `activity`, `metrics`, `reasoning`.

### `auth` schema — Authentication & Sessions

User credentials and session management. Intentionally minimal at launch (single admin user), structured for future expansion to multi-user, RBAC, and OIDC/SSO. See [07-authentication.md](./07-authentication.md) for full design rationale.

```sql
CREATE SCHEMA auth;

CREATE TABLE auth.users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,             -- Argon2id PHC string
    role TEXT NOT NULL DEFAULT 'admin',      -- 'admin' for now, future: 'viewer', 'manager', etc.
    is_active BOOLEAN NOT NULL DEFAULT true,
    person_id UUID REFERENCES org.people(id), -- optional link to org context
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE auth.sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,         -- SHA-256 of the session token (raw token never stored)
    session_type TEXT NOT NULL DEFAULT 'browser', -- 'browser' (login, 7-day expiry) or 'api_token' (no expiry, manually revoked)
    token_name TEXT,                         -- human-readable label, only set for api_token type
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ,                  -- NULL for api_tokens (no expiry)
    last_active_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    user_agent TEXT,                         -- browser info, for "active sessions" UI later
    ip_address INET
);

CREATE INDEX idx_sessions_user ON auth.sessions(user_id);
CREATE INDEX idx_sessions_expires ON auth.sessions(expires_at);
```

### `config` schema — Runtime Configuration

All configuration lives in the database so it can be modified from the UI at runtime. Source credentials (API tokens) are stored encrypted in `config.secrets` using AES-256-GCM, with the encryption key provided via the `PS_SECRET_KEY` environment variable. This is the only secret that must be supplied externally — all other credentials are managed through the admin UI.

```sql
CREATE SCHEMA config;

CREATE TABLE config.source_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_type TEXT NOT NULL,              -- 'github', 'jira', 'discourse', etc.
    name TEXT NOT NULL UNIQUE,              -- 'github-canonical', 'discourse-ubuntu', etc.
    enabled BOOLEAN NOT NULL DEFAULT true,
    settings JSONB NOT NULL DEFAULT '{}',   -- source-specific config (orgs, projects, base_url, etc.)
    schedule_cron TEXT,                     -- per-source schedule override, null = use global default
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE config.secrets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID REFERENCES config.source_configs(id) ON DELETE CASCADE, -- NULL for global secrets (e.g. AI provider API keys)
    secret_key TEXT NOT NULL,               -- e.g. 'github_token', 'jira_api_key', 'openrouter_api_key'
    encrypted_value BYTEA NOT NULL,         -- AES-256-GCM encrypted, includes nonce prefix
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_id, secret_key)
);

CREATE TABLE config.global_settings (
    key TEXT PRIMARY KEY,                   -- e.g. 'default_schedule', 'ai.tasks'
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### `org` schema — Organisation Context

```sql
CREATE SCHEMA org;

CREATE TABLE org.people (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    email TEXT,
    level TEXT,                    -- e.g. "Senior Engineer", "Staff Engineer"
    directory_id TEXT UNIQUE,      -- ID from the directory file import
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE org.platform_identities (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    person_id UUID NOT NULL REFERENCES org.people(id),
    platform TEXT NOT NULL,        -- 'github', 'jira', 'discourse', etc.
    platform_username TEXT NOT NULL,
    platform_user_id TEXT,         -- platform's internal ID if available
    UNIQUE (platform, platform_username)
);

-- Index for the hot path: "who is github user X?"
CREATE INDEX idx_identity_lookup ON org.platform_identities(platform, platform_username);

CREATE TABLE org.teams (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    org_name TEXT NOT NULL,        -- which of the two orgs
    parent_team_id UUID REFERENCES org.teams(id),  -- NULL = top-level team, set = squad
    lead_id UUID REFERENCES org.people(id),        -- director or manager
    github_team_slug TEXT,         -- from canonical-repo-automation config
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Efficient lookup of a team's squads
CREATE INDEX idx_teams_parent ON org.teams(parent_team_id) WHERE parent_team_id IS NOT NULL;

CREATE TABLE org.team_memberships (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    person_id UUID NOT NULL REFERENCES org.people(id),
    team_id UUID NOT NULL REFERENCES org.teams(id),
    start_date DATE NOT NULL,
    end_date DATE,                 -- NULL = current member
    UNIQUE (person_id, team_id, start_date)
);

CREATE TABLE org.repositories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    github_org TEXT NOT NULL,
    github_repo TEXT NOT NULL,
    default_branch TEXT DEFAULT 'main',
    primary_language TEXT,
    team_id UUID REFERENCES org.teams(id),  -- primary owning team
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (github_org, github_repo)
);

CREATE TABLE org.repo_scans (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,                     -- e.g. "uv adoption", "AI tooling"
    query TEXT NOT NULL,                    -- the question or scan rule description
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'running', -- 'running', 'completed', 'failed'
    summary JSONB                          -- aggregated results (counts, percentages)
);

CREATE TABLE org.repo_scan_results (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scan_id UUID NOT NULL REFERENCES org.repo_scans(id) ON DELETE CASCADE,
    repository_id UUID NOT NULL REFERENCES org.repositories(id),
    result JSONB NOT NULL,                 -- structured finding for this repo
    detail TEXT,                           -- human-readable explanation
    artifact_key TEXT,                     -- S3 key for raw output/logs in object storage, NULL if inline only
    UNIQUE (scan_id, repository_id)
);

CREATE INDEX idx_repo_scan_results_scan ON org.repo_scan_results(scan_id);
```

### `activity` schema — Activity Context

```sql
CREATE SCHEMA activity;

-- Single table for all contribution types, with a discriminator column
-- and JSONB for type-specific fields. This keeps queries simple while
-- remaining flexible for new source types.
CREATE TABLE activity.contributions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    person_id UUID REFERENCES org.people(id),   -- NULL if unresolved
    platform TEXT NOT NULL,                       -- 'github', 'jira', etc.
    contribution_type TEXT NOT NULL,              -- 'pull_request', 'code_review', etc.
    platform_id TEXT NOT NULL,                    -- unique ID on the source platform

    -- Common fields
    title TEXT,
    url TEXT,                                     -- link back to source
    state TEXT,                                   -- 'open', 'merged', 'closed', etc.
    created_at TIMESTAMPTZ NOT NULL,              -- when it happened on the platform
    updated_at TIMESTAMPTZ,
    closed_at TIMESTAMPTZ,

    -- Type-specific structured data
    metrics JSONB NOT NULL DEFAULT '{}',          -- e.g. {"lines_added": 42, "review_count": 3}
    metadata JSONB NOT NULL DEFAULT '{}',         -- platform-specific metadata

    -- Content for enrichment (nullable, only stored when needed)
    content TEXT,                                  -- review body, post body, etc.

    -- State tracking
    state_history JSONB DEFAULT '[]',             -- [{"state": "open", "at": "..."}, ...]

    -- Timestamps
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (platform, platform_id)
);

-- Primary query patterns
CREATE INDEX idx_contributions_person ON activity.contributions(person_id, created_at DESC);
CREATE INDEX idx_contributions_platform ON activity.contributions(platform, contribution_type, created_at DESC);
CREATE INDEX idx_contributions_state ON activity.contributions(state) WHERE state IN ('open', 'in_progress');
CREATE INDEX idx_contributions_created ON activity.contributions(created_at DESC);

-- GIN index for JSONB queries
CREATE INDEX idx_contributions_metrics ON activity.contributions USING GIN (metrics);

-- Ingestion tracking
CREATE TABLE activity.ingestion_watermarks (
    source_name TEXT PRIMARY KEY,
    watermark_value TEXT NOT NULL,
    last_successful_run TIMESTAMPTZ,
    last_attempt TIMESTAMPTZ,
    last_error TEXT,
    items_collected_last_run INTEGER DEFAULT 0
);

CREATE TABLE activity.ingestion_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_name TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'running',       -- 'running', 'completed', 'failed'
    items_collected INTEGER DEFAULT 0,
    error_message TEXT,
    rate_limit_waits_seconds INTEGER DEFAULT 0,   -- total seconds spent waiting
    metadata JSONB DEFAULT '{}'                   -- any additional run info
);

CREATE INDEX idx_ingestion_runs_source ON activity.ingestion_runs(source_name, started_at DESC);

-- ETag cache for conditional requests (reduces rate limit consumption)
CREATE TABLE activity.etag_cache (
    source_name TEXT NOT NULL,
    endpoint_url TEXT NOT NULL,       -- normalised URL (without per-run query params like 'since')
    etag TEXT NOT NULL,
    last_used TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (source_name, endpoint_url)
);
```

### `metrics` schema — Metrics Context

```sql
CREATE SCHEMA metrics;

-- Pre-computed team metrics for time periods
CREATE TABLE metrics.team_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL REFERENCES org.teams(id),
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    period_type TEXT NOT NULL,                    -- 'week', 'month', 'quarter'

    -- DORA metrics
    deployment_frequency REAL,
    lead_time_hours REAL,
    change_failure_rate REAL,
    mttr_hours REAL,

    -- Flow metrics
    throughput INTEGER,                           -- items completed
    avg_cycle_time_hours REAL,
    wip_avg REAL,
    flow_efficiency REAL,

    -- Review metrics
    avg_review_depth REAL,
    avg_review_turnaround_hours REAL,

    -- Raw data for custom calculations
    raw_metrics JSONB DEFAULT '{}',

    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (team_id, period_start, period_type)
);

-- Individual contribution profiles (for peer comparison, not ranking)
CREATE TABLE metrics.individual_profiles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    person_id UUID NOT NULL REFERENCES org.people(id),
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    period_type TEXT NOT NULL,

    -- Activity distribution across platforms
    activity_summary JSONB NOT NULL DEFAULT '{}',
    -- e.g. {"github": {"prs": 12, "reviews": 34}, "jira": {"tickets_closed": 5}}

    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (person_id, period_start, period_type)
);
```

### `reasoning` schema — AI/Reasoning Context

```sql
CREATE SCHEMA reasoning;

-- Vector embeddings for contributions
CREATE TABLE reasoning.embeddings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id UUID NOT NULL REFERENCES activity.contributions(id) ON DELETE CASCADE,
    embedding vector(1024),                       -- Gemini Embedding 2 with MRL truncation from native 3072; see Phase 3 plan
    model_name TEXT NOT NULL,                     -- which embedding model was used
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (contribution_id, model_name)
);

CREATE INDEX idx_embeddings_vector ON reasoning.embeddings
    USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);

-- AI-generated enrichments (traceable back to source content)
CREATE TABLE reasoning.enrichments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id UUID NOT NULL REFERENCES activity.contributions(id) ON DELETE CASCADE,
    enrichment_type TEXT NOT NULL,                -- 'sentiment', 'depth', 'topic', etc.
    value JSONB NOT NULL,                         -- structured result
    model_name TEXT NOT NULL,                     -- which LLM/model produced this
    confidence REAL,
    input_hash TEXT,                              -- hash of input text for reproducibility
    input_preview TEXT,                           -- first ~500 chars of input for quick audit
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (contribution_id, enrichment_type)
);

-- Generated insights (always auditable)
CREATE TABLE reasoning.insights (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scope_type TEXT NOT NULL,                     -- 'team' or 'individual'
    scope_id UUID NOT NULL,                       -- team_id or person_id
    period_start DATE,
    period_end DATE,
    insight_text TEXT NOT NULL,
    supporting_data JSONB,                        -- contribution IDs, metric snapshot IDs, scan result IDs
    reasoning_trace JSONB,                        -- ordered list of agent tool calls + results that led to this insight
    model_name TEXT NOT NULL,
    artifact_key TEXT,                            -- S3 key for rendered artifact (PDF, report), NULL if text-only
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Link table: which contributions fed into a metric snapshot
CREATE TABLE metrics.snapshot_sources (
    snapshot_id UUID NOT NULL REFERENCES metrics.team_snapshots(id) ON DELETE CASCADE,
    contribution_id UUID NOT NULL REFERENCES activity.contributions(id),
    PRIMARY KEY (snapshot_id, contribution_id)
);
```

## Key Design Decisions

### 1. Single contributions table with JSONB
Rather than a table per contribution type, we use one table with a discriminator (`contribution_type`) and JSONB for type-specific data. This keeps the schema manageable as we add sources, while still allowing efficient queries via GIN indexes.

### 2. PostgreSQL schemas for bounded contexts
Using `CREATE SCHEMA` gives us namespace isolation without the operational overhead of multiple databases. Cross-context queries (joining a contribution to its person) remain straightforward.

### 3. Upsert on `(platform, platform_id)`
The unique constraint on `(platform, platform_id)` gives us natural idempotent upserts. Re-ingesting the same PR just updates the row.

### 4. State history as JSONB array
For tracking PR state changes, Jira ticket transitions, etc. A JSONB array of `{state, timestamp}` objects is simple and queryable enough. If we need heavy analytics on state transitions, we can materialise a separate table later.

### 5. Metrics are pre-computed snapshots
Rather than computing DORA/flow metrics on every request, we periodically compute and store snapshots. The `computed_at` column lets us know how fresh they are. Real-time recalculation is possible but expensive; snapshots are good enough for the "compare teams" use case.

### 6. Object storage for artifacts (Phase 3+)
Binary artifacts (generated PDFs, repo scan logs, exported reports) go into S3-compatible object storage, not PostgreSQL. The database stores a reference (`artifact_key TEXT`) that points to the object. This keeps PostgreSQL focused on queryable data and avoids bloating the database with large blobs. See [01-architecture-overview.md — Object Storage Strategy](./01-architecture-overview.md#object-storage-strategy) for the full design. Tables with `artifact_key` columns: `reasoning.insights`, `org.repo_scan_results`.

## SQL Tooling: sqlx

**sqlx** is the SQL layer for this project. It provides:

- **Compile-time query checking** — queries are verified against the real database schema at build time
- **Migrations** — built-in migration runner, migrations live under `migrations/`
- **Async** — native async/await support with tokio
- **No ORM** — write real SQL, get compile-time safety without the abstraction penalty

### Always Use Type-Safe Query Macros

All queries **must** use the compile-time checked macros (`sqlx::query!`, `sqlx::query_as!`, `sqlx::query_scalar!`). Never use the runtime string-based `sqlx::query()` function.

```rust
// CORRECT — compile-time checked, returns typed struct
let team = sqlx::query_as!(
    Team,
    r#"SELECT id, name, org_name, parent_team_id, lead_id
       FROM org.teams WHERE id = $1"#,
    team_id
)
.fetch_one(&pool)
.await?;

// WRONG — no compile-time checking, avoid this
let team = sqlx::query("SELECT * FROM org.teams WHERE id = $1")
    .bind(team_id)
    .fetch_one(&pool)
    .await?;
```

This ensures schema changes are caught at build time, not at runtime.

### Offline Mode & Query Cache

For CI and development without a live database, sqlx supports **offline mode** via a cached query metadata file (`.sqlx/`):

- Run `cargo sqlx prepare` against a live database to generate the query cache
- The `.sqlx/` directory is checked into the repo
- CI builds use `SQLX_OFFLINE=true` to verify queries against the cache instead of a live DB
- The cache must be regenerated (`cargo sqlx prepare`) whenever queries or schema change

### Workflow

```sh
# Development: run migrations locally
cargo sqlx migrate run

# Before committing: update the query cache for offline builds
cargo sqlx prepare -- --workspace

# CI: build and check queries without a database
SQLX_OFFLINE=true cargo build
```

### Migration Structure

```
migrations/
├── 0001_create_config_schema.sql
├── 0002_create_org_schema.sql
├── 0003_create_activity_schema.sql
├── 0004_create_metrics_schema.sql
├── 0005_enable_pgvector.sql
├── 0006_create_reasoning_schema.sql
├── 0007_create_auth_schema.sql
└── ...
```

Each bounded context's schema is set up in its own initial migration, then evolved independently. `pgvector` is enabled in a dedicated migration (`0005`) **before** the reasoning schema (`0006`) because the reasoning tables use the `vector` type. The `auth` schema is created last (`0007`) because it references `org.people` via a foreign key.

### Deployment: Migrate Container

**The application does not run migrations itself.** Migrations are handled by a dedicated `ps-migrate` container that runs as a k8s init container before the API server starts.

The migrate container:
1. Runs all pending sqlx migrations
2. Seeds default config into `config.global_settings` if the table is empty (first deploy)
3. Does **not** seed `auth.users` — the first admin user is created via the first-run setup wizard in the UI (see [07-authentication.md](./07-authentication.md))
4. Exits — the API server init container dependency ensures it won't start until migration succeeds
5. If migration fails, the pod fails to start (visible, not silent)

```yaml
# k8s deployment (simplified)
initContainers:
  - name: migrate
    image: ps-migrate:latest    # Ubuntu + Chisel, contains only the migration binary + SQL files
    env:
      - name: DATABASE_URL
        valueFrom: ...
containers:
  - name: api
    image: ps-server:latest
```

The `ps-migrate` binary is a small Rust binary in the workspace that just runs `sqlx::migrate!()` and optional seed logic. It shares the same `migrations/` directory as the rest of the codebase.

## Performance Considerations

- At this scale (a few hundred people, ~6 sources, 3–6 hour cadence), PostgreSQL on a single machine will handle the load comfortably
- The main query patterns are: "give me all contributions for team X in period Y" — the indexes above cover this
- Embedding search (pgvector) with a few hundred thousand vectors is well within single-machine capability
- If we ever need more, PostgreSQL's partitioning (e.g. by `created_at` range) would be the first optimisation step
