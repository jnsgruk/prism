# Decision Log

Significant architectural decisions in reverse chronological order. Each entry records what was decided and why, so future contributors understand the reasoning without needing to read historical implementation plans.

---

## 2026-10-01 — Parallel CI jobs with Rust dependency caching

**Context:** The combined lint-and-test job took roughly 9–12 minutes in sampled
runs. Serial Clippy and Rust test compilation dominated the wait, while setup
installed development tools that CI did not use.

**Decision:** Run Rust checks, Rust tests, frontend checks/tests and protobuf
validation as four independent jobs. Install only the tools each job needs and
cache Cargo downloads and compiled dependencies separately for each Rust job.
Keep the full Rust test suite together and retain the local prek gate.

**Rationale:** This removes Clippy from the test job's critical path and reuses
compiled dependencies on warm runs, with modest workflow complexity. Separate
Rust jobs duplicate some compilation on cold runs and increase runner usage;
actual elapsed-time savings must be measured after cache population.

---

## 2026-10-01 — Gate agent sessions on application health and reconcile ambiguous creation

**Context:** Agent startup sent session POST as soon as Kubernetes reported
`Running`. One request consumed the SDK's full 120-second operation timeout;
its retry succeeded in 499 ms. An isolated pod reproduced refused / timed-out
HTTP requests after `Running` and before health succeeded. Session lookup errors
also allowed replacement of potentially live sessions.

**Decision:** Check OpenCode's `/global/health` with short connection/request
deadlines, share a bounded startup budget across preparation and connection,
retry only reads, and persist pod-UID-scoped creation intent before one POST.
Reconcile ambiguous creation by a stable conversation title. Preserve ordinary
operation / SSE budgets, streaming in ps-server and Restate journal ordering.

**Rationale:** Readiness polling handles application and transient network gaps
without paying a full operation timeout. Persisted intent prevents follow-up
requests from creating duplicates after an unknown POST outcome, including
server restart or cancellation. Without a supported idempotency key, intent
that produced no session must remain reconciliation-only until the pod is
replaced. A new pod UID safely permits recovery after expiry. The original
connection stall's packet-level cause remains unproven; health and stage timing
logs improve diagnosis without redesigning or modifying the live deployment.

---

## 2026-10-01 — Render workspace PDFs with React-PDF and PDF.js

**Context:** The Ask workspace sidebar did not render PDFs, and an expanded
sandboxed iframe showed a broken-document view in Chrome for a reported PDF.
The browser viewer/sandbox was a suspected contributor; its role was not
verified as the root cause.

**Decision:** Use a lazy feature-local React-PDF renderer in both the sidebar
and expanded Base UI dialog. Bundle the matching PDF.js worker and supporting
assets locally. Own document/page state and Blob lifetime in a feature-local
hook, reuse the authenticated streaming download, and render only the selected
page on the visible surface.

**Rationale:** Application-owned rendering provides consistent navigation,
zoom, selectable text, loading/error states and downloadable fallbacks without
depending on a browser PDF plugin. A single-page viewer bounds rendering work;
unmounting the hidden surface avoids duplicate canvases. Local worker/assets
keep production delivery independent of external CDNs. Password entry and
interactive PDF features remain outside V1.

---

## 2026-10-01 — Verify Ask file references and use conversation download routes

**Context:** An existing Ask answer linked a successfully generated PDF using
`/workspace/<filename>`. The browser received the SPA shell and displayed page
not found. The agent's filesystem path had been mistaken for a browser URL.

**Decision:** Keep `/workspace/<path>` as the agent contract, resolve file
metadata through an authenticated additive RPC, and rewrite verified file links
to `/ask/<conversation-id>/files/<path encoded by segment>`. The browser page
uses the existing authenticated streaming download RPC. Decode URL segments once
at the boundary and pass decoded relative paths to filesystem APIs. Validate
completed answers before storing/emitting them and repair historical responses
at render time, without changing stored history.

