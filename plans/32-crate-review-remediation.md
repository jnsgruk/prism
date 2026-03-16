# Plan 32: Crate Review Remediation

Addresses all findings from `reports/deep-review-crates.md` (2026-03-16).

Organised into 7 workstreams, each independently shippable. Within each workstream, tasks are ordered by dependency — later tasks may depend on earlier ones in the same workstream, but workstreams themselves are independent and can run in parallel.

---

## WS-1: Security Hardening

**Goal:** Close every high/medium security finding. Most are small, targeted fixes.

### 1.1 Restate SQL injection (high)

**Files:** `ps-server/src/services/handlers.rs:40,130-134`

The `is_invocation_alive` and `list_active_source_invocations` methods interpolate `invocation_id` and `source_name` directly into SQL strings sent to the Restate admin API. Restate's SQL API does not support parameterised queries, so we must validate inputs.

**Approach:**
- Add a `fn validate_restate_identifier(s: &str) -> Result<&str, Status>` helper in `handlers.rs` that rejects any string not matching `^[a-zA-Z0-9_-]+$` (use a `once_cell::sync::Lazy<Regex>`).
- Call it on `invocation_id` and `source_name` before interpolation.
- Return `Status::invalid_argument(...)` on failure.

### 1.2 Token journaling in Restate (high)

**Files:** `ps-workers/src/handlers/github_team_sync.rs:130-155`

The decrypted API token is returned from a `ctx.run()` side effect, which Restate journals to disk. This defeats at-rest encryption.

**Approach:**
- Move `decrypt_token` call **outside** `ctx.run()`. Restate only journals the return value of `ctx.run()` closures — code that runs outside them is re-executed on replay but not persisted.
- The `ctx.run("load_config", ...)` closure should return the encrypted secret bytes. Decryption happens after the closure returns, in normal async code.
- Same pattern applies if `github_ingestion.rs` has a similar path (verify and fix).

### 1.3 Hardcoded restore password (high)

**Files:** `ps-server/src/services/auth.rs:255-280`

`restore_backup` creates an admin user with password `"changeme"`.

**Approach:**
- Generate a random 24-character password using `rand::distributions::Alphanumeric`.
- Hash it with `hash_password()` as today.
- Return the plaintext password in the `RestoreBackupResponse` proto message (add a `generated_password` field to the proto).
- `psctl/restore.rs` displays it to the user once, with a warning to change it.
- `buf generate` after proto change.

### 1.4 Admin role enforcement (medium)

**Files:** `ps-server/src/services/admin.rs:203-237`, `ps-server/src/services/config.rs` (secret write endpoints)

`reset_data`, `create_api_token`, `revoke_api_token`, `create_backup` check authentication but not the `admin` role.

**Approach:**
- Add `fn require_admin(req: &Request<T>) -> Result<AuthContext, Status>` to `common.rs`, which calls `require_auth` then checks `ctx.role == "admin"` (or the `Role::Admin` enum once WS-3 lands). Returns `Status::permission_denied(...)` otherwise.
- Replace `require_auth` with `require_admin` in the four handlers above.
- Audit all other handlers for correct auth level. Any handler that mutates global state or exposes sensitive data should use `require_admin`.

### 1.5 Fail-closed auth interceptor (medium)

**Files:** `ps-server/src/interceptor.rs:139-142`

When no `Authorization` header is present on a non-public RPC, the request is forwarded without `AuthContext`. A missing `require_auth()` call in a handler silently allows unauthenticated access.

**Approach:**
- In the interceptor, when a non-public RPC has no `Authorization` header, return `Status::unauthenticated("missing authorization header")` immediately instead of forwarding.
- Remove the `require_auth` calls from individual handlers — they become redundant (the interceptor already guarantees `AuthContext` is present for non-public RPCs).
- Keep `require_admin` calls in handlers that need admin-level access.
- Update tests that rely on the current fail-open behaviour.

### 1.6 DB error leakage (medium)

**Files:** `ps-server/src/services/common.rs:18`

`db_err` exposes full sqlx error messages (table names, constraints, query fragments) to gRPC clients.

**Approach:**
- Change `db_err` to:
  ```rust
  pub fn db_err(e: impl std::fmt::Display) -> Status {
      tracing::error!(error = %e, "database error");
      Status::internal("internal error")
  }
  ```
- Same treatment for `backup_err`.
- Grep for any other `Status::internal(format!(...))` patterns that leak internals and fix them.

### 1.7 LIKE pattern escaping (medium)

**Files:** `ps-core/src/repo/metrics.rs:338-340`, `ps-core/src/repo/org/github_teams.rs:157`

