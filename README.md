<p align="center">
  <img src="icons/option3-spectrum-chevron.svg" width="128" alt="Prism">
</p>

# Prism

Engineering insights platform for understanding team and individual performance across multiple platforms (GitHub, Jira, Discourse, Launchpad, Google Drive, mailing lists).

Built in Rust (backend) + Vite/React (frontend) with PostgreSQL, gRPC (tonic + Connect), and Restate for ingestion orchestration.

## Getting Started

Enter the development environment (requires [Nix](https://nixos.org/download/)):

```bash
direnv allow   # or: nix develop
```

All tooling is provided by the Nix flake — nothing needs to be installed globally.

## Development

```bash
prek run -av              # Run all lints, tests, formatters
cargo build               # Build all Rust crates
cargo test                # Run all Rust tests
nix fmt                   # Format all files
buf lint                  # Lint proto files
buf generate              # Generate Rust + TypeScript code from protos
```

See [CLAUDE.md](CLAUDE.md) for full development guidelines, architecture, and conventions.

## Architecture

```
crates/
├── ps-core/          # Domain types, traits, error types, shared logic
├── ps-proto/         # Generated Rust code from proto definitions
├── ps-server/        # API server binary (tonic + tonic-web)
├── ps-ingestion/     # Ingestion service binary + source modules
├── ps-metrics/       # Metric computation logic
├── ps-migrate/       # Migration binary for k8s init container
└── psctl/            # Lightweight CLI client
frontend/             # Vite + React Router SPA + shadcn/ui
proto/prism/v1/       # Protobuf service definitions
k8s/                  # Kubernetes manifests (Kustomize)
migrations/           # PostgreSQL migrations (sqlx)
```

## License

AGPL-3.0-or-later

---

## Implementation Progress

### Phase 1 — Foundation

Core platform with GitHub as the single source, basic metrics, and team views.

- [x] **W0 — Project Tooling & Standards**
  Nix flake, devshell, treefmt, prek, clippy config, CLAUDE.md, direnv
- [x] **W1 — Restate vs Temporal Spike**
  Resolved: Restate confirmed (scored 3.9 vs Temporal 3.1)
- [x] **W2 — Backend Scaffolding**
  Rust workspace, crate structure, proto definitions, database migrations, CI
- [x] **W3 — Frontend Scaffolding**
  Vite + React Router SPA, Connect client generation, layout, component library setup
- [x] **W4 — Org Context, Directory Import & Source Configuration**
  People, teams, platform identities, and data source configuration — via API and UI
- [x] **W5 — GitHub Ingestion**
  Source adapter, Restate workflow, watermark tracking, identity resolution
- [x] **W6a — Ingestion Status UI**
  Ingestion status page, manual trigger/backfill buttons, run history
- [x] **W6b — Metrics Computation & Team UI**
  PR throughput, review turnaround, team comparison view

### Phase 2 — Breadth

Additional sources, flow metrics, and individual profiles.

- [ ] **W1 — Jira Source**
- [ ] **W2 — Discourse Source**
- [ ] **W3 — Flow Metrics & DORA**
- [ ] **W4 — Individual Profiles**

### Phase 3 — Intelligence

AI enrichment, embeddings, and agentic query interface.

- [ ] **W0 — Provider Foundation**
- [ ] **W1 — Enrichment Pipeline**
- [ ] **W2 — Embeddings & Semantic Search**
- [ ] **W3 — Agentic Query Interface**

### Phase 4 — Scale & Depth

Periodic insights, additional sources, and cross-platform correlation.

- [ ] **W1 — Periodic Insight Generation**
- [ ] **W2 — Launchpad Source**
- [ ] **W3 — Mailing List Source**
- [ ] **W4 — Google Drive Source**
- [ ] **W5 — Cross-Platform Correlation**