**Rationale:** Stable conversation routes support reload/new tabs and retain
session authentication without a public filesystem server or persisted blob
URLs. Metadata checks avoid downloading files during rendering. Revalidation
at transfer time handles later deletion; persistent PVC storage survives pod
expiry. Existing shared-conversation reads remain available to signed-in users,
and deletion revokes access even before storage cleanup finishes. No schema
migration or global filename lookup is needed.

---

## 2026-10-01 — Budget PostgreSQL shared memory and serialize insight refreshes

**Context:** A completed person ingestion/enrichment pipeline repeatedly failed
in `HistoricalSnapshotService/refresh_batch` at
`refresh_insights_quarter_2026-04-01`. PostgreSQL parallel hashes exhausted the
container's 64 MiB `/dev/shm` while the pod had only a 512 MiB total memory limit.
Four-team aggregation fan-out and overlapping periods amplified demand. A
temporary parallel-worker override completed the backfill and was then reset.
An isolated copy of the relevant tables reproduced the resize failure.

**Decision:** Mount a 256 MiB memory-backed `emptyDir` at `/dev/shm`, increase
the PostgreSQL memory request/limit to 512 MiB/1 GiB, and serialize insight
snapshot refreshes across periods and worker replicas using a transaction
advisory lock. Process one team at a time with its five independent aggregation
calls concurrent. Preserve parallel queries, existing period locks, query SQL,
provenance and the separate committed ingestion timeout policy.

**Rationale:** A larger mount fixes the runtime bottleneck; a larger container
limit accounts for its memory cost. Database-wide concurrency control prevents
additional refreshes or worker replicas from multiplying that cost. It trades
refresh throughput for headroom without disabling parallelism for all callers.
The enlarged-mount nine-query probe completed with no OOM events; a five-query
probe also succeeded at the old mount size. These observations do not guarantee
unbounded growth or interactive concurrency will fit. Query-specific serial
execution, disabling parallel hash, or membership-first rewrites remain tuning
options if measurements justify them. Increasing shared memory alone would
leave the original OOM risk and unbounded refresh overlap unresolved.

---

## 2026-10-01 — Allow ingestion API work to finish before requesting suspension

**Context:** A live person backfill repeatedly suspended under the cluster's
five-second inactivity timeout. Scoped reads outside `ctx.run()` re-fetched
committed pages on every replay, consuming API quota and making progress appear
to move backwards while only one new journal step completed per attempt.

**Decision:** Advertise a 15-minute inactivity timeout on `IngestionChunkService`
and use the same configured service definition in production and real Restate
tests. Restate 1.6 exposes this override at service level. Preserve the global
default for other handlers and the existing abort grace. Apply the same
timeout-only service patch to active deployments without changing journals.

**Rationale:** Provider reads, sequential diff requests and transient retries need
time between journal entries. Giving them a longer window removes avoidable
replay while preserving explicit durable sleep, retry and restart behavior.
Changing timeout metadata avoids interrupting the current run or journaling
provider bodies and secrets. Delayed-page integration coverage checks completion
and exactly one provider request per page under a short server default.

---

## 2026-10-01 — Bind shared enrichment writes to current queued inputs

**Context:** A shared AI batch can overlap a person backfill that changes its
captured input. Deleting existing results during ingestion alone cannot stop a
late old response from overwriting the new work and satisfying queue cleanup.

**Decision:** Capture queue IDs and source hashes, serialize result writes with
contribution and queue updates, and persist only matching results. Store source
hash provenance independently of the prompt hash and require it during type
selection and cleanup. Retain replacement work when stale responses arrive;
unknown legacy provenance does not satisfy newly queued work.

**Rationale:** Shared workers remain available while historical recovery can
trust that a drained queue represents results for current input. This avoids
holding database locks across provider calls and handles both bulk writes and
individual fallback retries consistently.

## 2026-10-01 — Enable person pipelines with durable historical refresh and account coverage

