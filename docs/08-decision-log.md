# Decision Log

Significant architectural decisions in reverse chronological order. Each entry records what was decided and why, so future contributors understand the reasoning without needing to read historical implementation plans.

---

## 2026-04-08 — Remove RustFS, Use Shared PVC for Workspace Storage

**Context:** RustFS (S3-compatible object storage) was deployed but never actively used. Workspace files were already stored on a shared ReadWriteMany PVC (`prism-workspaces`). The ArtifactStore code, S3 env vars, and RustFS deployment were dead weight.

**Decision:** Remove RustFS entirely. Standardise on the shared PVC approach with workspace garbage collection, streaming file downloads, and storage monitoring.

**Rationale:**
- RustFS had zero actual reads or writes — the PVC replaced it before any usage
- Shared PVC is simpler to operate: no separate deployment, credentials, or bucket management
- Streaming gRPC (64KB chunks) replaces base64 data URLs for file serving, reducing server memory
- Workspace directories are now cleaned up when conversations are deleted (via Restate handler)
- Storage usage (PVC + database) is now visible in the admin System tab

---

## 2026-04-07 — Strip OpenRouter, Keep Google Gemini Only

**Context:** Prism supported two AI providers (Google Gemini, OpenRouter) with dual-provider abstraction across routing, cost tracking, catalogue fetching, image generation, and frontend UI.

**Decision:** Remove OpenRouter entirely. Hardcode Google Gemini as the sole provider.

**Rationale:**
- Prism uses Gemini exclusively — OpenRouter support added complexity without value
- Removes ~200+ lines of enum variants, separate API clients, dual catalogue paths
- Simplifies frontend (single provider becomes static label)
- OpenRouter was a leaf dependency with no downstream features depending on it

---

## 2026-04-01 — Move SSE Streaming from Restate to ps-server

**Context:** Agentic query initially ran OpenCode SSE streaming (5+ minutes of non-journaled work) inside Restate Object handlers. Restate's 5-minute ABORT_TIMEOUT caused races: streams suspended mid-way, replay logic deleted recovery data, handlers retried forever.

**Decision:** Split into fast `prepare_query` (pod lifecycle, ~90s) in Restate, and SSE streaming in ps-server directly.

**Rationale:**
- Eliminates long-running non-journaled work in a journaled system
- ps-server already holds gRPC streams open for the duration — SSE fits naturally there
- Atomic concurrency guard via CAS update prevents duplicate claims
- Watchdog handler resets stale conversations; no more stuck invocations

---

## 2026-04-01 — Centralised Repository Pattern

**Context:** SQL queries were inline in gRPC handlers and source adapters, mixing database access with business logic.

**Decision:** Create a centralised repository layer in ps-core with one Repo struct per schema, bundled in a `Repos` struct.

**Rationale:**
- Encapsulates database access behind domain-oriented interfaces
- Tests run against real PostgreSQL — repos are concrete Clone-able structs, no trait mocking
- Shared across ps-server and ps-workers without a separate crate
- Clean DDD layering: presentation -> application -> domain -> infrastructure

---

## 2026-03-24 — OpenCode in Ephemeral K8s Pods for Agentic Query

**Context:** Phase 3 needed a natural-language query interface. Options: hand-roll an LLM orchestration loop, or use an existing agent framework.

**Decision:** Deploy OpenCode agent framework in ephemeral K8s pods with ps-mcp as the MCP server.

**Rationale:**
- Battle-tested agent orchestration (tool-call -> execute -> reprompt) without building 500+ lines of retry/iteration logic
- MCP stdio transport lets Prism provide data tools without modifying the agent framework
- Container isolation — agents can run code analysis tools safely
- Provider-agnostic via OpenCode's SDK

---

## 2026-03-18 — Adopt Rig Framework for LLM Abstraction

**Context:** Hand-rolled 1,100+ lines of provider abstraction code (Google, OpenRouter clients, request/response types). Phase 3 would require agent orchestration, structured extraction, and embeddings.

