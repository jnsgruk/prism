# Architecture Overview

## System Name

**Prism** — an engineering insights platform for understanding team and individual performance across multiple platforms.

## Guiding Principles

1. **Simple to operate** — runs on a desktop VM or small commodity hardware; avoid microservice sprawl
2. **Modular data sources** — adding a new platform (e.g. Google Drive) should be self-contained
3. **Incremental ingestion** — collect data every 3–6 hours, persist it, never start from scratch
4. **Domain-driven design** — model the business domain explicitly; let it guide boundaries
5. **Strong typing end-to-end** — Rust + gRPC + TypeScript gives us type safety from DB to UI

## High-Level Components

```
┌─────────────────────────────────────────────────────────┐
│                Frontend (Vite + React Router)             │
│              React + ShadCN + TypeScript                  │
└──────────────────────┬──────────────────────────────────┘
                       │ gRPC-Web / Connect
┌──────────────────────┴──────────────────────────────────┐
│                    API Gateway / Server                   │
│                     Rust (tonic)                          │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │ Auth Layer   │  │ Query Layer  │  │ AI/Reasoning   │  │
│  │ (sessions,   │  │ (read path)  │  │ (embeddings,   │  │
│  │  interceptor)│  │              │  │  LLM analysis) │  │
│  └─────────────┘  └──────────────┘  └────────────────┘  │
│  ┌──────────────┐                                        │
│  │ Metrics Calc │                                        │
│  │ (DORA, flow) │                                        │
│  └──────────────┘                                        │
└──────────────────────┬──────────────────────────────────┘
                       │
┌──────────────────────┴──────────────────────────────────┐
│                      PostgreSQL                          │
│         (source data, metrics, embeddings)                │
└──────────────────────┬──────────────────────────────────┘
                       │
┌──────────────────────┴──────────────────────────────────┐
│                 Ingestion Service                         │
│           Rust workers, orchestrated by                   │
│           Restate (confirmed)                             │
│                                                          │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   │
│  │ GitHub   │ │ Jira     │ │Discourse │ │Launchpad │   │
│  │ Source   │ │ Source   │ │ Source   │ │ Source   │   │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘   │
│  ┌──────────┐ ┌──────────┐                               │
│  │ Google   │ │ Mailing  │  ... more sources             │
│  │ Drive    │ │ Lists    │                               │
│  └──────────┘ └──────────┘                               │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│              Analysis Containers (ephemeral)              │
│         Scheduled via k8s API on demand                   │
│                                                          │
│  ┌──────────────────────────────────────────────────┐    │
│  │ Repo analysis pods: clone, scan, report back     │    │
│  │ Pre-built image with git, ripgrep, tokei, etc.   │    │
│  └──────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘

All workloads run on Kubernetes (Docker Desktop K8s for dev, production K8s TBD)
```

## Component Responsibilities

### Ingestion Service

- Runs on a schedule (3–6 hour cadence) via Restate
- Each source is a self-contained module implementing a common `Source` trait
- Tracks its own cursor/watermark per source so it can resume or backfill
- Handles rate limiting gracefully with visibility into wait times
- Optionally enriches data at ingestion time (embeddings, sentiment)

### API Server

- Rust service exposing gRPC (via `tonic` + `tonic-web` for browser clients)
- **Authentication & session management** — Argon2id password hashing, opaque session tokens, async auth interceptor on all RPCs (see [07-authentication.md](./07-authentication.md))
- Read-heavy workload: query persisted data, compute metrics on demand or from pre-aggregated tables
- Serves the AI/reasoning layer when agentic queries are made
- Manages configuration (teams, users, directory imports)
- Schedules analysis containers via k8s API and tracks their lifecycle

### Analysis Containers