**Context:** Person adapters and atomic persistence were complete, but launches
remained gated. Current-period-only recomputation left historical results stale,
and team-only GitHub discovery/topic-only Discourse discovery missed new activity
for active manually added people.

**Decision:** Enable the admitted Person workflow, skip its platform-wide identity
resolution, and consume transactional dirty generations for old/new UTC
week/month/quarter snapshots. Preserve period-end membership attribution and
refresh calculation links. Recover terminal owners' dirty work through a durable
bounded singleton loop; leave unresolved enrichment work pending and visible.
Keep the existing shared AI queue executor and its exact pipeline cancellation
ownership. Reuse person adapters for supplementary normal discovery with
independent source/account checkpoints, frozen cutoffs, one-day overlap and
seven-day GitHub/30-day Discourse initial lookbacks. Identity/source-boundary
changes reset only affected coverage; full historical collection remains an
explicit backfill. Persist the first discovery lower bound separately from
completed coverage, so failed first attempts cannot lose deferred events as a
new invocation's lookback moves forward. Only complete stored traversals publish
target coverage.

**Rationale:** Contribution provenance and independent acknowledgements survive
crashes/cancellation without widening a person's historical import or inventing
membership. Independent account coverage finds reviews and old-topic likes that
source-wide update timestamps cannot prove complete. Existing adapters and queues
avoid another scheduler or duplicate platform implementation. Drain affected
journals before rollout, retain unrelated state, and validate with actual Restate
replay/cancellation plus real PostgreSQL/provider fixtures.

## 2026-10-01 — Publish GitHub coverage after all review pages finish

**Context:** A successful review continuation could advance the global update watermark while later pages were unfinished. A later failure discarded the cursor; fresh `updated:>` discovery then skipped the unchanged parent PR and permanently lost remaining reviews. Earlier repository pages can also have newer timestamps than subsequently discovered parents.

**Decision:** Keep GitHub's observed update maximum in the cursor for progress, but publish a separate completion watermark only after successful source collection, with no pending reviews or failed targets. Jira and Discourse keep their existing incremental coverage policy. Drain affected old GitHub invocations before deployment because watermark journal steps change.

**Rationale:** Source-wide coverage must describe completed collection across repositories and nested review pages. Restarting a failed run may refetch committed rows, which stable natural keys deduplicate, while Restate still resumes active chunks durably. This trades extra reads on fresh retries for complete recoverable history.

---

## 2026-10-01 — Target saved accounts and commit scoped provenance atomically

**Context:** Team/project/topic sweeps missed review-only and reply-only history and could widen a person's import. Discourse normal ingestion estimated likes at post creation time, making later historical corrections unauditable.

**Decision:** Discover each saved account directly within its configured source bounds, freeze event eligibility and bounded resumable cursors, and expose upstream visibility/cap failures in progress and outcomes. Enforce identity, attribution and cancellation again in an atomic repository transaction covering contributions, queues and a separate change manifest. Isolate person cursors from global watermarks. Authorize Discourse-like timestamp corrections only with real user-action evidence and retain old/new affected periods. Keep full Person pipeline admission gated until historical recomputation and ongoing tracking pass their separate release stages.

**Rationale:** Direct discovery finds cross-author context without crediting its participants, bounded checkpoints allow durable retries, write-time ownership prevents account races, and atomic queue/provenance persistence prevents false completion or lost processing. Separate manifests survive ordinary metadata refreshes and give historical recomputation exact inputs.

---

## 2026-10-01 — Reserve pipeline admission before durable dispatch

**Context:** Checking for an active workflow before dispatch allowed concurrent launches, and a lost Restate acknowledgement left the caller unsure whether work had started. Time-based run association could include unrelated scheduled work.

**Decision:** Reserve one globally admitted pipeline in PostgreSQL, persist the request and caller before dispatch, and use its UUID as a retry-safe Restate workflow key. Deliver through a leased recoverable outbox. Track exact invocation ownership for coordinators, chunks, processing, and detached continuations; expose cancellation as pending until owned work stops.

