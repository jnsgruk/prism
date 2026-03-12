# Open Questions & Decisions

Decisions that need input before or during implementation.

## 1. Restate vs Temporal — RESOLVED (Restate confirmed)

**Decision:** **Restate** is confirmed as the orchestration engine. The spike (see [evaluation](~/code/canonical/temporal-restate-spike/evaluation.md)) scored Restate 3.9 vs Temporal 3.1 across six dimensions. Key factors:

- **Developer experience (4.0 vs 2.2):** Restate's Rust SDK is clean and first-class. Temporal's Rust SDK is alpha-quality (0.1.0-alpha.1) with numerous ergonomic issues.
- **Operational overhead (4.0 vs 3.0):** Restate is a single binary with embedded RocksDB — 3 containers total. Temporal needs 4-5 containers and its own PostgreSQL schemas.
- **Architectural fit (4.6 vs 3.4):** Restate's single-node model is ideal for our Canonical K8s setup. No schema conflicts with our application PostgreSQL.

**Trade-offs accepted:** No built-in UI (mitigated by our own ingestion status page + Restate admin API), pre-1.0 SDK (0.9, core API surface is stable), separate backup story for RocksDB state (acceptable since watermarks are in our PostgreSQL).

**Patterns to carry forward:**
1. `IngestionJob` trait — orchestrator-agnostic abstraction layer
2. Virtual objects keyed by source name — per-source concurrency control
3. Durable side effects with `ctx.run()` — named, retriable steps (plan, fetch, store, advance)
4. Durable sleep for rate limits — `ctx.sleep()`
5. Watermarks in PostgreSQL, not Restate state — queryability and auditability

---

## 2. Contributions: Single Table vs Table-per-Type — RESOLVED (Single table + typed Rust layer)

**Decision:** Single table with discriminator + JSONB at the database level, but with **strongly typed Rust structs per contribution type** in the application layer. The `metrics` and `metadata` JSONB columns are serialized/deserialized through typed enums, so the Rust code gets full type safety even though the DB schema is flexible.

```rust
// Typed contribution variants — JSONB is deserialized into these
enum ContributionData {
    PullRequest(PullRequestMetrics),
    CodeReview(CodeReviewMetrics),
    JiraTicket(JiraTicketMetrics),
    DiscoursePost(DiscoursePostMetrics),
    // ...
}

// Each variant has its own strongly typed struct
struct PullRequestMetrics {
    lines_added: i32,
    lines_removed: i32,
    time_to_merge_hours: Option<f64>,
    reviewer_count: i32,
}
```

This gives us: flexible schema at DB level (easy to add sources), strong types in Rust (compile-time safety), and the ability to add typed views or materialised views later if query performance on specific fields becomes a concern.

---

## 3. How to Handle DORA Metrics Without Deployment Data — RESOLVED (Skip for now)

**Decision:** Skip deployment-dependent DORA metrics (deployment frequency, change failure rate) for now. Focus on lead time, review turnaround, and flow metrics which we can compute from PR/code data. Deployment metrics can be added later if we integrate with CI/CD systems.

---

## 4. Frontend ↔ Backend Protocol Details — RESOLVED (Connect + Buf)

**Decision:**
- **`buf` CLI** for proto management: linting, breaking change detection, code generation
- **Connect protocol** (from Buf) over HTTP — `@connectrpc/connect-web` on the frontend
- Connect supports server streaming for the ingestion status page

---

## 5. Launchpad API Viability — PARTIALLY RESOLVED (Phase 4, research complete)

**Decision:** Launchpad is a **Phase 4** source. A research spike has been completed to evaluate the Launchpad API capabilities and determine what data maps to DORA/flow metrics. See [10-spike-launchpad-api.md](./10-spike-launchpad-api.md) for findings.

---

## 6. Google Drive Integration Scope — DEFERRED

**Decision:** Ignore Google Drive for now. The eventual goal would likely be tracking documents of specific types authored, but the exact signals and scope are unclear. Revisit when other sources are stable and there's a clearer picture of what Drive data would add.

---

## 7. Mailing List Archive Format — RESOLVED (Pipermail mbox)

**Decision:** Ubuntu mailing lists use **Pipermail archives**. Use the approach from `~/code/newsagent` (`src/tools/mailing_list.rs`) as the reference implementation.

The newsagent tool:
- Downloads gzip-compressed monthly mbox files from `https://lists.ubuntu.com/archives/{list_name}/{YYYY-Month}.txt.gz`
- Parses mbox format using the `mail-parser` crate
- Handles threading via In-Reply-To, References headers, and normalised subject fallback
- Deduplicates threads across multiple lists using Message-IDs

This gives us the archive format (Pipermail/mbox over HTTP) and a working reference for the parser. The Prism source adapter can follow the same data access pattern.

---

