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

## Encrypted Secrets

Source credentials (API tokens) are stored encrypted in `config.secrets` using AES-256-GCM. Only `PS_SECRET_KEY` (256-bit, base64-encoded) comes from environment. All other configuration is managed through the admin UI via gRPC.

The `GetSource` RPC never returns secret values — only a boolean indicating whether each secret is set.

## Manual people and account ownership

Administrator-managed people are created by `OrgRepo::create_person` in one transaction with optional account rows and an optional membership. `membership_management` is a typed `Management` TEXT value (`imported` or `manual`): a manual person records an intentional team choice, including no team. Existing rows default to imported. New manual people have no directory ID and `last_import_at` stays NULL until directory reconciliation; they are excluded from stale-import detection. Once reconciled, their existing import lifecycle applies, while their manual membership choice remains protected.

Accounts expose a stable identity row UUID and the optional opaque `platform_user_id`. Explicit add/update/remove operations check the person owns the row; omissions preserve existing values, and account-ID clearing is a separate operation. Jira requires an account ID for manual writes. Usernames are normalized to lowercase; Jira IDs retain their exact case. Discourse uses `discourse-<instance>` in the existing platform column. Account edits affect future resolution and leave previously attributed contributions intact.

Database constraints enforce case-insensitive `(platform, username)` uniqueness for username-based platforms and legacy Jira rows without account IDs. Jira Cloud accounts with IDs are owned exclusively by their opaque account ID; display names may repeat across accounts and people. Username lookup returns no owner when a label matches multiple people. An ownership trigger prevents reassignment, including concurrent writes. Migration 0038 checks for collisions before normalizing existing usernames and adding indexes; existing duplicates require an administrator to repair ownership and retry. Migration 0039 replaces the global username constraint with partial indexes while retaining Jira account-ID uniqueness. Jira CSV imports promote compatible legacy rows in place and upsert Cloud accounts by account ID. Identity `management` records manual provenance, and per-platform manual resolution status also protects account removal from in-flight automated lookup. Directory, Jira CSV, and automated resolution writers preserve these choices and return warnings or conflicts instead of taking ownership.

Directory import matches a stable directory ID first, then exactly one case-insensitive, trimmed email match with no conflicting directory ID. It attaches a new directory ID to that same UUID. Ambiguous matches are skipped with warnings; unmatched rows sharing a manual person’s name or manually owned account are also skipped for explicit repair. Portable org exports add person IDs, directory/import metadata, account IDs, management fields, and manual platform-resolution markers (including removed accounts) with backwards-compatible defaults. Merge snapshots existing manual platform choices before processing incoming accounts, so multiple accounts in the same batch do not block one another. Replace recreates exported person UUIDs verbatim, including distinct people sharing an email; legacy exports retain their reconciliation defaults. Full database backups preserve these columns and ownership constraints.