User-supplied search terms are not escaped for LIKE wildcards (`%`, `_`). `list_people_paginated` already does this correctly.

**Approach:**
- Extract the escaping logic from `list_people_paginated` into a shared helper:
  ```rust
  fn escape_like_pattern(input: &str) -> String {
      input.replace('%', "\\%").replace('_', "\\_")
  }
  ```
- Place it in `ps-core/src/repo/mod.rs` (or a `repo/helpers.rs` if the module root is crowded).
- Apply in `list_team_contributions` and `list_github_teams`.

### 1.8 psctl session token exposure (high)

**Files:** `psctl/src/commands/restore.rs:81`

Session token printed to stdout where it can be logged or piped.

**Approach:**
- Print the token to stderr, prefixed with a warning: `eprintln!("⚠ Session token (treat as sensitive): {}", token)`.
- Add a comment explaining why stderr: it avoids capture in `psctl restore backup.bin > log.txt`.

### 1.9 psctl backup file permissions (low)

**Files:** `psctl/src/commands/backup.rs:23`

Backup files are created with default permissions (potentially world-readable on shared systems).

**Approach:**
- After writing the file, set permissions to `0o600` using `std::fs::set_permissions` with `std::os::unix::fs::PermissionsExt`.
- Gate behind `#[cfg(unix)]` — on Windows, default ACLs are typically per-user already.

### 1.10 URL encoding in GitHub REST client (medium)

**Files:** `ps-workers/src/client.rs:60-65`

URL parameters (`owner`, `repo`, etc.) interpolated without encoding.

**Approach:**
- Use `reqwest::Url::parse` + `.path_segments_mut().push()` to build URLs, or at minimum apply `urlencoding::encode()` to each path segment.
- This is particularly important for org names with special characters (e.g., `C++` → `C%2B%2B`).

### 1.11 GraphQL username sanitization (medium)

**Files:** `ps-workers/src/source.rs:456-457`

GitHub usernames interpolated into GraphQL search queries without sanitization.

**Approach:**
- Validate usernames against GitHub's rules (`^[a-zA-Z0-9-]+$`) before interpolation.
- Skip (with `tracing::warn!`) any username that fails validation rather than injecting it.

### 1.12 Secret memory handling (low)

**Files:** `ps-core/src/crypto.rs:50`, `ps-core/src/ingestion.rs:12`, `ps-server/src/services/config.rs:348`

Secret key material and decrypted values are plain `[u8; 32]` / `String` without zeroization.

**Approach:**
- Add `zeroize` crate as a dependency of `ps-core`.
- Wrap `IngestionContext.secret_key` as `Zeroizing<[u8; 32]>`.
- In `load_secret_key`, wrap the decoded bytes in `Zeroizing`.
- In `config.rs` set_secret handler, wrap the decrypted value in `secrecy::SecretString` (or just `Zeroizing<String>`) so it's cleared after encryption.
- This is defense-in-depth; the window is small since env vars are already in process memory.

### 1.13 Backup input validation (low)

**Files:** `ps-server/src/services/auth.rs:186-214`

`preview_backup` and `restore_backup` accept arbitrary binary data on a public endpoint.

**Approach:**
- Add a max upload size check (e.g., 100MB) early in the streaming handler — reject with `Status::resource_exhausted` if exceeded.
- Ensure `BackupReader` handles malformed input gracefully (no panics). Add a fuzz-style test with random bytes.

### 1.14 Connection timeout for ps-migrate (low)

**Files:** `ps-migrate/src/main.rs:17`

No connection timeout configured; the init container could hang indefinitely.

**Approach:**
- Add `.acquire_timeout(Duration::from_secs(30))` to `PgPoolOptions`.
- Optionally use `PgConnectOptions::from_str(&database_url)?` instead of `connect(&database_url)` to avoid holding the URL string longer than necessary.

---

## WS-2: Performance — Batch N+1 Queries

**Goal:** Eliminate all N+1 query patterns identified in the review. These are the highest-impact performance improvements.

### 2.1 Bulk upsert contributions (high)

**Files:** `ps-core/src/repo/activity.rs`, `ps-workers/src/source.rs:679-692`

`store_batch_impl` calls `upsert_contribution` per item.

**Approach:**
- Add `ActivityRepo::bulk_upsert_contributions(&self, items: &[ContributionInput]) -> Result<usize>`:
  ```sql
  INSERT INTO activity.contributions (
      platform, contribution_type, platform_id, platform_username,
      person_id, title, url, state, created_at, updated_at, closed_at,
      merged_at, metrics, metadata, source_name
  )
  SELECT * FROM UNNEST(
      $1::text[], $2::text[], $3::text[], $4::text[],
      $5::uuid[], $6::text[], $7::text[], $8::text[],
      $9::timestamptz[], $10::timestamptz[], $11::timestamptz[],
      $12::timestamptz[], $13::jsonb[], $14::jsonb[], $15::text[]
  )
  ON CONFLICT (platform, platform_id) DO UPDATE SET ...
  ```