**Rationale:** Database uniqueness closes admission races, stable workflow keys prevent duplicate work after uncertain delivery, and explicit ownership isolates cancellation and history from concurrently scheduled jobs. Preserve partial writes for reruns and keep the Person release gate closed until adapter and processing safety is verified.

---

## 2026-10-01 — Freeze pipeline scope and preserve legacy workflow journals

**Context:** Ingestion accepted a date string and selected work by platform, which cannot safely identify one saved person across multiple configured sources or survive configuration changes during a durable run.

**Decision:** Share typed All/Person scope, saved source and identity snapshots, a frozen run boundary, and an explicit processing scope in `ps-core`. Accept only saved IDs from API callers. Reserve admission durably before dispatch and recover the same workflow UUID. Track explicit descendant ownership, using Restate ancestry for missing or legacy registry entries, and release admission only after owned work stops. Preserve the distinction between user cancellation and an unexpectedly failed root. Introduce versioned scoped workflow/object entrypoints while retaining legacy arguments and optional chunk defaults. Keep Person capability disabled until scoped adapters and processing exist; require Jira Cloud opaque account IDs and exact Discourse instances.

**Rationale:** Durable snapshots preserve the admitted ownership and selected configurations across retries. Separate discovery from event-time eligibility so old parent PRs do not hide newer reviews. Versioned entrypoints preserve existing Restate replay contracts, and the processing boundary prevents a narrow ingestion request from silently expanding to global work.

---

## 2026-10-01 — Jira ownership by account ID, with repeatable display labels

**Context:** Jira lookup returns nonunique display names. Saving them under the global username constraint rejected distinct accounts with identical names.

**Decision:** Keep Jira display names in the existing username field, but exempt accounts with opaque IDs from username uniqueness. Retain account-ID uniqueness and immutable ownership, preserve username uniqueness for legacy Jira rows without IDs, and promote those legacy rows during CSV import without changing their row UUID. Ambiguous username lookups do not select an owner.

**Rationale:** Cloud attribution already uses opaque IDs. Labels should not prevent valid account selection or become an ownership key; existing username-based platforms and legacy imports retain their safeguards.

---

## 2026-10-01 — Explicit manual person and account ownership

**Context:** Colleagues absent from directory exports need to be tracked without inventing membership, and existing import/resolution upserts could transfer accounts. Jira activity is attributed by opaque Cloud account IDs rather than display names.

**Decision:** Create people, accounts, and optional membership atomically. Store typed imported/manual management on membership choices and accounts, enforce immutable account ownership in PostgreSQL, and preserve manual account removal through per-platform manual resolution status. Reconcile directory IDs or unique compatible email matches to the same UUID. Manual people remain outside stale detection until they have actually been imported; reconciliation does not grant a permanent stale exemption. Account corrections leave historical attribution intact. Portable exports preserve identity IDs used for ingestion and management metadata with old-format defaults.

**Rationale:** Explicit choices prevent silent assignment or cross-person attribution while preserving existing membership history and imported-person lifecycle. Jira lookup supports Cloud only and requires explicit account selection; direct opaque ID entry remains available. Multi-tenant Jira identities, Server/Data Center adapters, person merging, historical reassignment, and person backfills remain separate work.

---

## 2026-09-02 — Reuse the Workspace Claim for Development Restate State

**Context:** Restate needs durable state across pod restarts, but the development cluster's rawfile CSI pool could not allocate another persistent volume. The existing 50 GiB `prism-workspaces` ReadWriteMany claim had sufficient headroom.

**Decision:** Mount `prism-workspaces` into the Restate StatefulSet at `/restate-data` with the dedicated `restate-data` subpath, and keep `RESTATE_BASE_DIR` at `/restate-data/store`.

**Rationale:**
- Restate state survives pod replacement without requiring another scarce development volume
- The subpath prevents Restate files from mixing with per-conversation workspace directories
- The tradeoff is shared capacity and a shared storage failure domain; production deployments should use a dedicated durable Restate volume

