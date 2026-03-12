# Implementation Plan

High-level roadmap for Prism, broken into four phases. Each phase has its own detailed plan document.

## Design References

These documents capture the architectural decisions and domain design that inform all phases:

| Document | Covers |
|----------|--------|
| [01 Architecture Overview](./01-architecture-overview.md) | System components, crate structure, technology choices, deployment model |
| [02 Domain Model](./02-domain-model.md) | Bounded contexts, entity relationships, identity resolution, repository analysis |
| [03 Data Ingestion Strategy](./03-data-ingestion-strategy.md) | Source trait, watermarks, rate limits, backfill, DB-backed config |
| [04 Database Design](./04-database-design.md) | All schemas (auth, config, org, activity, metrics, reasoning), sqlx patterns, migrations |
| [05 Frontend Strategy](./05-frontend-strategy.md) | Next.js, ShadCN/Radix, nanostores, React Query, Connect, Tremor charts |
| [06 AI Reasoning](./06-ai-reasoning.md) | Traceability, model providers, agent tools, cost model |
| [08 Open Questions](./08-open-questions.md) | Resolved decisions and remaining open items |
| [07 Authentication](./07-authentication.md) | Auth schema, session tokens, first-run wizard, auth interceptor, future OIDC path |

## Spikes

| Spike | Status |
|-------|--------|
| [Restate vs Temporal](./09-spike-restate-vs-temporal.md) | Complete — Restate confirmed (scored 3.9 vs 3.1). See [evaluation](~/code/canonical/temporal-restate-spike/evaluation.md) |
| [Launchpad API](./10-spike-launchpad-api.md) | Research complete — implementation in Phase 4 |

---

## Phase Overview

### Phase 1: Foundation — [Detail Plan](./12-phase1-foundation.md)

Stand up the core platform and prove the end-to-end data pipeline with a single source.

- Project scaffolding (Rust workspace, Next.js app, PostgreSQL, proto definitions, buf config)
- Authentication — `auth` schema, Argon2id passwords, session tokens, auth interceptor, first-run setup wizard
- Org context (people, teams, directory import)
- GitHub source (highest signal, most mature from contristat)
- Basic metrics computation (PR throughput, review turnaround)
- Team comparison view in frontend
- Ingestion status page

**Exit criteria:** A deployed system that ingests GitHub data for configured teams and displays team-level PR metrics in the UI.

---

### Phase 2: Breadth — [Detail Plan](./13-phase2-breadth.md)

Add remaining core data sources and individual-level views.

- Jira source
- Discourse source(s) — each instance as a separate source
- DORA metrics (lead time proxy from available data)
- Individual profile view
- Flow metrics (cycle time, WIP, throughput trends)

**Exit criteria:** Multiple data sources feeding metrics. Individual and team views with flow metrics across GitHub, Jira, and Discourse.

---

### Phase 3: Intelligence — [Detail Plan](./14-phase3-intelligence.md)

Layer AI capabilities over the metrics foundation.

- AI enrichment (review depth analysis, sentiment, contribution categorisation)
- Embeddings generation and similarity search (pgvector)
- Agentic query interface (natural language questions about team performance)

**Exit criteria:** AI-enriched metrics with full traceability. Users can ask questions and get sourced answers.

---

### Phase 4: Scale & Depth — [Detail Plan](./15-phase4-scale-depth.md)

Periodic autonomous insights, additional sources, and cross-platform correlation.

- Periodic insight generation (scheduled AI analysis producing actionable summaries)
- Launchpad source (merge proposals, bug tasks — see spike)
- Mailing list source (Pipermail/mbox — parser reference in `~/code/newsagent`)
- Google Drive source (document authorship tracking)
- Cross-platform correlation (linking activity across sources for holistic view)

**Exit criteria:** The system proactively surfaces insights, covers all planned data sources, and correlates activity across platforms.

---

## Cross-Cutting Concerns

These apply across all phases and should be addressed incrementally:

| Concern | Approach |
|---------|----------|
| **Authentication** | Single admin user via first-run wizard in Phase 1; session tokens on all RPCs. Multi-user, RBAC, OIDC/SSO expandable later. See [07-authentication.md](./07-authentication.md) |
| **Traceability** | Every metric and insight links back to source data from Phase 1 onward |
| **Identity resolution** | Manual mapping in Phase 1, automated suggestions in later phases |
| **Testing** | Integration tests against real DB (sqlx test fixtures), e2e tests per phase |
| **Observability** | Structured logging from Phase 1, metrics/tracing added as system grows |
| **Security** | Secrets stored encrypted (AES-256-GCM) in `config.secrets`; only `PS_SECRET_KEY` env var. Argon2id password hashing, hashed session tokens in DB |
| **Deployment** | Kubernetes (Docker Desktop K8s for dev, production TBD), Envoy Gateway for routing, Ubuntu + Chisel containers, ps-migrate init container |