- Build parallel `Vec`s for each column from the `ContributionInput` slice.
- Replace the loop in `store_batch_impl` with a single call.
- Keep the single-row `upsert_contribution` for cases where only one item needs upserting (if any).

### 2.2 Batch org import (high)

**Files:** `ps-core/src/repo/org/import.rs:88-114`

Per-record SQL in loops: `upsert_person`, `assign_team_if_needed`, `track_team_name`, `map_identities`.

**Approach — phased within this task:**

**Phase A — batch `upsert_person`:**
- Add `OrgRepo::bulk_upsert_people(tx, records: &[ImportRecord]) -> Vec<PersonRow>`:
  ```sql
  INSERT INTO org.people (name, email, ...)
  SELECT * FROM UNNEST($1::text[], $2::text[], ...)
  ON CONFLICT (email) DO UPDATE SET name = EXCLUDED.name, ...
  RETURNING id, email
  ```
- Map returned IDs back to records by email.

**Phase B — batch `assign_team_if_needed`:**
- Collect `(person_id, team_id)` pairs in memory, then:
  ```sql
  INSERT INTO org.team_memberships (person_id, team_id)
  SELECT * FROM UNNEST($1::uuid[], $2::uuid[])
  ON CONFLICT (person_id, team_id) DO NOTHING
  ```

**Phase C — batch `map_identities`:**
- Same UNNEST pattern for `org.platform_identities`.

**Phase D — refactor `ImportState`:**
- Replace the 7 free functions + mutable `ImportState` with an `ImportRunner` struct that owns the state and has methods for each phase.
- This improves readability without changing behaviour.

### 2.3 Batch list_sources secret status (high)

**Files:** `ps-server/src/services/config.rs:136-139`

`fetch_secret_status` makes a separate DB call per source.

**Approach:**
- Add `ConfigRepo::list_all_secret_keys(&self) -> Result<HashMap<Uuid, Vec<String>>>`:
  ```sql
  SELECT source_id, secret_key FROM config.secrets
  ```
  Group in Rust by `source_id`.
- In `list_sources`, call once and look up per source.

### 2.4 Batch GitHub team member/repo sync (medium)

**Files:** `ps-core/src/repo/org/github_teams.rs:88-99,129-142`

`replace_github_team_members` and `replace_github_team_repos` insert one row at a time.

**Approach:**
- Replace the INSERT loop with:
  ```sql
  INSERT INTO org.github_team_members (github_team_id, username)
  SELECT $1, unnest($2::text[])
  ```
- Same for repos.

### 2.5 Batch repository upserts (medium)

**Files:** `ps-workers/src/repos.rs:48-49`

`upsert_repository` called per repo in a loop.

**Approach:**
- Add `OrgRepo::bulk_upsert_repositories(repos: &[RepositoryInput])` using UNNEST arrays.
- Replace the loop in `repos.rs`.

### 2.6 Decrypt token once per run (medium)

**Files:** `ps-workers/src/source.rs:268,445`

`decrypt_token` called on every `fetch_batch` page.

**Approach:**
- Decrypt in `execute_ingestion` once, before the fetch loop.
- Pass the decrypted token through the `IngestionContext` (it already has `secret_key`; add a `token: String` field).
- Do NOT put the decrypted token inside a `ctx.run()` closure return value (see WS-1.2).

---

## WS-3: Type Safety — Finish String-to-Enum Migration

**Goal:** Complete the work started in plan 31. The enum definitions and sqlx impls exist; the remaining work is propagating them through repo row types, services, and workers.

### 3.1 Role enum

**Files:** `ps-core/src/auth/session.rs:13`, `ps-core/src/models/enums.rs`, `ps-server/src/interceptor.rs:18`, `ps-server/src/services/auth.rs`

**Approach:**
- Define `enum Role { Admin }` in `models/enums.rs` with `impl_sqlx_text!`.
- Use it in `AuthContext.role` (both in `ps-core` and `ps-server`).
- Replace hardcoded `"admin"` strings in `auth.rs` (`complete_setup`, `login`, `restore_backup`).
- Update `require_admin` (from WS-1.4) to match on `Role::Admin`.

### 3.2 Repo row types → enums

