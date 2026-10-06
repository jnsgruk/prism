# Database Design

PostgreSQL with pgvector. Six schemas act as bounded contexts, each owned by a dedicated repository struct.

## Schemas

| Schema | Purpose | Repo |
| --- | --- | --- |
| `auth` | Users, sessions, API tokens | `AuthRepo` |
| `config` | Source configs, encrypted secrets, global settings | `ConfigRepo` |
| `org` | People, teams, platform identities, team memberships, repositories | `OrgRepo` |
| `activity` | Contributions, ingestion watermarks, ingestion runs, ETag cache | `ActivityRepo` |
| `metrics` | Pre-computed team and individual snapshots | `MetricsRepo` |
| `reasoning` | AI enrichments, embeddings, conversations, model/catalogue state, insight snapshots | `ReasoningRepo`, `InsightsRepo` |

`InsightsRepo` is a read-model over `reasoning` data for query ergonomics — it does not imply a separate schema.

## Repository Pattern

All database access is centralised in `ps-core/src/repo/`. One Repo struct per schema.

The `Repos` struct bundles all repos and is constructed once from a `PgPool` in `main.rs`, then cloned into each service and handler.

**Layering rules:**

1. All `sqlx::query!` calls live in `ps-core/src/repo/` — services, ingestion sources, and other crates never contain direct SQL
2. Services are thin gRPC adapters — they receive `Repos`, delegate to repo methods, and map between domain types and proto types
3. One repo per schema — cross-schema joins are permitted only as read-only queries within the primary consumer repo
4. No `PgPool` in services or sources — only `main.rs` and the repo layer touch `PgPool`

## Directory team matching

Directory import resolves all people before assigning teams, so file ordering
cannot hide an existing manager identity. Team and squad assignments first match
teams led by that person within the same organization, including renamed teams
and directory depth changes. Generated directory names become aliases for the
existing UUID for membership and hierarchy wiring. Existing active memberships
and manual team choices remain protected. Name matching and creation are
fallbacks when no led team exists. If a person leads multiple teams, an exact
name match disambiguates; otherwise new members remain unassigned with a warning
rather than creating another team or guessing.

## Team deletion

Teams with no active memberships and no child teams can be permanently deleted.
`OrgRepo::delete_team` locks the team row and validates within one transaction,
then removes memberships whose end date is today or earlier. The membership
foreign key remains restrictive so active membership rows cannot cascade.
Migration 0048 cascades metric snapshots (and their source links) and clears
repository team assignments with `ON DELETE SET NULL`. Insight snapshots and
GitHub team mappings already cascade. People, repositories, contributions and
individual metrics remain intact. Deletion removes the team's historical
membership attribution and derived team snapshots; the confirmation dialog
makes that consequence explicit.

## Domain Enums

Domain concepts (platform, contribution type, state, ingestion status, period type, role) use Rust enums stored as `TEXT` in PostgreSQL. The `impl_sqlx_text!` macro bridges sqlx encode/decode. No custom Postgres type migrations needed — the Rust compiler enforces valid values.

Enums live in `ps-core/src/models/enums.rs`. Use `.parse::<Platform>()` idiomatically. Never use string literals like `"github"` or `"merged"`.

## pgvector

The `vector` extension powers embedding storage and similarity search. Embeddings are stored in the `reasoning` schema with IVFFlat indexes for approximate nearest-neighbour queries.

## Migration Strategy

- Migrations live in `migrations/` as sequential numbered SQL files
- The `ps-migrate` binary runs as a K8s init container — the application binary never runs migrations
- sqlx offline mode: after changing any `query!` macro or migration, run `cargo sqlx prepare --workspace` and commit the `.sqlx/` directory. `mise run generate:sqlx` includes all targets so typed SQL in integration fixtures also builds offline. CI builds with `SQLX_OFFLINE=true`.
- Always use type-safe query macros (`sqlx::query!`, `sqlx::query_as!`, `sqlx::query_scalar!`) — never the runtime `sqlx::query()` string-based function

Pipeline invocation ownership survives deletion of run history. The optional
`activity.pipeline_invocations.run_id` foreign key uses `ON DELETE SET NULL`,
so ResetData can delete ingestion runs while retaining exact invocation IDs and
parent relationships needed for cancellation and recovery.