**Decision:** Adopt Rig (`rig-core`) as the LLM framework.

**Rationale:**
- Eliminates ~800 lines of provider HTTP client code
- Structured extraction via derive macros for enrichment
- EmbeddingModel trait for embeddings
- 20+ provider support with active maintenance
- Wrapped behind thin TaskRouter adapter to mitigate pre-1.0 API instability

---

## 2026-03-18 — RustFS for S3 Storage *(superseded 2026-04-08)*

**Context:** Agentic query generates artifacts (charts, reports) that need durable storage accessible to the frontend.

**Decision:** Use RustFS as self-hosted S3-compatible object storage.

**Rationale:**
- Self-hosted, no cloud dependency
- S3-compatible API — standard tooling and SDKs work out of the box
- Lightweight single-binary deployment suitable for single-node K8s

**Superseded by:** Shared PVC approach (see 2026-04-08 entry). RustFS was never actually used for workspace files — the shared PVC replaced it before any real usage.

---

## 2026-03-13 — Feature-First Code Organisation

**Context:** Codebase was at risk of fragmenting features across layer-first directories (handlers, services, models, repo).

**Decision:** Organise all code feature-first — one directory per domain feature, subdivided by concern.

**Rationale:**
- A feature change stays in one directory instead of touching four scattered locations
- New developers understand a feature by reading one module
- Scales cleanly with tier model (single file -> siblings -> nested subdirectories)
- Same structure in Rust and TypeScript for consistency

---

## 2026-03-13 — Next.js to Vite Migration

**Context:** Frontend was built with Next.js but used `"use client"` on every page — gaining no SSR, server components, API routes, or middleware benefits.

**Decision:** Migrate to Vite + React Router + Caddy.

**Rationale:**
- Removes Next.js build complexity and Node.js production runtime
- Vite's native ESM dev server is faster for SPA development
- React Router explicit route definitions are clearer than file convention magic
- Caddy handles SPA fallback natively; lighter production container
- All dependencies (TypeScript, Tailwind, shadcn/ui) are framework-agnostic

---

## 2026-03-13 — Restate over Temporal for Orchestration

**Context:** Needed a durable orchestrator for data ingestion workflows. Spiked both Restate and Temporal.

**Decision:** Adopt Restate. Spike scored Restate 3.9 vs Temporal 3.1.

**Rationale:**
- Simpler deployment (single binary vs Temporal's 3-4 pods)
- Cleaner Rust SDK with fewer breaking changes
- Native durable sleep for rate-limit handling
- Lower operational overhead on single-node K8s
- Ingestion logic stays in ps-core, independent of orchestrator choice

---

## 2026-03-12 — typescript-go for Type Checking

**Context:** TypeScript type checking with tsc was slow in CI and local development.

**Decision:** Use typescript-go (`tsgo`) for all TypeScript type checking.

**Rationale:**
- Significantly faster type checking than standard tsc
- Drop-in replacement — same type system, same diagnostics
- Used in both CI and local pre-commit hooks

---

## 2026-03-12 — Bun over pnpm

**Context:** Needed a JavaScript/TypeScript runtime and package manager for the frontend.

**Decision:** Use Bun as both runtime and package manager, replacing pnpm.

**Rationale:**
- Faster install and runtime than Node.js + pnpm
- Simpler lockfile
- Built-in test runner (though we use Vitest for React Testing Library compatibility)

---

## 2026-03-12 — Ubuntu + Chisel Containers

**Context:** Needed a container base image strategy. Options: Alpine, distroless, Ubuntu, scratch.

**Decision:** Ubuntu-based images, slimmed with Chisel for production.

**Rationale:**
- Better compatibility with native dependencies than Alpine (musl vs glibc)
- Chisel produces minimal layers (base-files, ca-certificates, libssl3) comparable to distroless
- Familiar debugging environment when needed
- Consistent with Canonical's container strategy