**Files:**
- `ps-core/src/repo/activity.rs:22` — `IngestionRunRow.status: String` → `IngestionStatus`
- `ps-core/src/repo/activity.rs:33` — `SourceStatusRow.source_type: String` → `Platform`
- `ps-core/src/repo/metrics.rs:279-293` — `ContributionDetailRow.platform/contribution_type/state` → enums
- `ps-core/src/repo/org/mod.rs:49` — `IdentityRow.platform: String` → `Platform`
- `ps-core/src/repo/org/mod.rs:74` — `ImportIdentity.platform: String` → `Platform`

**Approach:**
- Change the struct field types.
- The `impl_sqlx_text!` macro already handles encode/decode, so `sqlx::query_as!` will work if the DB column is `TEXT`.
- For columns that are `VARCHAR` or have a different type, verify compatibility or adjust the column type in a migration.
- Run `cargo sqlx prepare --workspace` after changes.

### 3.3 Implement FromStr for domain enums

**Files:** `ps-core/src/models/enums.rs:39-46`

`from_str_opt` methods should be replaced with `std::str::FromStr` impls.

**Approach:**
- Implement `FromStr` for `Platform`, `ContributionState`, `PeriodType`, `ContributionType`, `IngestionStatus`.
- Keep `from_str_opt` as a thin wrapper (`s.parse().ok()`) for existing call sites that expect `Option`, or migrate them to `.parse()`.
- Unify `impl_sqlx_text!` closures to use `FromStr` internally.

### 3.4 Error::Database structured variant

**Files:** `ps-core/src/error.rs:3-27`

All error variants carry `String` payloads.

**Approach:**
- Change `Database(String)` to `Database(sqlx::Error)` (or `Database(Box<sqlx::Error>)` to keep `Error` small).
- Update the `From<sqlx::Error>` impl to use the original error.
- Replace `e.to_string().contains("duplicate key")` in `config.rs:117` with proper `sqlx::Error` inspection:
  ```rust
  if let Error::Database(ref db_err) = e {
      if let Some(pg) = db_err.as_database_error() {
          if pg.is_unique_violation() { ... }
      }
  }
  ```
- Other variants can remain `String` for now — they are constructed from non-sqlx sources.

### 3.5 SourceState enum in psctl

**Files:** `psctl/src/format.rs:38-46`

Matches on raw `i32` magic numbers.

**Approach:**
- Use `SourceState::try_from(state)` from the generated proto enum.
- Match on enum variants instead of integer literals.

### 3.6 Typed ContributionInput.metrics/metadata

**Files:** `ps-core/src/ingestion.rs:32`

`metrics` and `metadata` are `serde_json::Value`.

**Approach:**
- Define `ContributionMetrics` struct with known fields (`review_hours: Option<f64>`, `additions: Option<i32>`, `deletions: Option<i32>`, etc.).
- Define `ContributionMetadata` struct for platform-specific data.
- Both should `#[derive(Serialize, Deserialize)]` with `#[serde(flatten)]` or explicit fields.
- Store as `jsonb` in Postgres — sqlx handles `Json<T>` natively.
- Migrate gradually: start with `Option` fields and `#[serde(default)]` for forward compatibility.

### 3.7 BTreeMap for BackupManifest

**Files:** `ps-core/src/backup.rs:18`

`HashMap<String, i32>` produces non-deterministic serialization.

**Approach:**
- Change to `BTreeMap<String, i32>`. One-line change. Produces deterministic JSON output.

### 3.8 Remove no-op parse in password.rs

**Files:** `ps-core/src/password.rs:5-8`

`.parse::<String>()` on a `String` is a no-op.

**Approach:**
- Replace `generate_hash(password).parse::<String>().map_err(...)` with `Ok(generate_hash(password))`.

### 3.9 Consistent impl_sqlx_text! usage

**Files:** `ps-core/src/models/enums.rs:242-258`

`ContributionType` and `IngestionStatus` use inline closures; others use `from_str_opt`.

**Approach:**
- After 3.3 lands (FromStr impls), update all `impl_sqlx_text!` invocations to use `s.parse().ok()` uniformly.

---

## WS-4: Concurrency — Parallelize Independent I/O

**Goal:** Use `tokio::try_join!`, `JoinSet`, or `futures::stream::buffer_unordered` for independent async operations.

### 4.1 compute_all_snapshots (medium)

**Files:** `ps-metrics/src/lib.rs:86-89`

Sequential DB query + upsert per team.

**Approach:**
- Use `futures::stream::iter(teams).map(|t| compute_snapshot(t)).buffer_unordered(4).try_collect()`.
- Cap concurrency at 4 to avoid overwhelming the DB pool.

### 4.2 reconcile_stale_runs (medium)

**Files:** `ps-server/src/services/handlers.rs:86-124`