---

## 2026-09-02 — Codex-Native Repository Guidance

**Context:** Repository guidance used Claude Code-specific discovery paths (`CLAUDE.md`, `.claude/rules`, and `.claude/skills`). Codex does not natively discover those path-scoped rule files, and Claude-specific skill metadata and tool names are not portable.

**Decision:** Adopt Codex-native project configuration. Repository-wide guidance lives in `AGENTS.md`; scoped rules live in nested `AGENTS.md` files; shared project skills live under `.agents/skills`. Local Claude permission settings are not translated because Codex permissions are controlled by the active environment and trusted project configuration rather than a directly equivalent repository allow-list.

**Rationale:**
- Codex discovers the project conventions without user-level fallback configuration
- Nested instructions retain the previous path-specific behavior
- The review skill uses the portable agent-skills format and Codex collaboration tools
- Removing parallel Claude configuration prevents the two instruction sets from drifting

---

## 2026-06-18 — Directory Re-import as a Safe Merge

**Context:** HTML directory imports (`directory.html`) carry no `directory_id`, so the upsert path matched people only by `directory_id` and otherwise inserted unconditionally. Re-importing therefore created a fresh `org.people` row for every record on every upload — duplicating the entire org and migrating platform identities onto the duplicates via the `ON CONFLICT (platform, platform_username)` remap. The "safe re-import" guarantees in the code only ever applied to JSON imports that supply a `directory_id`. There was also no handling of leavers: stale people were merely counted, and only for `directory_id` rows (never set by HTML), so HTML imports always reported zero.

**Decision:** Make directory re-import a true merge. `upsert_person` matches an existing person by `directory_id` (JSON) or, failing that, by email (HTML, case-insensitive) before inserting, so re-imports update in place and only genuine new joiners are inserted. Leavers (active, import-managed people absent from the file) are detected via `last_import_at` and, when `deactivate_stale` is set, deactivated — guarded by a maximum stale fraction (20%) that skips deactivation on partial/truncated files.

**Key design choices:**
- **Email as the HTML match key:** the directory has no stable per-person id; email is unique-enough in practice and already present on every record. Duplicate emails (no DB constraint) resolve to the oldest row for determinism.
- **`last_import_at IS NOT NULL` = "import-managed":** distinguishes directory people from manually-added ones, so manual entries are never treated as leavers. The first import under this logic is a safe baseline (no deactivations) because no one is yet marked managed.
- **Opt-in, guarded deactivation:** `deactivate_stale` defaults off (stale people are only reported); the fraction guard prevents a truncated upload from mass-deactivating the org. Deactivation is reversible (sets `active = false`, ends memberships).
- **Manual structure preserved:** team memberships are only assigned when a person has none, and lead/parent wiring only fills NULLs.

**Rationale:**
- Fixes silent, catastrophic duplication on the most common import path
- Gives leaver handling that matches operator intent (add joiners, remove leavers) without clobbering manually-curated org structure
- The guard plus opt-in default keeps the destructive path safe by construction

---

## 2026-04-22 — Backup/Restore via `pg_dump`/`pg_restore` K8s Jobs

**Context:** The initial backup system used custom JSONL serialization (~2800 lines) with streaming, pagination, per-table SHA-256 checksums, and a Restate handler for process isolation. This approach had compounding reliability problems: CPU-bound JSON serialization starved the tokio runtime, blocking HTTP/2 PING/PONG and causing Restate keep-alive timeouts; journal sequences were fragile across code changes; and every schema migration required updating both export and import paths. Several iterations (heartbeat journaling, per-table journal chunking, `spawn_blocking` writers) improved reliability incrementally but added complexity without addressing the root cause: we were reimplementing what `pg_dump` already does.

