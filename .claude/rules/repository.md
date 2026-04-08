---
paths:
  - "crates/ps-core/src/repo/**"
  - "migrations/**"
---

# Repository Layer Rules

All database access is centralised in `ps-core/src/repo/`. Each repo maps to one database schema (bounded context):

| Repo | Schema | Responsibility |
| --- | --- | --- |
| `AuthRepo` | `auth` | Users, sessions, API tokens |
| `ConfigRepo` | `config` | Source configs, encrypted secrets |
| `OrgRepo` | `org` | People, teams, platform identities, repositories |
| `ActivityRepo` | `activity` | Contributions, watermarks, ingestion runs, ETag cache |
| `MetricsRepo` | `metrics` | Pre-computed team/individual snapshots, contribution queries |
| `ReasoningRepo` | `reasoning` | Enrichments, embeddings, conversations, model/catalogue state |
| `InsightsRepo` | `reasoning` | Read-only insight queries and aggregation views |

`InsightsRepo` is a read-model over `reasoning` data for query ergonomics — not a separate schema.

## Layering Rules

1. **All `sqlx::query!` calls must live in `ps-core/src/repo/`** — services, ingestion sources, and other crates must never contain direct SQL.
2. **Services are thin gRPC adapters** — they receive `Repos`, delegate to repo methods, and map between domain types and proto types. Business logic that doesn't need proto types belongs in `ps-core`.
3. **One repo per schema** — cross-schema joins permitted only as read-only queries in the primary consumer repo.
4. **No `PgPool` in services or sources** — only `main.rs` and the repo layer touch `PgPool`.

## The `Repos` Bundle

The `Repos` struct bundles all repos. Constructed once from a `PgPool` in `main.rs`, then cloned into each service and handler.

## Domain Enums

Use `impl_sqlx_text!` for all domain enums. Stored as `TEXT` in PostgreSQL. The Rust compiler enforces valid values — no custom Postgres type migrations needed. Implement `FromStr`/`Display` via the macro.

## Encrypted Secrets

`config.secrets` values encrypted with AES-256-GCM. The `GetSource` RPC returns only booleans indicating whether each secret is set — never the secret value itself.

## Migrations

- Sequential numbered SQL files in `migrations/`
- Run by `ps-migrate` init container — the app binary **never** runs migrations
- After changing queries or migrations: `cargo sqlx prepare --workspace` and commit `.sqlx/`
- Always use type-safe query macros — never `sqlx::query()` string-based