Sequential HTTP calls to Restate admin API per active source.

**Approach:**
- Collect active sources, then `futures::future::join_all` the Restate checks.
- These are read-only HTTP GETs — safe to parallelize.

### 4.3 compare_teams sequential snapshots (medium)

**Files:** `ps-server/src/services/metrics.rs:202-223`

Each team snapshot computed sequentially.

**Approach:**
- Same `buffer_unordered` pattern as 4.1.

### 4.4 GitHub team sync: members + repos (medium)

**Files:** `ps-workers/src/handlers/github_team_sync.rs:228-243`

`fetch_all_members` and `fetch_all_repos` per team are independent.

**Approach:**
- `tokio::try_join!(fetch_all_members(...), fetch_all_repos(...))` per team.

### 4.5 count + data queries (low)

**Files:** `ps-core/src/repo/org/people.rs:179-197`

Two sequential queries for paginated listing.

**Approach:**
- `tokio::try_join!(count_query, data_query)`.
- Apply same pattern to `list_team_contributions` if it also has a separate count query.

### 4.6 Admin count queries (low)

**Files:** `ps-server/src/services/admin.rs:48-53`

Four independent count queries.

**Approach:**
- `tokio::try_join!(count_sources, count_people, count_teams, count_users)`.

### 4.7 Handler cancellation (low)

**Files:** `ps-server/src/services/handlers.rs:414-433`

Sequential HTTP DELETE calls to cancel invocations.

**Approach:**
- `futures::future::join_all` the cancellation requests.

---

## WS-5: Structure & Readability — Split God-Files, Extract Helpers

**Goal:** Reduce file sizes below 500 lines, eliminate duplication.

### 5.1 Split source.rs (high)

**Files:** `ps-workers/src/source.rs` (784 lines)

**Approach — split into a `source/` module directory:**
- `source/mod.rs` — `Source` trait impl, top-level orchestration, re-exports
- `source/cursor.rs` — `Cursor`, `CursorPhase`, serialization
- `source/fetch.rs` — `fetch_team_repos`, `fetch_member_search`, page-level fetch logic
- `source/store.rs` — `store_batch_impl`, contribution conversion, identity resolution
- `source/plan.rs` — `plan_ingestion`, `initial_cursor`, settings parsing

Each file should be 100-200 lines. Keep `Source` trait and `IngestionPlan` in `mod.rs` since they are the public API.

### 5.2 Split org.rs service (medium)

**Files:** `ps-server/src/services/org.rs` (890 lines)

**Approach:**
- Extract directory import logic (~150 lines: `parse_file_content`, `parse_html_to_records`, `derive_team_assignment`, `DirectoryRecord`, `DirectoryIdentity`) into `services/org/import.rs`.
- Keep the gRPC handler methods in `services/org/mod.rs` (or `services/org.rs` — the handler just calls the extracted functions).

### 5.3 Extract session creation helper (medium)

**Files:** `ps-server/src/services/auth.rs:62-94,124-141,265-281`

Session creation logic repeated 3 times.

**Approach:**
- Add to `auth.rs`:
  ```rust
  async fn create_session(
      &self,
      user_id: Uuid,
      session_type: &str,
  ) -> Result<(String, prost_types::Timestamp), Status> {
      let token = generate_token();
      let token_hash = hash_token(&token);
      let expires_at = OffsetDateTime::now_utc() + Duration::days(7);
      self.repos.auth.create_session(user_id, &token_hash, session_type, expires_at).await.map_err(db_err)?;
      Ok((token, to_timestamp(expires_at)))
  }
  ```
- Replace the 3 inline copies.

### 5.4 Extract handler boilerplate (medium)

**Files:** `ps-workers/src/handlers/github_ingestion.rs`, `github_team_sync.rs`, `metrics_compute.rs`

`load_config`, `create_run`, `complete_run`, `fail_run` are near-identical.

**Approach:**
- Add `handlers/common.rs` with:
  ```rust
  pub async fn load_source_config(ctx: &SharedObjectContext, state: &SharedState, source_name: &str) -> Result<SourceConfig, TerminalError>
  pub async fn create_ingestion_run(ctx: &SharedObjectContext, state: &SharedState, ...) -> Result<Uuid, TerminalError>
  pub async fn complete_ingestion_run(ctx: &SharedObjectContext, state: &SharedState, ...) -> Result<(), TerminalError>
  pub async fn fail_ingestion_run(ctx: &SharedObjectContext, state: &SharedState, ...) -> Result<(), TerminalError>
  ```
- Each handler file calls the shared functions.

### 5.5 Extract IngestionContext construction (medium)

**Files:** `ps-workers/src/handlers/github_ingestion.rs:290-295,342-347,383-388`