## 8. Identity Resolution Confidence — RESOLVED (Current plan, revisit later)

**Decision:** Go with the current plan: store contributions with `person_id = NULL` when identity can't be resolved, surface unresolved identities in the admin UI for manual mapping. Username changes are handled by allowing multiple `platform_identities` per person.

This is sufficient for Phase 1. More sophisticated identity resolution (fuzzy matching, automatic suggestions, confidence scoring) can be addressed in a later phase once we see the actual volume and patterns of unresolved identities.

---

## 9. Multi-Discourse Instance Handling — RESOLVED (Separate sources, tagged metrics)

**Decision:** Each Discourse instance is a **separate source** in `config.source_configs`, with its own watermark, schedule, and API credentials. Metrics distinguish which instance they came from.

- People are **not necessarily** active across all instances, though there's often correlation within teams.
- The `platform` field on contributions already captures this — e.g. `discourse-ubuntu`, `discourse-snapcraft` — so metrics naturally group by instance.
- Cross-instance activity for the same person is handled by the existing identity resolution (multiple `platform_identities` per person, one per Discourse instance).

---

## 10. Implementation Ordering — RESOLVED

### Phase 1: Foundation
- Project scaffolding (Rust workspace, Next.js app, PostgreSQL, proto definitions)
- Org context (people, teams, directory import)
- GitHub source (most mature from contristat, highest signal)
- Basic metrics computation (PR throughput, review turnaround)
- Team comparison view in frontend
- Ingestion status page

### Phase 2: Breadth
- Jira source
- Discourse source(s)
- DORA metrics (or proxy)
- Individual profile view
- Flow metrics

### Phase 3: Intelligence
- AI enrichment (review depth, sentiment)
- Embeddings + similarity
- Agentic query interface

### Phase 4: Scale & Depth
- Periodic insight generation
- Launchpad source
- Mailing list source (Pipermail/mbox)
- Google Drive source
- Cross-platform correlation

---

## 11. GitHub API: REST vs GraphQL — RESOLVED

**Decision:** Use **both REST and GraphQL**, leveraging each where it's strongest. GraphQL is the primary interface for bulk data fetching (PRs with reviews and comments in a single query). REST is used for endpoints without GraphQL equivalents (Events API for change radar) and as a fallback if GraphQL rate limits are exhausted.

Both APIs share the same authentication but have separate rate limit tracking. Plan for hitting limits early — use GitHub App tokens from day one.

---

## 12. CLI vs UI for Operations — RESOLVED

**Decision:** **UI-first, with `psctl` CLI.** All operations — directory import, config changes, manual ingestion triggers — are driven through the gRPC API. The admin UI is the primary user-facing surface. `psctl` is a thin gRPC client wrapper (the `ps-cli` crate) that provides CLI access to the same API for scripting and automation. It authenticates via API tokens (see [07-authentication.md](./07-authentication.md#api-tokens-for-psctl-and-automation)).

---

## 13. Authentication & Authorisation — RESOLVED

**Decision:** Lightweight auth from Phase 1. Single admin user created via a first-run setup wizard in the UI. See [07-authentication.md](./07-authentication.md) for full design.

**Key decisions:**
- **Argon2id** password hashing (RustCrypto `argon2` crate, pure Rust)
- **Opaque session tokens** (256-bit random, SHA-256 hashed in DB), sent as `Authorization: Bearer <token>` — not cookies
- **Async tonic interceptor** (`tonic-middleware`) validates every RPC; only `GetSetupStatus`, `CompleteSetup`, and `Login` are public
- **`auth` schema** with `users` and `sessions` tables — structured for multi-user/RBAC/OIDC from day one, but only one admin user created initially
- **No user management UI** for now — just the first-run wizard and login page
- **First-run detection:** `GetSetupStatus` checks if any admin user exists; if not, redirects to `/setup`

**Future expansion path (no rework needed):**
- Multiple users → already supported by schema
- Role-based access → `role` column on `auth.users`, policy checks per RPC
- OIDC/SSO → add `auth_provider` + `external_id` columns, swap login verification; `openidconnect` crate
- MFA/Passkeys → add `auth.mfa_credentials` table, insert challenge step
- API keys → same sessions table with no expiry, or dedicated table

---

## 14. Configuration Management — RESOLVED

**Decision:** Configuration lives in the database (`config` schema), modifiable at runtime from the UI. No config files in the loop during normal operation.

- **Secrets:** stored encrypted in the `config.secrets` table using AES-256-GCM. The only env var is `PS_SECRET_KEY` (the encryption key). All other credentials are managed through the admin UI via `ConfigService.SetSecret`.
- **Bootstrap:** a CLI import command (`ps-server config import --file`) can seed initial config from a TOML file on first run.
- **Directory import:** managed from the admin UI (upload or path reference in source config).
