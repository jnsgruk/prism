# 34 — Directory Import as Ingestion Source

**Status:** Proposal
**Date:** 2026-03-16

## Context

The Canonical staff directory import currently lives in `ps-server` as a synchronous gRPC handler:

```
ps-server/src/directory.rs          — HTML parser (scraper-based)
ps-server/src/services/org/import.rs — format detection, team hierarchy derivation
ps-server/src/services/org/mod.rs    — OrgService::import_directory() handler
ps-core/src/repo/org/import.rs       — transactional 4-pass DB import
```

The flow today: frontend uploads a file → gRPC handler parses it inline → repo layer runs a multi-pass transaction → response with counts. Everything is synchronous and request-scoped.

Meanwhile, all other data ingestion (GitHub, and future Jira/Launchpad/Discourse) follows the Restate-orchestrated `Source` trait pattern in `ps-workers`:

```
plan() → fetch_batch() → store_batch() → advance_watermark()
```

The question: should the directory import be refactored to use the ingestion/Restate pattern?

## Analysis

### What the directory import shares with ingestion sources

| Aspect | Directory Import | GitHub Ingestion |
|--------|-----------------|------------------|
| External data → people + identities | Yes | Yes |
| Identity resolution (username → person_id) | Yes (at import time) | Yes (at store time) |
| Team creation/assignment | Yes (derived from hierarchy) | Yes (via team sync) |
| Idempotent re-import | Yes (upsert by directory_id/name) | Yes (upsert by platform_external_id) |
| Stale detection | Yes (last_import_at tracking) | No (watermark-based) |

### What is fundamentally different

| Aspect | Directory Import | Ingestion Sources |
|--------|-----------------|-------------------|
| **Data origin** | User-uploaded file (bytes in request) | External API (HTTP polling) |
| **Trigger** | Manual, on-demand | Scheduled or manual trigger |
| **Authentication** | None (file is the data) | API token (encrypted, decrypted at runtime) |
| **Volume** | ~hundreds of records, sub-second | Thousands–millions, minutes–hours |
| **Fetching** | No fetching — data is already present | Multi-batch cursor-based pagination |
| **Rate limiting** | N/A | Core concern (adaptive phase skipping) |
| **Watermark** | N/A (full-file replacement) | Essential for incremental ingestion |
| **Durability** | Single transaction, fast enough to be synchronous | Must survive restarts, needs checkpointing |
| **Output** | Org structure (people, teams, hierarchy) | Activity data (contributions) |

### The Source trait is a poor fit

The `Source` trait (`plan → fetch_batch → store_batch → advance_watermark`) is designed for **incremental polling of external APIs**. Every method assumes:

- There's an external service to call (`fetch_batch` makes HTTP requests)
- Data arrives in pages that need cursor-based iteration
- Progress must be checkpointed because runs take minutes/hours
- A watermark tracks "where we left off" for the next run

The directory import has **none of these characteristics**. The data is already fully present in the request body. There's nothing to fetch, no cursor to advance, no watermark to track. Forcing it into the Source trait would mean:

- `plan()` — returns a trivial "plan" with no repos
- `fetch_batch()` — returns the already-parsed records in one shot
- `store_batch()` — does the real work (but now separated from the enrichment)
- `advance_watermark()` — no-op

This is ceremony without value. The Restate durability guarantees solve a problem (long-running multi-batch ingestion surviving restarts) that doesn't exist here.

### Where there IS real overlap

The overlap isn't in the orchestration pattern — it's in the **domain logic**:

1. **HTML parsing** (`directory.rs`) — a reusable parser for a specific external format
2. **Team hierarchy derivation** (`import.rs`) — business logic for inferring org structure from nesting depth + manager relationships
3. **Identity mapping** — linking platform usernames to `org.identities`
4. **Person upsert** — creating/updating people in `org.people`

This is shared **domain logic**, not shared orchestration. The right home for it is `ps-core`, not `ps-workers`.

## Recommendation