Identical construction 3 times.

**Approach:**
- Add a builder or factory on `SharedState`:
  ```rust
  impl SharedState {
      fn ingestion_context(&self) -> IngestionContext { ... }
  }
  ```
- Replace 3 inline constructions.

### 5.6 Deduplicate TeamWithCount mapping (low)

**Files:** `ps-core/src/repo/org/teams.rs:38-51,75-85,108-121`

Copy-pasted 3 times.

**Approach:**
- Extract a `fn map_team_row(row: ...) -> TeamWithCount` helper.
- Call from `list_teams`, `get_team`, `get_all_teams`.

### 5.7 Deduplicate GitHubTeamRow mapping (low)

**Files:** `ps-core/src/repo/org/github_teams.rs:175-188,249-262`

**Approach:**
- Same as 5.6 — extract `fn map_github_team_row(row: ...) -> GitHubTeamRow`.

### 5.8 Extract JSON-to-prost conversion (low)

**Files:** `ps-server/src/services/config.rs:50-111`

4 functions (~60 lines) for `serde_json` ↔ `prost_types::Struct` conversion.

**Approach:**
- Move to `services/common.rs` — they are generic utilities used by config but not specific to it.

### 5.9 Deduplicate Timestamp construction (low)

**Files:** `ps-server/src/services/auth.rs:142-145,200-203,283-286`

Manual `prost_types::Timestamp` construction duplicates `to_timestamp` helper.

**Approach:**
- Replace all 3 with calls to `to_timestamp()` from `common.rs`.

### 5.10 Extract REST client generic helper (low)

**Files:** `ps-workers/src/client.rs`

All REST methods follow identical request/response/pagination pattern.

**Approach:**
- Add a generic method:
  ```rust
  async fn paginated_get<T: DeserializeOwned>(&self, url: &str) -> Result<(Vec<T>, Option<String>, RateLimitInfo)>
  ```
- Deduplicate `list_org_repos`, `list_org_teams`, `list_team_members`, `list_team_repos`.

### 5.11 Extract psctl client constructor (low)

**Files:** `psctl/src/commands/*.rs`

Every command repeats `XxxServiceClient::with_interceptor(channel.clone(), auth.clone())`.

**Approach:**
- Add a `Clients` struct to `client.rs` that holds all service clients, constructed once.
- Pass `&Clients` to each command function.

### 5.12 list_team_contributions params struct (low)

**Files:** `ps-core/src/repo/metrics.rs:301-391`

11 parameters, `#[allow(clippy::too_many_arguments)]` suppressed.

**Approach:**
- Define `ListContributionsParams` struct (mirrors `ListPeopleParams` already in `people.rs`).
- Remove the `#[allow]`.

### 5.13 Replace build_progress_json with struct (low)

**Files:** `ps-workers/src/handlers/github_ingestion.rs:493-582`

90 lines of manual `serde_json::Map` construction.

**Approach:**
- Define `ProgressReport` struct with `#[derive(Serialize)]`.
- Replace manual map building with struct construction + `serde_json::to_value(&report)`.

### 5.14 Derive handler list from single source (low)

**Files:** `ps-server/src/services/handlers.rs:490-512`

Hardcoded `Vec` of handler info must be manually synced with handler validation.

**Approach:**
- Define a `const HANDLERS: &[HandlerInfo]` array.
- Use it in both `list_handlers` and `trigger_handler` validation.

### 5.15 Deduplicate psctl chunking logic (low)

**Files:** `psctl/src/commands/restore.rs:19-23,70-73`

Near-identical chunking for preview and restore.

**Approach:**
- Extract `fn chunk_data<T>(data: &[u8], f: impl Fn(Vec<u8>) -> T) -> Vec<T>`.

### 5.16 Enum conversion boilerplate (low)

**Files:** `ps-server/src/services/metrics.rs:32-46`, `ps-server/src/services/org.rs:63-81`

Mechanical `parse_period_type`/`period_type_to_proto`/`parse_team_type` functions.

**Approach:**
- Implement `From<ProtoType> for DomainType` and `From<DomainType> for ProtoType` (or `TryFrom`).
- Replace manual conversion functions with `.into()` / `.try_into()`.

---

## WS-6: Correctness & Minor Idiom Fixes

### 6.1 Review-turnaround bug (high)

**Files:** `ps-metrics/src/lib.rs:131-157`

Fallback review matching does not filter by `pr_platform_id`.

**Approach:**
- Build a `HashMap<&str, Vec<OffsetDateTime>>` keyed by `pr_platform_id` from the reviews.
- For each PR, look up reviews by its `platform_id` and find the earliest one after `pr.created_at`.
- This also fixes the O(PRs * reviews) performance issue (WS-4 territory but the fix is here).
- Add a test that verifies reviews are not cross-attributed between PRs.

