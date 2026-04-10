# Prism Overview

Prism is an engineering insights platform for understanding team and individual performance across multiple platforms — GitHub, Jira, Discourse, Launchpad, Google Drive, and mailing lists. It ingests data from these sources, computes metrics (DORA, flow, individual contributions), enriches data with AI, and provides an agentic query interface for natural-language exploration.

## Tech Stack

| Layer | Technology |
| --- | --- |
| Backend | Rust (10 crates) |
| Frontend | Vite + React Router SPA, shadcn/ui, TypeScript (strict, checked with typescript-go) |
| Database | PostgreSQL + pgvector |
| API | gRPC (tonic + tonic-web), Connect protocol for browser clients |
| Orchestration | Restate (durable execution for ingestion and background work) |
| AI | Rig framework (Google Gemini), OpenCode agent in ephemeral K8s pods |
| Package manager | Bun (frontend), Cargo (Rust) |
| Dev environment | mise + prek |
| Containers | Ubuntu-based with Chisel, multi-stage Dockerfile |
| Deployment | Kubernetes (Kustomize), Envoy Gateway, Caddy for static frontend |

## How to Read These Docs

| Document | Scope |
| --- | --- |
| **This file** | What Prism is and how the docs fit together |
| [CLAUDE.md](../CLAUDE.md) | Day-to-day coding conventions, style rules, UI patterns, workflow commands. The authoritative reference for *how to write code* in this project. |
| [README.md](../README.md) | Quickstart: entering the dev environment, basic commands |
| `docs/01-architecture` | System design, crate roles, code organisation principles |
| `docs/02-database` | Schema design, repository pattern, migrations |
| `docs/03-ingestion` | Data ingestion pipeline and Restate handler architecture |
| `docs/04-ai-reasoning` | AI enrichment, embeddings, and agentic query |
| `docs/05-frontend` | Frontend stack, layout, state management |
| `docs/06-infrastructure` | Containers, Kubernetes, proto tooling, local dev |
| `docs/07-development` | Testing strategy, naming conventions, workflow |
| `docs/08-decision-log` | Dated log of significant architectural decisions |

The docs describe **how things are now and why** — not a historical log of how we got here. When making significant changes, update the relevant doc and add a decision log entry.

## Codebase Map

```
crates/                     # Rust workspace
  ps-core/                  #   Domain types, repository layer (all DB access), auth, crypto
  ps-proto/                 #   Generated gRPC code from protobuf (never hand-edit)
  ps-server/                #   API server binary — gRPC services, auth interceptor
  ps-workers/               #   Restate worker binary — ingestion, metrics, AI pipeline
  ps-metrics/               #   Metric computation logic (DORA, flow)
  ps-reasoning/             #   LLM abstraction via Rig — enrichment, embeddings, insights
  ps-agent/                 #   Agent container lifecycle — K8s pod management
  ps-mcp/                   #   MCP server for agent containers — data tools + S3 artifacts
  ps-migrate/               #   Migration binary for K8s init container
  psctl/                    #   Lightweight CLI client

frontend/                   # Vite + React SPA
  views/                    #   Feature modules (admin, teams, ingestion, ask, etc.)
  components/               #   Shared UI components (app shell, data-table, shadcn/ui)
  lib/                      #   Service plumbing (API clients, hooks, session, providers)

proto/canonical/prism/v1/   # Protobuf service definitions (9 files)
migrations/                 # PostgreSQL migrations (run by ps-migrate)
k8s/                        # Kubernetes manifests (Kustomize)
docs/                       # This directory — architecture and decision documentation
```