`activity.contribution_changes` stores person-import provenance separately from
replaceable contribution metadata. Each changed natural key retains its existing
contribution UUID and records source, pipeline and ingestion-run IDs, previous
and current attribution/timestamps/metric inputs, and affected UTC weeks, months
and quarters. A content hash makes replay recording idempotent. Deleting run
history nulls its run reference; resetting contribution data cascades its change
records. Scoped contribution upserts, eligible enrichment/embedding queues and
change records share a transaction, with saved identity/person ownership and
pipeline cancellation revalidated under row locks. Advisory natural-key locks
also cover ordinary contribution writers. Only provenance-backed Discourse-like
operations can correct `created_at`; ordinary upserts retain authoritative time
and action evidence.

## Historical invalidations and account discovery coverage

`activity.snapshot_invalidations` expands each durable contribution change into
unique UTC week/month/quarter periods, with independent raw-metric and insight
refresh timestamps. Its foreign key cascades with the change record. Migration
0044 also creates dirty work for already persisted change manifests. Workers
acknowledge only the selected UUID generations after successful recomputation;
new concurrent changes remain dirty. Insight work waits for queued enrichment,
and recovery selects terminal pipeline owners so cancellation or partial failure
cannot discard committed work. Team and insight snapshot calculations preserve
membership effective dates and replace obsolete calculation source links.

Insight snapshot refreshes acquire a database-wide transaction advisory lock
before their existing period lock, so current-period, owned historical and
recovery handlers share one aggregation budget across worker replicas. They
process one team at a time, retaining the five independent aggregation calls
within that team. Review quality fetches depth and sentiment sequentially;
coverage also has sequential subqueries. Source collection and snapshot writes
follow aggregation. Waiting refreshes use nonblocking lock attempts and return
their connections between attempts; rollback, cancellation and connection loss
release the lock. Raw metric refreshes retain their independent period locks.
This trades inter-team/inter-period throughput for predictable memory demand;
it does not throttle interactive insight RPCs or unrelated database work.

The 2026-10-01 backfill failure was PostgreSQL POSIX dynamic shared memory
exhaustion, not persistent disk capacity. The database log identifies sentiment
aggregation as failing; review-depth aggregation also uses the same parallel
hash shape. For April–June 2026 and the Ubuntu Engineering team, measured plans
used two workers and a parallel hash over approximately 36,525 period-wide
contributions (257-byte estimated row width), consuming 10–11 MiB before team
membership filtering. The depth-by-significance plan used another approximately
4.4 MiB parallel hash. Coverage used private hashes, including approximately
6 MiB over enrichment IDs. Existing person/date and enrichment indexes remain
available; these broad period joins are valid planner choices, not evidence
that an index is missing.

