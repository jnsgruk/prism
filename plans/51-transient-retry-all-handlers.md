# 51 — Transient error retry for all ingestion handlers

## Problem

A transient GitHub 502 was silently skipping repos during backfill (fixed in `6a123f6`
for GitHub only). The same gap exists in Jira and Discourse: a single 502/503/timeout
skips an entire project or category with no retry.

GitHub team sync is lower risk — Restate retries the whole handler and the operation is
idempotent — so it's out of scope here.

## Current state

| Handler   | Retry? | Error type at call site         | Transient classification? |
|-----------|--------|---------------------------------|---------------------------|
| GitHub    | Yes    | `GraphQLClientError` / `GitHubError` | `is_transient()` on both |
| Jira      | No     | `ps_core::Error::Internal(String)`   | No — status code lost     |
| Discourse | No     | `ps_core::Error::Internal(String)`   | No — status code lost     |

The root issue for Jira and Discourse is that their clients convert HTTP errors into
`Error::Internal(String)`, discarding the status code. By the time the caller sees the
error, it can't distinguish a retryable 502 from a permanent 404.

## Design

Add an `HttpStatus` variant to `ps_core::Error` that preserves the status code, plus an
`is_transient()` method. Update Jira/Discourse clients to use it. Lift the
`retry_transient()` helper to a shared module and apply it at fetch call sites.

This keeps retry logic visible at the call site (consistent with GitHub), preserves
status code information, and gives all handlers a single transient classification method.

## Implementation

### Step 1 — Add `HttpStatus` variant and `is_transient()` to `ps_core::Error`

File: `crates/ps-core/src/error.rs`

- Add `HttpStatus { status: u16, message: String }` variant
- Add `is_transient()` method covering `HttpStatus` (5xx) and `Internal` (timeout strings
  from reqwest)
- No breaking changes — existing `Internal` usage continues to work

### Step 2 — Lift `retry_transient` to a shared module

Move the `retry_transient()` helper from `crates/ps-workers/src/github/source/fetch.rs`
to a new `crates/ps-workers/src/retry.rs` module. Update GitHub fetch to import from
there. The function is generic over error type via `is_transient: fn(&E) -> bool`, so it
works with both `ps_core::Error` and the GitHub-specific error types.

### Step 3 — Update Jira client to use `HttpStatus`

File: `crates/ps-workers/src/jira/client.rs`

In `search()` and `get_issue_with_changelog()`, replace:
```rust
Err(ps_core::Error::Internal(format!("jira search returned {status}: {body}")))
```
with:
```rust
Err(ps_core::Error::HttpStatus { status: status.as_u16(), message: body })
```

Keep `reqwest::Error` mapping as `Internal` (reqwest errors already contain timeout/connect
info that `is_transient()` can match on via string heuristics, or we can map those to
`HttpStatus` with a synthetic status code — but string matching is pragmatic enough).

### Step 4 — Update Discourse client to use `HttpStatus`

File: `crates/ps-workers/src/discourse/client.rs`

In `require_success()`, replace:
```rust
Err(ps_core::Error::Internal(format!("discourse API returned {status}")))
```
with:
```rust
Err(ps_core::Error::HttpStatus { status: status.as_u16(), message: format!("discourse API returned {status}") })
```

### Step 5 — Add retry to Jira fetch

File: `crates/ps-workers/src/jira/source/fetch.rs`

Wrap the `client.search()` call in the per-project fetch loop with `retry_transient()`,
using `ps_core::Error::is_transient` as the classifier. The existing skip-and-record
pattern stays for permanent errors.

### Step 6 — Add retry to Discourse fetch

File: `crates/ps-workers/src/discourse/source/fetch.rs`

Three call sites:
1. **Category-level `latest_for_category()`** — wrap with `retry_transient()`
2. **Topic detail `topic()`** — wrap with `retry_transient()`
3. **Post likers `post_likers()`** — wrap with `retry_transient()`

### Step 7 — Verify journal safety

All retry sites are inside `fetch_batch()` which runs outside `ctx.run()`. Confirm no
`ctx.run()` calls are introduced. Existing in-flight invocations are unaffected.

## Out of scope

- **GitHub team sync** — Restate retries the whole handler; low blast radius
- **Enrichment handler** — AI provider errors have their own retry/backoff logic
- **Identity resolution** — Discourse API calls here are best-effort lookups
