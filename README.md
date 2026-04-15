<p align="center">
  <img src="icons/option3-spectrum-chevron.svg" width="128" alt="Prism">
</p>

# Prism

Engineering insights platform for understanding team and individual performance across multiple platforms (GitHub, Jira, Discourse, Launchpad, Google Drive, mailing lists).

Built in Rust (backend) + Vite/React (frontend) with PostgreSQL, gRPC (tonic + Connect), and Restate for ingestion orchestration.

## Getting Started

Install [mise](https://mise.jdx.dev/) then set up the development environment:

```bash
mise trust
mise run install-deps     # Install native OS packages
mise install              # Install all dev tools
prek install              # Install git hooks
cd frontend && bun install  # Install frontend dependencies
```

## Development

```bash
prek run -av              # Run all lints, tests, formatters
mise run fmt              # Format all files
mise run check            # Full CI validation (fmt + lint + typecheck)
mise run test             # Run all tests (Rust + frontend)
mise run generate         # Generate proto types + SQLx cache
cargo build               # Build all Rust crates
buf lint                  # Lint proto files
buf generate              # Generate Rust + TypeScript code from protos
```

See [CLAUDE.md](CLAUDE.md) for full development guidelines, architecture, and conventions.

## Documentation

See [docs/00-overview.md](docs/00-overview.md) for an overview of the system, tech stack, and codebase map. See [docs/](docs/) for architecture, design decisions, and technical documentation. See [CLAUDE.md](CLAUDE.md) for development guidelines and coding conventions.

## License

AGPL-3.0-or-later
