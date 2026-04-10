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
| `admin.proto` | API tokens, backup, reset |
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

**Shared workspace PVC** (`prism-workspaces`, defined in `ps-server.yaml`): A single ReadWriteMany PVC mounted by both ps-server (read-only at `/workspaces`) and all agent pods (read-write at `/workspace` via `subPath: {conversation_id}`). This allows ps-server to serve workspace file listings directly from the filesystem. Agent pods are isolated to their own conversation subdirectory. Workspace directories are cleaned up when conversations are deleted. Requires an RWX-capable storage class (Docker Desktop hostpath works on a single node; production needs NFS, EFS, or similar).

## Gateway

Envoy Gateway handles TLS termination and routes requests:
- Frontend static assets served by Caddy
- gRPC API traffic routed to ps-server
- Connect protocol (gRPC-Web) for browser clients

## Local Development

The Tiltfile configures Docker Desktop K8s for local development:
- Docker builds with BuildKit cache mounts (debug mode, incremental)
- Resource dependencies: ps-migrate -> ps-server -> ps-workers -> ps-frontend
- Port forwards: ps-server (8080), ps-workers (9080), ps-frontend (3000), postgres (5432), restate (9070), rustfs (9000-9001)
- Live-reload on code changes

The pre-commit gate is `prek run -av` — all lints, tests, and formatters must pass before committing.