- Ephemeral pods spun up on demand for repository analysis tasks
- Ubuntu-based container images, slimmed with [Chisel](https://github.com/canonical/chisel) to include only the needed slices
- Pre-built image with common tools: `git`, `ripgrep`, `tokei`, language runtimes as needed
- Clones repos, runs analysis, reports results back to the API server
- Lifecycle managed via k8s API — the server creates Jobs/Pods, streams logs, collects results
- Resource-limited to prevent a large clone from starving the system
- The agentic layer orchestrates what runs inside these containers

### Frontend

- Vite + React Router SPA + ShadCN UI component library
- TypeScript with strict mode, enforced by oxlint/oxfmt; type-checked with typescript-go
- Bun as runtime and package manager; static SPA served by Caddy in production (no Node.js runtime)
- Connects to API via gRPC-Web (or Connect protocol)
- Dashboards for team comparisons, individual contribution views, ingestion status

### Database (PostgreSQL)

- Single PostgreSQL instance (simplicity)
- `pgvector` extension for embedding storage
- Schema designed per bounded context (see domain model)
- Tracks ingestion watermarks and job history

## Deployment Model

- Single machine (VM or bare metal)
- **Docker Desktop Kubernetes** for development — validated with Tilt in the cortexlabsai/connect repo
  - Full k8s API for scheduling workloads, tracking lifecycle, streaming logs
  - All system components (server, ingestion, PostgreSQL) run as k8s workloads
  - Enables ephemeral container scheduling for on-demand repo analysis (clone, run tools, report back)
  - **Envoy Gateway** provides path-based routing so all services are accessible via `localhost` (no port juggling)
  - Production K8s runtime TBD (Canonical K8s snap, k3s, or cloud — the manifests are portable)
- **External dependency: AI/LLM APIs** — embeddings and agentic reasoning require cloud API access (OpenRouter and/or Google Gemini API). The system abstracts providers behind a common trait so the choice is configurable.

## Crate / Package Structure (Rust side)

> **Note:** For detailed directory organisation within each crate and the frontend, including feature-first conventions, naming rules, and tier escalation, see [18-code-structure.md](./18-code-structure.md).

```
prism/
├── crates/
│   ├── ps-core/          # Domain types, traits, shared logic
│   │   └── src/
│   │       └── auth/     # Password hashing, token generation, session management
│   ├── ps-workers/       # Restate worker binary — ingestion, team sync, metrics compute
│   │   ├── src/
│   │   │   ├── sources/
│   │   │   │   ├── github.rs
│   │   │   │   ├── jira.rs
│   │   │   │   ├── discourse.rs
│   │   │   │   ├── launchpad.rs
│   │   │   │   ├── google_drive.rs
│   │   │   │   └── mailing_list.rs
│   │   │   └── ...
│   ├── ps-server/        # API server binary (includes AuthService + auth interceptor)
│   ├── ps-metrics/       # Metric computation (DORA, flow, etc.)
│   ├── ps-reasoning/     # AI/LLM integration, embeddings
│   ├── ps-proto/         # Protobuf definitions + generated code
│   └── ps-cli/           # psctl CLI tool (thin gRPC client, depends on ps-proto only)
├── frontend/             # Vite + React Router SPA
├── proto/                # .proto source files
├── plans/
├── SPEC.md
└── Cargo.toml            # Workspace root
```

## Key Technology Choices

| Concern            | Choice                   | Rationale                                                |
| ------------------ | ------------------------ | -------------------------------------------------------- |
| Backend language   | Rust                     | Spec requirement; strong type safety, performance        |
| Authentication     | Argon2id + session tokens | Password hashing via RustCrypto `argon2`; opaque Bearer tokens; async tonic interceptor. See [07-authentication.md](./07-authentication.md) |
| API protocol       | gRPC (tonic) + Connect   | Spec requirement; strong typing matches Rust             |
| Protobuf tooling   | Buf (buf.build)          | Proto linting, breaking change detection, code generation |
| Database           | PostgreSQL + pgvector    | Reliable, flexible, supports embeddings                  |
| SQL layer          | sqlx                     | Compile-time checked queries, offline mode for CI        |
| Object storage     | S3-compatible (Phase 3)  | RustFS or Garage for artifacts; `object_store` crate client; see [Object Storage Strategy](#object-storage-strategy) |
| Orchestration      | Restate                  | Durable workflows; single binary, embedded RocksDB; [spike confirmed](~/code/canonical/temporal-restate-spike/evaluation.md) |
| Dev environment    | Nix flake + direnv       | Reproducible tooling, automatic devshell entry           |
| Build tooling      | clang + mold             | Fast compilation and linking for rapid iteration         |
| Formatting         | treefmt (rustfmt, nixfmt, oxfmt, deadnix, shfmt) | Unified formatting, no prettier needed |
| Pre-commit         | prek (git-hooks.nix)     | treefmt, clippy, tests, oxlint, buf-lint                 |
| Clippy             | Pedantic + restriction subset | Production guardrails with targeted allows for tonic/sqlx |
| Frontend framework | Vite + React Router + React | SPA with client-side routing; static build served by Caddy |
| UI components      | ShadCN + Base UI         | Composable owned components + accessible primitives      |
| Server state       | React Query (TanStack)   | Caching, refetching, optimistic updates for API data     |
| Frontend API       | @connectrpc/connect-web  | Type-safe clients generated from proto definitions       |
| Runtime validation  | Zod                      | Schema validation at system boundaries (forms, URL params, env config); complements proto-generated types |
| Frontend tooling   | TypeScript (typescript-go), Bun, oxlint + oxfmt | Strict, modern, fast — Bun as runtime/package manager, typescript-go for type-checking |

## Object Storage Strategy

**Introduced in Phase 3.** PostgreSQL handles all structured data, metrics, and embeddings. Object storage is added when the AI/reasoning layer starts producing artifacts that don't belong in the database.

### What goes where

| PostgreSQL | Object Storage |
|-----------|---------------|
| Structured metrics, contributions, embeddings | Generated reports (PDFs, rendered summaries) |
| JSONB metadata, enrichment scores | Repo scan raw output and logs |
| Reasoning traces (structured) | Large agentic artifacts (exportable reports) |
| Short content (`TEXT` columns) | Cached API responses for replay/debugging |

### Server: S3-compatible, provider-agnostic

**Do not lock into MinIO** — its open-source edition is no longer actively maintained. Evaluate at deployment time:

| Candidate | Notes |
|-----------|-------|
| **RustFS** | Apache 2.0, Rust-based, S3-compatible, single binary. Alpha (v1.0.0-alpha) but actively developed. Best performance for small objects. |
| **Garage** | Production-ready, lightweight (~1GB RAM), S3-compatible, single binary. Designed for modest hardware. |
| **AWS S3** | If the deployment moves to cloud. Same client, different endpoint config. |

The choice is a deployment decision, not an application decision — the application talks S3 API only.

### Client: `ObjectStore` trait

The Rust side uses the `object_store` crate (Apache Arrow project) which provides a unified async trait across S3, GCS, Azure, and local filesystem:

```rust
/// Application-level wrapper around object_store::ObjectStore.
/// All services that read/write artifacts go through this trait.
#[async_trait]
trait ArtifactStore: Send + Sync {
    async fn put(&self, key: &ArtifactKey, bytes: Bytes) -> Result<()>;
    async fn get(&self, key: &ArtifactKey) -> Result<Bytes>;
    async fn presign_get(&self, key: &ArtifactKey, expiry: Duration) -> Result<Url>;
    async fn delete(&self, key: &ArtifactKey) -> Result<()>;
    async fn list(&self, prefix: &str) -> Result<Vec<ArtifactKey>>;
}
```

`ArtifactKey` is a typed wrapper encoding the artifact type and ID in the key path (e.g. `insights/2026-03/team-x-report.pdf`, `scans/{scan_id}/repo-output.json`).

In tests, `object_store::local::LocalFileSystem` is used — no S3 server needed. In production, `object_store::aws::AmazonS3` pointed at the S3-compatible endpoint.

### Deployment

One StatefulSet with a PVC (~200MB RAM idle). One HTTPRoute in the Envoy Gateway for pre-signed URL access (so the frontend can download artifacts directly without proxying through the API server). Added to the Tiltfile alongside PostgreSQL and Restate.

### Bucket layout

```
ps-artifacts/
├── insights/          # Generated insight reports, periodic summaries
├── scans/             # Repo scan raw output, logs, analysis artifacts
├── conversations/     # Exported conversation transcripts
└── cache/             # Raw API response cache (debugging/replay)
```

## What This Document Does NOT Cover

- Domain model details → see [02-domain-model.md](./02-domain-model.md)
- Ingestion pipeline design → see [03-data-ingestion-strategy.md](./03-data-ingestion-strategy.md)
- Database schema → see [04-database-design.md](./04-database-design.md)
- Frontend details → see [05-frontend-strategy.md](./05-frontend-strategy.md)
- AI/reasoning approach → see [06-ai-reasoning.md](./06-ai-reasoning.md)
- Authentication & authorisation → see [07-authentication.md](./07-authentication.md)
- Open decisions → see [08-open-questions.md](./08-open-questions.md)
