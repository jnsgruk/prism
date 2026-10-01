# Infrastructure

## Development Environment (mise + prek)

All dev tooling is managed by [mise](https://mise.jdx.dev/) (`.mise.toml`). Git hooks are managed by [prek](https://github.com/jdx/prek) (`prek.toml`). Run `mise install` then `prek install` to set up.

**System dependencies** (install via your package manager): `clang`, `mold`, `pkg-config`, `libssl-dev`, `postgresql-client`.

mise provides:
- **Rust:** stable toolchain (clippy, rust-analyzer, rustfmt)
- **Protobuf:** protoc, buf (lint, generate, breaking-change detection)
- **Database:** sqlx-cli
- **Frontend:** bun, node, vp (vite-plus — wraps oxfmt, oxlint, vitest, tsgo)
- **K8s:** tilt, kubectl, kubectx, helm
- **Testing:** cargo-nextest, cargo-watch

**Tasks** follow a `verb:scope` convention (`mise run fmt`, `mise run check`, `mise run test`, etc.). See `.mise.toml` for the full list.

Pre-commit hooks run fmt-check, clippy, buf-lint, frontend lint/typecheck/test, and cargo-test.

## Continuous Integration

GitHub Actions runs four independent jobs in `.github/workflows/ci.yml`:

- **Rust Checks:** `mise run check:rs` (formatting and Clippy).
- **Rust Tests:** `mise run test:rs` (database setup binary and the full nextest suite).
- **Frontend:** frozen dependency installation, `mise run check:ts` and `mise run test:ts`.
- **Protobuf:** `mise run check:proto`, plus breaking-change detection against the PR base branch on pull requests.

Each job installs only its required mise tools and runs tasks with `--skip-tools`
to avoid installing the rest of the development toolset. Only Rust jobs install
native build dependencies. Rust builds retain SQLx offline mode and warnings as errors;
tests keep the existing Docker-based PostgreSQL and Restate fixtures.

`Swatinem/rust-cache` caches Cargo downloads and compiled dependencies separately
for each Rust job. Its default keys account for the job ID, Rust toolchain,
Cargo manifests/lockfiles, Cargo configuration and Rust flags. Workspace crates
are rebuilt; cold runs and dependency changes still incur compilation costs.
Mise tool caches also use separate prefixes per job to avoid collisions between
tool subsets. Local `prek run -av` remains the complete pre-commit gate.

Required branch checks should use **Rust Checks**, **Rust Tests**, **Frontend**
and **Protobuf** in place of the former **Lint & Test** and
**Proto Breaking Changes** checks.

## Containers

All containers are Ubuntu-based, slimmed with Chisel for production images.

The multi-stage Dockerfile (`crates/Dockerfile`) supports:
- **Dev targets:** Ubuntu 24.04 base with libc, libssl, ca-certificates; runs as unprivileged "prism" user
- **Prod targets:** Minimal scratch images via Chisel (base-files, ca-certificates, libssl3)
- **Build args:** `PROFILE` (debug for Tilt, release for CI), `BIN` (ps-server, ps-workers, ps-migrate)
- BuildKit cache mounts on cargo registry and target/ for fast incremental rebuilds

The frontend container uses Caddy to serve static files with SPA fallback.

## Protobuf and Code Generation

Proto files live in `proto/canonical/prism/v1/` — one file per domain area:

| File | Domain |
| --- | --- |
| `auth.proto` | Login, setup, session management |
| `admin.proto` | API tokens, reset, system info |
| `backup.proto` | Backup export, preview, restore |
| `config.proto` | Source CRUD, secrets, connection tests |
| `org.proto` | People, teams, identities, repositories |
| `metrics.proto` | Snapshots, contributions, flow metrics |
| `insights.proto` | Enrichment aggregation, insight queries |
| `reasoning.proto` | AI settings, enrichments, conversations |
| `handlers.proto` | Ingestion/system handler dispatch |
| `common.proto` | Shared message types |

**Workflow after proto changes:**

1. `buf lint` — validate against Buf standard rules
2. `buf generate` — produces Rust types in `crates/ps-proto/src/gen/` and TypeScript Connect clients in `frontend/lib/api/gen/`
3. `buf breaking --against .git#branch=main` — catch compatibility issues
4. Rebuild both backend and frontend

The frontend Connect transport auto-discovers services. New service hooks go in `lib/hooks/` if shared or `views/<feature>/hooks/` if feature-local.

## Kubernetes

Manifests live in `k8s/` using Kustomize:

```
k8s/
  base/                    # Core service manifests
    namespace.yaml
    postgres.yaml          # PostgreSQL + pgvector
    restate.yaml           # Restate orchestrator
    ps-migrate.yaml        # Init container (runs migrations)
    ps-server.yaml         # API server
    ps-workers.yaml        # Restate workers
    ps-frontend.yaml       # Caddy serving static SPA
    gateway.yaml           # Route definitions
    agent-rbac.yaml        # RBAC for dynamic agent pod management
    agent-network-policy.yaml
    secrets.yaml
  gateway/                 # Envoy Gateway (Helm chart v1.7.0)
```

**Agent pods** are created dynamically by ps-agent via the K8s API when agentic queries are initiated. RBAC grants ps-workers permission to create/delete pods in the namespace.

**Shared workspace PVC** (`prism-workspaces`, defined in `ps-server.yaml`): A single ReadWriteMany PVC mounted by ps-server (read-only at `/workspaces`) and all agent pods (read-write at `/workspace` via `subPath: {conversation_id}`). Restate also stores its durable local state on this claim at `/restate-data`, isolated with `subPath: restate-data`. This reuse avoids exhausting the development cluster's rawfile CSI volume pool while retaining state across pod restarts. Workspace directories are cleaned up when conversations are deleted. The claim requires an RWX-capable storage class; production should give Restate a dedicated durable volume to isolate orchestration state from workspace capacity and failure domains.

### PostgreSQL memory

`k8s/base/postgres.yaml` mounts a memory-backed `emptyDir` at `/dev/shm`, capped
at 256 MiB. PostgreSQL requests 512 MiB and has a 1 GiB container memory limit.
The runtime's default 64 MiB shared-memory mount is too small for simultaneous
parallel hash joins during historical insight refreshes. PostgreSQL keeps
`dynamic_shared_memory_type=posix`, `work_mem=4MB`, and the default
`max_parallel_workers_per_gather=2`; no persistent serial-query override is
required.

The mount cap does not reserve memory or add memory outside the container
limit. Memory-backed volume pages count toward that limit. With the observed
128 MiB `shared_buffers` and a fully used 256 MiB mount, about 640 MiB remains
for private backend/worker allocations, other shared structures, autovacuum,
kernel accounting and file cache. This is planning headroom, not a strict
PostgreSQL memory bound: multiple sort/hash nodes, sessions and maintenance
workers can still exceed it. Increasing the mount without increasing the old
512 MiB pod limit would replace shared-memory failures with an OOM risk.
Insight refresh concurrency is bounded in application code as described in
[Database Design](02-database.md#historical-invalidations-and-account-discovery-coverage).
Reassess both limits and concurrency when scaling data or worker replicas.

Before rollout, check active Restate invocations and PostgreSQL sessions,
including agent queries. This StatefulSet change replaces the single database
pod and briefly interrupts connections; the existing data PVC is retained.
Apply during an idle window, wait for readiness, then verify `/dev/shm`,
resource limits, PostgreSQL settings and OOM counters. Use `EXPLAIN (ANALYZE,
BUFFERS, SETTINGS)` on representative bounded queries and sample `df /dev/shm`
and cgroup `memory.current`, `memory.stat`, and `memory.events` during concurrent
refreshes. Low usage after failure/completion misses transient allocations;
file cache near the limit alone is not proof of an OOM, so inspect reclaimable
cache and `oom`/`oom_kill` events too. Never delete live PostgreSQL shared-memory
files to free space.

After a worker rollout, wait for old pods to exit before forcing Restate
deployment discovery through `http://ps-workers:9081/`; startup discovery via
the Service can still reach an old replica. Verify `IngestionChunkService`
retains its separate 15-minute inactivity timeout and existing abort grace.

Emergency serial execution (`ALTER SYSTEM SET max_parallel_workers_per_gather
= 0; SELECT pg_reload_conf();`) can unblock a retry, at the cost of latency.
After recovery, run `ALTER SYSTEM RESET max_parallel_workers_per_gather;
SELECT pg_reload_conf();` and verify `pg_settings` reports the intended setting
and source. Resume the existing retryable Restate invocation rather than clearing
its journal or creating duplicate backfill work.

PostgreSQL's [resource settings](https://www.postgresql.org/docs/17/runtime-config-resource.html)
describe per-operation and parallel-worker memory budgets; Kubernetes documents
the container memory accounting of [memory-backed emptyDir volumes](https://kubernetes.io/docs/concepts/storage/volumes/#emptydir).

## Gateway

Envoy Gateway handles TLS termination and routes requests:
- Frontend static assets served by Caddy
- gRPC API traffic routed to ps-server
- Connect protocol (gRPC-Web) for browser clients

## Local Development

The Tiltfile supports both Canonical K8s and Docker Desktop K8s for local development — no flags needed:
- Docker builds with BuildKit cache mounts (debug mode, incremental)
- Resource dependencies: ps-migrate -> ps-server -> ps-workers -> ps-frontend
- Port forwards: ps-server (8080), ps-workers (9080), ps-frontend (3000), postgres (5432), restate (9070), rustfs (9000-9001)
- Live-reload on code changes

The pre-commit gate is `prek run -av` — all lints, tests, and formatters must pass before committing.

## Backup & Restore

See [docs/09-backup-restore.md](09-backup-restore.md) for the full backup/restore system documentation — archive format, architecture, CLI usage, authentication, testing, and known limitations.