**Decision:** Replace the entire custom backup pipeline with `pg_dump` and `pg_restore` running as Kubernetes Jobs via a new `ps-backup` container (`pgvector/pgvector:pg17` base, matching the DB server). The `BackupGenerator` trait abstracts K8s Job management (production) vs direct execution (tests). Drop v1 JSONL archive support entirely.

**Key design choices:**
- **K8s Jobs, not Restate handlers:** Jobs provide natural process isolation, automatic cleanup (TTL), and resource limits without the journal fragility that plagued the Restate approach
- **Schema drop before restore:** `DROP SCHEMA ... CASCADE` before `pg_restore` (instead of `pg_restore --clean`) avoids FK conflicts with excluded tables like `activity.ingestion_runs`
- **No backward compatibility:** v1 JSONL archives are rejected outright — the old code path is deleted, not maintained
- **Container image:** `pgvector/pgvector:pg17` base guarantees pg tool version alignment with the database

**Rationale:**
- Eliminates ~4300 net lines of serialization, streaming, journaling, and backup-only repo code
- Schema migrations no longer require backup code changes — `pg_dump` captures the schema automatically
- `pg_dump` custom format includes built-in checksums, making manual integrity verification unnecessary
- K8s Jobs provide process isolation (the original motivation for the Restate handler) without journal complexity
- The `BackupGenerator` trait pattern is simpler than the previous wiremock-based Restate dispatch mock

---

## 2026-04-15 — Fail Pipeline on Any Source Ingestion Failure

**Context:** A GitHub ingestion run processed 31,835 items across 17 chunks over 3+ hours, then chunk 18 hit persistent GitHub 502 errors. Two problems surfaced: (1) the chunk error propagated via `?` and skipped `finalise_run()`, leaving the run record orphaned in `running` status; (2) the pipeline continued into downstream stages (metrics, enrichment, embedding) because the previous behaviour only halted if *all* source handlers failed.

**Decision:** (1) Catch chunk failures in the coordinator and always call `finalise_run()` — if items were already stored the run completes with warnings, otherwise it fails cleanly. (2) Change the pipeline ingestion gate from "all failed" to "any failed" — any source failure halts the pipeline and cancels remaining stages.

**Rationale:**
- Downstream stages (enrichment, embedding, insights) process data from all sources together. Running them with incomplete data produces misleading metrics and wastes AI spend on contributions that will be re-processed when the failed source succeeds.
- Orphaned run records create confusing UI state and require manual cleanup. The invariant should be: `finalise_run()` is always called, regardless of how the chunk loop exits.
- The watermark system means re-triggering the pipeline after the transient failure is resolved will pick up where the failed source left off — no data is lost.

---

## 2026-04-10 — Journal Branching Decisions in fetch_store_loop for Restate Determinism

**Context:** `fetch_batch()` in the ingestion orchestration loop is deliberately not journaled (large API responses). However, its result determines whether the next Restate journal entry is a `ctx.sleep()` (rate limit) or a `store_batch` via `ctx.run()`. After a pod restart, Restate replays the journal but re-executes `fetch_batch()` against the live API — if the rate limit has reset, the fetch returns different results, producing a different journal sequence and triggering Restate error 570. This caused an infinite retry loop that burned through GitHub API quota every cycle.

**Decision:** Introduce a `BatchAction` enum that captures the branching decision (sleep vs process, with pre-computed sleep durations and cursor state). This is journaled via `journaled_value!` after each `fetch_batch()`. All downstream branching uses the journaled decision, making the journal sequence deterministic on replay regardless of what the API returns.

**Rationale:**
- Journaling the full fetch response is impractical (megabytes of PR data per batch)
- The `BatchAction` enum serializes to tens of bytes — negligible journal overhead
- On replay, `ctx.run()` closures for `store_batch`/`advance_watermark` return previously journaled results without re-executing, so it's safe that the batch data may differ on replay
- Pre-computing `wait_secs` avoids non-deterministic `now_utc()` calls during replay
- Applies to all three ingestion sources (GitHub, Jira, Discourse) via the shared `fetch_store_loop`

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