### 6.2 Dead code in source.rs (medium)

**Files:** `ps-workers/src/source.rs:106-131`

`orgs` built via parse-then-`.and(None)` always yields empty `Vec`.

**Approach:**
- Remove the dead `orgs` parsing logic.
- If the intent was to use orgs from settings, verify the correct code path exists in `transition_to_member_search` and add a comment.

### 6.3 Redundant create_source call (low)

**Files:** `ps-workers/src/handlers/github_ingestion.rs:186`

`registry::create_source` called twice.

**Approach:**
- Remove the second call. The source type doesn't change during a run.

### 6.4 Unnecessary JSON Value round-trip (low)

**Files:** `ps-workers/src/handlers/github_ingestion.rs:268-317`

**Approach:**
- Use `serde_json::to_string` / `from_str` instead of going through `serde_json::Value`.

### 6.5 Minor idiom fixes (low, batch)

Batch of trivial one-line fixes:
- `ps-metrics/src/lib.rs:38-47` — Replace `map_or_else` with `match`.
- `ps-metrics/src/lib.rs:84-89` — Remove `computed` counter, use `teams.len()`.
- `ps-metrics/src/lib.rs:11-16` — Add `Debug, Clone, Copy` derives to `ReviewTurnaround`.
- `psctl/src/restore.rs:39` — Use `sort_by` instead of `sort_by_key` to avoid clone.
- `psctl/src/client.rs:26` — `Option<&String>` → `Option<&str>`.
- `psctl/src/main.rs:53` — Add `value_parser` for `--since` to validate date format at parse time.

### 6.6 reset_all bypasses query! macro (low)

**Files:** `ps-core/src/repo/activity.rs:510-530`

`format!("DELETE FROM {table}")` uses runtime `sqlx::query`.

**Approach:**
- Replace with individual `sqlx::query!("DELETE FROM activity.contributions")`, `sqlx::query!("DELETE FROM activity.watermarks")`, etc.
- The table list is static and small — explicit queries are both safer and consistent with project rules.

---

## WS-7: Memory & Efficiency

### 7.1 build_people O(N*M) → HashMap (medium)

**Files:** `ps-server/src/services/org.rs:36-61`

Linear scan of identities per person.

**Approach:**
- Build `HashMap<Uuid, Vec<&IdentityRow>>` from identities first.
- Look up per person in O(1).

### 7.2 Stream backup in psctl (medium)

**Files:** `psctl/src/commands/restore.rs:15-73`

Entire backup loaded into memory, then chunked twice.

