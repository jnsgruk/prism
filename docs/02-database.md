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
- sqlx offline mode: after changing any `query!` macro or migration, run `cargo sqlx prepare --workspace` and commit the `.sqlx/` directory. CI builds with `SQLX_OFFLINE=true`.
- Always use type-safe query macros (`sqlx::query!`, `sqlx::query_as!`, `sqlx::query_scalar!`) — never the runtime `sqlx::query()` string-based function

## Encrypted Secrets

Source credentials (API tokens) are stored encrypted in `config.secrets` using AES-256-GCM. Only `PS_SECRET_KEY` (256-bit, base64-encoded) comes from environment. All other configuration is managed through the admin UI via gRPC.

The `GetSource` RPC never returns secret values — only a boolean indicating whether each secret is set.