Previously four teams fanned out into up to 20 aggregation calls per refresh.
The default worker pool admitted ten connections, with one occupied by the
period lock; different periods could overlap. `work_mem=4MB` is a per-operation
budget, not a query/session limit. `hash_mem_multiplier=2` and parallel
participants multiply hash budgets, and several hashes/queries may coexist.
The new refresh uses two lock connections and at most five aggregation queries;
the Kubernetes memory allocation provides headroom for PostgreSQL workers and
other callers. See [Infrastructure](06-infrastructure.md#postgresql-memory).

Validation used an isolated `pgvector/pgvector:pg17` container with copied team,
membership, contribution and enrichment tables (353,353 contributions and
88,565 enrichments). Nine concurrent review-depth/sentiment queries reproduced
2/4 MiB shared-segment resize failures with `/dev/shm=64MiB`. With a 256 MiB
mount and 1 GiB container limit, the same probe succeeded, sampling about
109.5 MiB peak shared-memory use and no OOM events. A representative five-query
group succeeded even on 64 MiB (about 33.6 MiB sampled peak). These samples are
workload-specific lower bounds on peaks, not guarantees for arbitrary growth.
The updated computation also completed overlapping week and quarter requests
for all 45 teams in the isolated copy (90 snapshots in about 15–18 seconds).
The sampled repeat peaked at about 21 MiB shared memory and 675 MiB total
cgroup memory, with zero OOM events. Total usage included file cache, so it
should not be interpreted as an irreducible private-memory requirement.
Forced generic prepared plans for depth/sentiment chose serial nested-loop
joins in this copy (about 0.7–1.1 seconds); custom plans used parallel hashes.
The full refresh used the normal SQLx prepared-statement cache. Plan selection
and slow-query timings therefore depend on parameters, cache history and load.
Disabling parallel hash reduced shared-memory use, with a small latency cost
in that probe, but is not imposed globally. The query SQL and metric semantics
are unchanged; revisit membership-first/materialized input plans with measured
results if broader periods or larger datasets outgrow this budget.

`activity.identity_discovery_coverage` keys supplementary GitHub/Discourse
coverage by `(source_id, identity_id)`. Resetting activity also clears these
checkpoints so previously deleted history is not treated as harvested. A content
version binds the saved account and source settings. Edits invalidate that version without deleting contribution
history or unrelated checkpoints. Frozen upper cutoffs advance only after a
complete account traversal and successful terminal storage; source-wide update
watermarks cannot establish this coverage. Saved inactive accounts cannot
publish a new target checkpoint. First runs use an explicit bounded lookback,
while administrator backfills provide older history. Journalled planning stores
`initial_since` before supplementary fetching; `covered_through` stays NULL until
a complete stored traversal. Failed first attempts retain that initial lower
bound across later invocations, even after the lookback window would have moved
past deferred events. Bulk baseline creation validates active saved accounts and
exact source/platform bounds. Account/policy changes reset the baseline and
completed cutoff together.

`reasoning.enrichments.source_content_hash` binds queue-generated AI output to
its structured source input. Queue IDs and hashes are validated under
contribution/queue locks before persistence; queue cleanup requires matching
hashes for every applicable enrichment type. Migration 0047 leaves unknown
legacy provenance NULL so it cannot acknowledge replacement input.

## Encrypted Secrets

Source credentials (API tokens) are stored encrypted in `config.secrets` using AES-256-GCM. Only `PS_SECRET_KEY` (256-bit, base64-encoded) comes from environment. All other configuration is managed through the admin UI via gRPC.

The `GetSource` RPC never returns secret values — only a boolean indicating whether each secret is set.

## Manual people and account ownership

Administrator-managed people are created by `OrgRepo::create_person` in one transaction with optional account rows and an optional membership. `membership_management` is a typed `Management` TEXT value (`imported` or `manual`): a manual person records an intentional team choice, including no team. Existing rows default to imported. New manual people have no directory ID and `last_import_at` stays NULL until directory reconciliation; they are excluded from stale-import detection. Once reconciled, their existing import lifecycle applies, while their manual membership choice remains protected.

Accounts expose a stable identity row UUID and the optional opaque `platform_user_id`. Explicit add/update/remove operations check the person owns the row; omissions preserve existing values, and account-ID clearing is a separate operation. Jira requires an account ID for manual writes. Usernames are normalized to lowercase; Jira IDs retain their exact case. Discourse uses `discourse-<instance>` in the existing platform column. Account edits affect future resolution and leave previously attributed contributions intact.

Database constraints enforce case-insensitive `(platform, username)` uniqueness for username-based platforms and legacy Jira rows without account IDs. Jira Cloud accounts with IDs are owned exclusively by their opaque account ID; display names may repeat across accounts and people. Username lookup returns no owner when a label matches multiple people. An ownership trigger prevents reassignment, including concurrent writes. Migration 0038 checks for collisions before normalizing existing usernames and adding indexes; existing duplicates require an administrator to repair ownership and retry. Migration 0039 replaces the global username constraint with partial indexes while retaining Jira account-ID uniqueness. Jira CSV imports promote compatible legacy rows in place and upsert Cloud accounts by account ID. Identity `management` records manual provenance, and per-platform manual resolution status also protects account removal from in-flight automated lookup. Directory, Jira CSV, and automated resolution writers preserve these choices and return warnings or conflicts instead of taking ownership.

Directory import matches a stable directory ID first, then exactly one case-insensitive, trimmed email match with no conflicting directory ID. It attaches a new directory ID to that same UUID. Ambiguous matches are skipped with warnings; unmatched rows sharing a manual person’s name or manually owned account are also skipped for explicit repair. Portable org exports add person IDs, directory/import metadata, account IDs, management fields, and manual platform-resolution markers (including removed accounts) with backwards-compatible defaults. Merge snapshots existing manual platform choices before processing incoming accounts, so multiple accounts in the same batch do not block one another. Replace recreates exported person UUIDs verbatim, including distinct people sharing an email; legacy exports retain their reconciliation defaults. Full database backups preserve these columns and ownership constraints.

Portable merge prefers a matching UUID or directory ID over other people sharing
the same email. Contradictory UUID/directory matches, or a supplied email belonging
only to a different person, remain ambiguous and are skipped before applying
accounts or memberships.