**Approach:**
- Use `tokio::fs::File` + `tokio::io::AsyncReadExt::read_buf` to stream chunks.
- For preview: stream once, collect server response.
- For restore: stream again from disk (don't keep in memory).
- This eliminates the 3x memory amplification.

### 7.3 Build headers once per client (low)

**Files:** `ps-workers/src/client.rs:361-376`, `ps-workers/src/graphql.rs:237-244`

`default_headers()` allocates `HeaderMap` on every request.

**Approach:**
- Store the `HeaderMap` in the client struct, built once during construction.
- Clone the stored map (cheap, since `HeaderMap::clone` is efficient) or use `reqwest::Client::default_headers()` at client build time.

### 7.4 HashSet lookup without allocation (low)

**Files:** `ps-workers/src/source.rs:489`

`cur.ingested_repos.contains(&(owner.clone(), repo.clone()))` allocates per lookup.

**Approach:**
- Change `HashSet<(String, String)>` to `HashSet<(String, String)>` but look up with a borrowed key.
- Or use `HashSet<String>` with `format!("{owner}/{repo}")` as the key (single allocation vs two).

### 7.5 Lighter query for team IDs (low)

**Files:** `ps-metrics/src/lib.rs:83`

`list_teams(None, None)` fetches full `TeamWithCount` but only `.id` is used.

**Approach:**
- Add `OrgRepo::list_team_ids() -> Vec<Uuid>` — a simple `SELECT id FROM org.teams` without the member count join.

### 7.6 compare_team_snapshots CTE optimization (low)

**Files:** `ps-core/src/repo/metrics.rs:96-152`

Recursive CTE per row via `CROSS JOIN LATERAL`.

**Approach:**
- Pre-compute member counts for all teams in a single recursive CTE at the top of the query, then join.
- Benchmark before/after — the current approach may be fine for small team counts.

---

## Implementation Order

Recommended sequence for shipping these workstreams:

| Order | Workstream | Rationale |
|-------|-----------|-----------|
| 1 | WS-1 (Security) | Highest risk. Ship 1.1–1.8 as a single commit. |
| 2 | WS-6.1 (Review bug) | Correctness bug producing wrong metrics. |
| 3 | WS-2 (Batch N+1) | Largest performance impact. Ship 2.1–2.3 first. |
| 4 | WS-3 (Type safety) | Ship 3.1–3.2 first (Role enum + repo row types). |
| 5 | WS-5 (Structure) | Ship 5.1 (source.rs split) first, then others. |
| 6 | WS-4 (Concurrency) | Medium impact, low risk. |
| 7 | WS-7 (Memory) | Low impact, low risk. |
| 8 | WS-6.2–6.6 (Minor fixes) | Batch into a cleanup commit. |

Each workstream's tasks should be committed in logical chunks. Run `prek run -av` after each chunk.

---

## Checklist

- [x] WS-1.1: Validate Restate SQL inputs
- [x] WS-1.2: Move token decryption outside ctx.run()
- [x] WS-1.3: Generate random restore password
- [x] WS-1.4: Add role checks to admin operations
- [ ] WS-1.5: Fail-closed auth interceptor (deferred — generic ResBody constraint)
- [x] WS-1.6: Stop leaking DB errors
- [x] WS-1.7: Escape LIKE wildcards
- [x] WS-1.8: Session token to stderr
- [x] WS-1.9: Backup file permissions
- [x] WS-1.10: URL encoding in REST client
- [x] WS-1.11: GraphQL username sanitization
- [x] WS-1.12: Zeroize secret key material
- [x] WS-1.13: Backup input validation
- [x] WS-1.14: Connection timeout for ps-migrate
- [ ] WS-2.1: Bulk upsert contributions
- [ ] WS-2.2: Batch org import
- [ ] WS-2.3: Batch list_sources secret status
- [ ] WS-2.4: Batch GitHub team member/repo sync
- [ ] WS-2.5: Batch repository upserts
- [ ] WS-2.6: Decrypt token once per run
- [x] WS-3.1: Role enum
- [ ] WS-3.2: Repo row types → enums
- [x] WS-3.3: FromStr for domain enums
- [x] WS-3.4: Error::Database structured variant
- [x] WS-3.5: SourceState enum in psctl
- [ ] WS-3.6: Typed ContributionInput.metrics/metadata
- [x] WS-3.7: BTreeMap for BackupManifest
- [x] WS-3.8: Remove no-op parse in password.rs
- [x] WS-3.9: Consistent impl_sqlx_text! usage
- [ ] WS-4.1: Parallelize compute_all_snapshots
- [ ] WS-4.2: Parallelize reconcile_stale_runs
- [ ] WS-4.3: Parallelize compare_teams
- [x] WS-4.4: Parallelize GitHub team sync members+repos
- [ ] WS-4.5: Parallelize count+data queries
- [ ] WS-4.6: Parallelize admin count queries
- [ ] WS-4.7: Parallelize handler cancellation
- [ ] WS-5.1: Split source.rs
- [ ] WS-5.2: Split org.rs service
- [x] WS-5.3: Extract session creation helper
- [ ] WS-5.4: Extract handler boilerplate
- [ ] WS-5.5: Extract IngestionContext construction
- [x] WS-5.6: Deduplicate TeamWithCount mapping
- [x] WS-5.7: Deduplicate GitHubTeamRow mapping
- [ ] WS-5.8: Extract JSON-to-prost conversion
- [x] WS-5.9: Deduplicate Timestamp construction
- [ ] WS-5.10: Extract REST client generic helper
- [ ] WS-5.11: Extract psctl client constructor
- [x] WS-5.12: list_team_contributions params struct
- [x] WS-5.13: Replace build_progress_json with struct
- [x] WS-5.14: Derive handler list from single source
- [ ] WS-5.15: Deduplicate psctl chunking logic
- [ ] WS-5.16: Enum conversion boilerplate
- [x] WS-6.1: Fix review-turnaround bug
- [x] WS-6.2: Remove dead orgs code
- [x] WS-6.3: Remove redundant create_source call
- [x] WS-6.4: Remove unnecessary JSON Value round-trip
- [x] WS-6.5: Minor idiom fixes (batch)
- [x] WS-6.6: Replace runtime sqlx::query in reset_all
- [x] WS-7.1: build_people HashMap
- [ ] WS-7.2: Stream backup in psctl
- [ ] WS-7.3: Build headers once per client
- [ ] WS-7.4: HashSet lookup without allocation
- [ ] WS-7.5: Lighter query for team IDs
- [ ] WS-7.6: compare_team_snapshots CTE optimization