**Don't move directory import into the Restate/Source pattern.** The abstraction mismatch would add complexity without benefit.

Instead, address the real structural issues:

### 1. Move the HTML parser to `ps-core`

`directory.rs` currently lives in `ps-server` (the API binary). It's pure parsing logic with no server dependencies — just `scraper`. Move it to `ps-core` so it's available to any crate that needs it.

```
ps-core/src/directory.rs  ← move from ps-server/src/directory.rs
```

This also unblocks future consumers (e.g. a `psctl import` command, or a worker that fetches the directory page on a schedule).

### 2. Move enrichment logic to `ps-core`

The team hierarchy derivation in `services/org/import.rs` (`derive_team_assignment`, `parse_html_to_records`, `DirectoryRecord`) is business logic, not gRPC adapter code. It belongs next to the repo import logic:

```
ps-core/src/directory.rs           — HTML parser (moved from ps-server)
ps-core/src/directory/mod.rs       — re-exports
ps-core/src/directory/parser.rs    — HTML parsing (scraper)
ps-core/src/directory/enrichment.rs — team hierarchy derivation, DirectoryRecord type
```

Or simpler — keep it as two files in `ps-core`:

```
ps-core/src/directory.rs   — HTML parsing
ps-core/src/import.rs      — DirectoryRecord, format detection, team derivation
```

### 3. Thin down the gRPC handler

After the moves, `OrgService::import_directory()` becomes a true thin adapter:

```rust
async fn import_directory(&self, request: Request<ImportDirectoryRequest>) -> ... {
    let content = String::from_utf8(req.file_content)?;
    let records = ps_core::import::parse_file_content(&content)?;
    let import_records = records.into_iter().map(Into::into).collect();
    let result = self.repos.org.import_records(&import_records).await?;
    Ok(Response::new(result.into()))
}
```

### 4. Future: scheduled directory fetch as a Restate handler

If/when Prism needs to **automatically fetch** the directory page on a schedule (rather than manual upload), _that_ is the right time to add a Restate handler — but even then it wouldn't use the `Source` trait. It would be a simple single-step handler:

```rust
#[restate_sdk::object]
trait DirectorySyncHandler {
    async fn sync() -> Result<(), TerminalError>;
}
```

With the logic: HTTP GET the directory URL → parse HTML → import records → done. No batching, no cursor, no watermark. Restate provides durability and scheduling, but the Source trait's multi-batch machinery isn't needed.

### 5. Consider a general "file import" trait if more formats emerge

If future file-based imports appear (e.g. CSV employee exports from BambooHR, LDAP dumps), a lightweight trait could standardize the parse→enrich→import pipeline:

```rust
trait FileImport {
    type Record;
    fn parse(&self, content: &str) -> Result<Vec<Self::Record>, Error>;
    fn to_import_records(&self, records: Vec<Self::Record>) -> Vec<ImportRecord>;
}
```

But don't build this until a second consumer exists (three-tier escalation rule).

## Proposed refactoring steps

1. **Move `directory.rs` from `ps-server` to `ps-core`** — add `scraper` dependency to `ps-core`
2. **Move `DirectoryRecord`, `DirectoryIdentity`, `parse_file_content`, `derive_team_assignment` to `ps-core`** — next to or merged with the parser
3. **Update `ps-server/services/org/mod.rs`** — import from `ps-core` instead of local `import` module, remove `mod import` and the `directory.rs` file
4. **Add `psctl import` subcommand** (optional, low priority) — now possible since parsing lives in a shared crate
5. **Tests stay colocated** — parser tests in `ps-core/src/directory.rs`, integration tests unchanged

This is a small, low-risk refactor (moving code, no behaviour change) that fixes the real structural problem (business logic trapped in the server binary) without introducing unnecessary orchestration complexity.

## Summary

The directory import and the ingestion sources share **domain concerns** (people, identities, teams) but not **operational concerns** (API polling, rate limits, batching, durability). The right fix is to push the shared domain logic down into `ps-core`, not to force a file-upload flow into an API-polling framework.
