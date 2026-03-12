# Spike: Restate vs Temporal for Ingestion Orchestration

## Goal

Validate the decision to use Restate over Temporal by building a minimal but representative ingestion workflow with both orchestrators. The spike uses a single GitHub source (fetching pull requests for one repository) as the test case. By the end, we should have enough evidence to commit confidently to one orchestrator.

## Success Criteria

1. Both implementations produce identical ingestion results for the same GitHub repository.
2. Each evaluation dimension (see below) has a written verdict with evidence.
3. The ingestion business logic is shared between both implementations, proving the abstraction layer works.
4. A clear recommendation is documented with trade-offs.

## Scope

### In Scope

- A shared ingestion trait and GitHub PR collection implementation (pure business logic, no orchestrator dependency).
- A Restate-based harness that drives the shared logic.
- A Temporal-based harness that drives the shared logic.
- PostgreSQL storage of fetched PR data (minimal schema, just enough to prove the pipeline).
- Rate limit handling and backoff (GitHub's `X-RateLimit-*` headers).
- A simulated backfill scenario (fetch PRs updated in the last 30 days).
- Single-node Canonical K8s deployment of each orchestrator.
- Written evaluation against all dimensions listed below.

### Out of Scope

- Multiple data sources (Jira, Discourse, etc.) — GitHub only.
- Embeddings, enrichment, or AI reasoning.
- Frontend or gRPC API.
- Production-grade schema or domain model.
- Performance benchmarking at scale.

## Directory Structure

```
spikes/restate-vs-temporal/
├── Cargo.toml                    # Workspace root for the spike
├── README.md                     # How to run the spike (written during implementation)
├── crates/
│   ├── spike-core/               # Shared types, traits, and GitHub ingestion logic
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── github.rs         # GitHub API client + PR fetching
│   │       ├── ingest.rs         # IngestionJob trait + types
│   │       ├── store.rs          # Database storage trait + PostgreSQL impl
│   │       └── rate_limit.rs     # Rate limit tracker
│   ├── spike-restate/            # Restate service implementation
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs           # Restate service wrapping spike-core
│   └── spike-temporal/           # Temporal worker + workflow implementation
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs           # Temporal worker
│           ├── workflow.rs       # Workflow definition
│           └── activities.rs     # Activity definitions wrapping spike-core
├── k8s/
│   ├── restate/                  # Manifests for deploying Restate server
│   ├── temporal/                 # Manifests for deploying Temporal server
│   ├── postgres.yaml             # Shared PostgreSQL deployment
│   └── namespace.yaml
├── docker/
│   ├── Dockerfile.restate        # Container image for the Restate service
│   └── Dockerfile.temporal       # Container image for the Temporal worker
└── evaluation.md                 # Filled in during/after the spike
```

## Shared Abstraction Layer

The key architectural constraint is that ingestion logic must be orchestrator-agnostic. The `spike-core` crate owns all business logic. The orchestrator-specific crates are thin wrappers.

### Core Trait

```rust
/// Represents a unit of ingestion work that an orchestrator can execute.
/// Each method is a logical step that the orchestrator calls as a
/// separately retriable operation.
#[async_trait]
pub trait IngestionJob {
    /// Determine what needs to be fetched based on the current watermark.
    async fn plan(&self, config: &SourceConfig) -> Result<IngestionPlan>;

    /// Fetch a batch of data from the external API.
    /// Returns the items fetched and rate limit status.
    async fn fetch_batch(&self, cursor: &Cursor) -> Result<FetchResult>;

    /// Persist a batch of fetched items to the database.
    async fn store_batch(&self, items: &[PullRequest]) -> Result<()>;

    /// Update the watermark after successful storage.
    async fn advance_watermark(&self, new_watermark: Watermark) -> Result<()>;
}
```

Each method maps to a discrete retriable step in both orchestrators. The orchestrator decides the retry policy, timeout, and error handling — the business logic just does the work.

### Why This Shape

- `plan` is separated so the orchestrator can inspect what work is pending and make scheduling decisions.
- `fetch_batch` is a single batch (not "fetch everything") so the orchestrator can checkpoint between batches and handle rate limits between calls.
- `store_batch` and `advance_watermark` are separate so we never advance the watermark without having persisted the data (even if the process crashes between them, the worst case is re-fetching already-stored data, which is safe because we upsert).

## Implementation Plan

### Phase 1: Shared Core (Days 1-2)

1. **Set up the spike workspace.** Create `Cargo.toml` workspace with the three crates.

2. **Define core types in `spike-core`:**
   - `SourceConfig` — repo owner/name, API token reference, schedule.
   - `Watermark` — timestamp-based cursor for GitHub's `since` parameter.
   - `Cursor` — current position within a paginated fetch (page number + watermark).
   - `FetchResult` — batch of PRs + next cursor + rate limit info.
   - `PullRequest` — minimal struct: number, title, state, author, created_at, updated_at, merged_at, additions, deletions, review count.
   - `RateLimitInfo` — remaining calls, reset time, whether we should back off.
   - `IngestionPlan` — describes the work: watermark to start from, estimated page count.

3. **Implement GitHub client (`github.rs`):**
   - Use `reqwest` with `octocrab` or raw REST calls to GitHub's API.
   - List PRs for a single repo with `state=all&sort=updated&direction=asc&since={watermark}`.
   - Handle pagination via `Link` header.
   - Parse and respect `X-RateLimit-Remaining` and `X-RateLimit-Reset`.
   - Return `FetchResult` per page.

4. **Implement PostgreSQL storage (`store.rs`):**
   - Use `sqlx` with a minimal schema:
     ```sql
     CREATE TABLE pull_requests (
         id SERIAL PRIMARY KEY,
         repo TEXT NOT NULL,
         pr_number INTEGER NOT NULL,
         title TEXT NOT NULL,
         state TEXT NOT NULL,
         author TEXT NOT NULL,
         created_at TIMESTAMPTZ NOT NULL,
         updated_at TIMESTAMPTZ NOT NULL,
         merged_at TIMESTAMPTZ,
         additions INTEGER,
         deletions INTEGER,
         review_count INTEGER,
         fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
         UNIQUE(repo, pr_number)
     );

     CREATE TABLE ingestion_watermarks (
         source_name TEXT PRIMARY KEY,
         watermark_value TEXT NOT NULL,
         last_successful_run TIMESTAMPTZ,
         last_attempt TIMESTAMPTZ,
         last_error TEXT
     );
     ```
   - Upsert PRs on `(repo, pr_number)`.
   - Read/write watermarks.

5. **Implement `IngestionJob` for GitHub** — ties the client and storage together.

6. **Write a plain `main.rs` in `spike-core` (or a test) that runs the full pipeline without any orchestrator**, to validate the business logic works standalone. This is the baseline.

### Phase 2: Restate Implementation (Days 3-4)

1. **Deploy Restate server on Canonical K8s.**
   - Restate ships as a single container image.
   - Write k8s manifests: Deployment + Service for the Restate server.
   - Expose the ingestion and admin ports.

2. **Implement `spike-restate` service:**
   - Use the `restate-sdk` crate.
   - Define a Restate service (virtual object keyed by source name) with handlers that call through to `IngestionJob` methods.
   - Map the ingestion flow:
     - **Handler: `run_ingestion`** — called on schedule or manually.
       - Calls `plan()` to determine work.
       - Loops: calls `fetch_batch()`, then `store_batch()`, then `advance_watermark()`.
       - Each call is a Restate action (automatically retriable, journaled).
       - Between batches, check rate limit info. If throttled, use Restate's `ctx.sleep()` to durably wait.
     - **Handler: `backfill`** — same flow but with an overridden start watermark.
     - **Handler: `get_status`** — reads Restate's key-value state to report current progress.
   - Use Restate's built-in key-value state to store per-source metadata (last run time, current status).
   - Use Restate's awakeable/sleep for durable rate limit waits.

3. **Set up scheduling:**
   - Option A: Use a Kubernetes CronJob that calls Restate's HTTP invocation API.
   - Option B: Use Restate's cron-like delayed calls (call self with a delay at the end of each run).
   - Evaluate which feels more natural and observable.

4. **Build container image and deploy to k8s.**
   - Multi-stage Dockerfile: build with `rust:latest`, run with a minimal base.
   - Register the service with the Restate server via its admin API.

5. **Test the full flow:**
   - Trigger a manual ingestion run.
   - Verify PRs appear in PostgreSQL.
   - Simulate a rate limit hit (use a repo with many PRs, or artificially lower the threshold).
   - Kill the service mid-run, restart, verify it resumes from the last checkpoint.
   - Trigger a backfill.

### Phase 3: Temporal Implementation (Days 5-6)

1. **Deploy Temporal server on Canonical K8s.**
   - Temporal requires: Temporal server, a database (can share the existing PostgreSQL or use a separate one — note Temporal needs its own schemas), and optionally the Temporal UI.
   - Use the `temporalio/auto-setup` container image for simplicity, or write manifests for the individual components.
   - Document the full set of resources required.

2. **Implement `spike-temporal` worker:**
   - Use the `temporal-sdk-core` / `temporal-sdk` Rust crate.
   - Define a workflow: `github_ingestion_workflow`.
     - Calls `plan()` as the first activity.
     - Loops: calls `fetch_batch()` activity, then `store_batch()` activity, then `advance_watermark()` activity.
     - Between batches, check rate limit. If throttled, use `workflow.sleep()`.
   - Define activities in `activities.rs` — each wraps the corresponding `IngestionJob` method.
   - Configure retry policies per activity (e.g., `fetch_batch` retries with exponential backoff, `store_batch` retries with shorter intervals).
   - Register the worker with the Temporal server on a named task queue.

3. **Set up scheduling:**
   - Use Temporal's built-in schedule feature (`ScheduleClient`) to run the workflow every N hours.
   - Alternatively, use a Kubernetes CronJob that starts a workflow execution via the Temporal CLI.

4. **Build container image and deploy to k8s.**

5. **Test the full flow** — same test scenarios as Phase 2.

### Phase 4: Evaluation (Days 7-8)

Run both implementations against the same GitHub repository and write up the evaluation document.

## Evaluation Dimensions

Each dimension should be scored 1-5 and accompanied by specific observations.

### 1. Developer Experience

- **API ergonomics in Rust.** How natural does the SDK feel? Are there excessive boilerplate, weird lifetime issues, or `unsafe` blocks?
- **Documentation quality.** Are the Rust SDK docs complete? Are there Rust-specific examples?
- **Compile times.** Do the SDK dependencies significantly impact build times?
- **Debugging.** When something goes wrong, how easy is it to understand what happened? Are error messages helpful?
- **Testing.** Can workflows/services be unit-tested without running the server? What does the local development loop look like?

### 2. Operational Overhead

- **Deployment complexity.** How many k8s resources are needed? How many containers? How much configuration?
- **Resource consumption.** Measure CPU and memory usage of the orchestrator server and workers at idle and during a run. This is a single-node deployment; resource efficiency matters.
- **Upgrades.** What does upgrading the orchestrator look like? Are there database migrations?
- **Failure modes.** What happens if the orchestrator server goes down? How does it recover?
- **Backup/restore.** Is the orchestrator's state easy to back up? Is it in PostgreSQL (shared) or something else?

### 3. Rust SDK Maturity

- **Crate stability.** Is the crate at 1.0? How active is development? When was the last release?
- **Feature completeness.** Are all the features we need available in the Rust SDK, or are some only in Go/Java/TypeScript?
- **Community.** Are there other Rust users? GitHub issues, Discord activity, Stack Overflow questions?
- **Breaking changes.** How often does the API change?

### 4. Error Handling and Retry/Backfill Patterns

- **Retry configuration.** How granular is retry control? Can we set different policies per step?
- **Retry observability.** Can we see how many times a step was retried and why?
- **Rate limit integration.** How naturally does "wait until X time, then retry" fit into the model? Is durable sleep a first-class concept?
- **Partial failure.** If we fetch 5 pages successfully and the 6th fails, does the orchestrator resume from page 6, or start over?
- **Backfill.** How do we trigger a one-off run with a custom watermark? Is it a new workflow instance, a signal, something else?
- **Idempotency.** Does the orchestrator help with idempotency, or is that entirely on us?

### 5. Observability

- **Built-in UI.** Does the orchestrator ship with a dashboard? What can you see in it?
- **Logging integration.** Can we use `tracing` and see correlated logs across steps?
- **Metrics.** What metrics are exposed out of the box (Prometheus, OpenTelemetry)?
- **History/audit.** Can we see the full execution history of a past run — each step, its input/output, timing?
- **Alerting hooks.** Can we detect and alert on failed runs?

### 6. Fit for Our Architecture

- **Single-node suitability.** Is this orchestrator designed to run comfortably on a single machine, or is it optimized for distributed clusters?
- **PostgreSQL compatibility.** Can the orchestrator use our existing PostgreSQL instance, or does it need its own database?
- **Scheduling.** Does built-in scheduling meet our 3-6 hour cadence need, or do we need external cron?
- **State management.** How does the orchestrator's state model interact with our watermark strategy? Is there overlap/conflict?
- **Future source integration.** How easy would it be to add a second source (e.g., Jira) based on what we learned?

## Test Repository

Use a moderately active public repository for testing. Suggested: `canonical/cloud-init` or `canonical/lxd` — both have enough PR volume to exercise pagination and rate limit handling without being overwhelming.

Alternatively, use a smaller repo (e.g., `canonical/chisel`) for initial development, then switch to a larger one for the rate limit and backfill tests.

## Infrastructure Requirements

| Component | Restate Setup | Temporal Setup |
|-----------|--------------|----------------|
| Orchestrator server | 1 pod (single binary) | 3-4 pods (server, matching, history, optionally UI) |
| Orchestrator database | Embedded (RocksDB) or PostgreSQL | PostgreSQL (separate schemas) |
| Worker | 1 pod (the Rust service) | 1 pod (the Rust worker) |
| PostgreSQL (app data) | 1 pod (shared) | 1 pod (shared, but Temporal also needs DB access) |
| Total pods | 2-3 | 5-6 |

All deployed in a dedicated namespace (e.g., `spike-ingestion`) on the Canonical K8s snap cluster.

## Environment Setup

1. **Canonical K8s snap** — ensure the cluster is running and `kubectl` is configured.
2. **Container registry** — use a local registry (e.g., `registry:2` running in-cluster) or build images directly on the node.
3. **GitHub token** — a personal access token with `repo` read scope, stored as a k8s Secret.
4. **PostgreSQL** — deploy a simple single-instance PostgreSQL pod with a PVC for persistence. Both the application data and (if needed) Temporal's schemas live here.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Temporal Rust SDK too immature to build the spike | Medium | High | Time-box to 2 days. If blocked, document what failed and score accordingly. |
| Restate's newer status means hitting undocumented edge cases | Medium | Medium | Engage Restate's Discord/community early if stuck. |
| Deploying Temporal on single-node k8s is painful | Medium | Medium | Use `temporalio/auto-setup` to simplify. Accept that this is part of the evaluation. |
| GitHub rate limits slow down testing | Low | Low | Use a small repo for dev, only switch to a larger repo for specific rate limit tests. Cache responses locally during development. |

## Deliverables

1. **Working code** in `spikes/restate-vs-temporal/` — both implementations runnable.
2. **`evaluation.md`** — filled-in scoring and narrative for each dimension.
3. **A final recommendation** — confirming or revising the current lean toward Restate, with specific evidence.
4. **Identified patterns** — concrete code patterns and abstractions to carry forward into the production `ps-ingestion` crate, regardless of which orchestrator wins.

## Timeline

| Phase | Days | Description |
|-------|------|-------------|
| Phase 1: Shared core | 1-2 | Trait design, GitHub client, PostgreSQL storage, standalone test |
| Phase 2: Restate | 3-4 | Restate deployment, service implementation, testing |
| Phase 3: Temporal | 5-6 | Temporal deployment, worker implementation, testing |
| Phase 4: Evaluation | 7-8 | Side-by-side comparison, write-up, recommendation |

Total: approximately 8 working days.

## Open Questions to Resolve During the Spike

- Can Restate's built-in key-value state replace our `ingestion_watermarks` table, or do we want watermarks in PostgreSQL regardless for queryability?
- Does Temporal's Rust SDK support schedules natively, or do we need the Temporal CLI / a Go bootstrap?
- How do both orchestrators handle long-running rate limit waits (e.g., 15 minutes)? Is a durable sleep truly durable through restarts?
- What is the cold-start time for each orchestrator on our hardware? Does it meaningfully affect a 3-6 hour schedule?
