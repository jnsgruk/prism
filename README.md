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
├── ps-workers/       # Restate worker binary — ingestion, team sync, metrics compute
├── ps-metrics/       # Metric computation logic
├── ps-migrate/       # Migration binary for k8s init container
└── psctl/            # Lightweight CLI client
frontend/             # Vite + React Router SPA + shadcn/ui
proto/prism/v1/       # Protobuf service definitions
k8s/                  # Kubernetes manifests (Kustomize)
migrations/           # PostgreSQL migrations (sqlx)
```

## Documentation

See [docs/](docs/) for architecture, design decisions, and technical documentation. See [CLAUDE.md](CLAUDE.md) for development guidelines and coding conventions.

## License

AGPL-3.0-or-later
