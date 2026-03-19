# Plan 49: Fix GitHub Diff-Fetch Rate Limit Blocking

## Problem

`fetch_pr_diffs()` in `crates/ps-workers/src/github/source/fetch.rs` uses `tokio::time::sleep()` to wait for REST rate limit reset (up to ~60 minutes). This sleep is invisible to Restate — it happens inside `fetch_batch()`, which is outside `ctx.run()`. Restate's invoker abort timeout (`RESTATE_WORKER__INVOKER__ABORT_TIMEOUT=5min`) kills the handler after 5 minutes, triggering a full journal replay.

On replay, the in-memory progress counters reset to zero (they're not journaled), making the UI show the run as "restarted from scratch" — even though all data is safe and the watermark correctly prevents re-fetching. However, the real cost is:

1. **Wasted API calls**: replaying the journal re-executes non-journaled fetch calls
2. **Missing diffs**: if the rate limit is still active after replay, the same diffs get skipped again
3. **Diffs never recovered**: the watermark has already advanced past the affected PRs, so they won't be re-fetched on the next regular run — diffs are permanently missing without a manual backfill

## Design

### Approach: Skip + Durable Sleep + Retry Diffs

When `fetch_pr_diffs()` hits the REST rate limit:

1. **Don't sleep** — skip remaining diffs for the current batch, return which items were skipped
2. **Store the batch** — items are safe in the DB (with enrichment_content minus the diff field)
3. **Durable sleep** — `fetch_store_loop` calls `ctx.sleep()` until the rate limit resets (Restate-aware, survives replays)
4. **Retry skipped diffs** — after sleep, re-fetch only the skipped diffs and re-enqueue enrichments with updated content

This ensures every PR eventually gets its diff without requiring a backfill.

## Implementation

### Step 1: Change `fetch_pr_diffs()` return type

**File**: `crates/ps-workers/src/github/source/fetch.rs` (lines 564–653)

Currently returns `()`. Change to return skipped PR info and rate limit:

```rust
struct DiffFetchOutcome {
    /// PRs that were skipped due to rate limiting.
    /// Each entry is (item_index, owner, repo, pr_number).
    skipped: Vec<(usize, String, String, u32)>,
    /// Rate limit info if we hit the limit (for durable sleep calculation).
    rate_limit: Option<RateLimitInfo>,
}

async fn fetch_pr_diffs(
    ctx: &IngestionContext,
    items: &mut [ContributionInput],
) -> DiffFetchOutcome
```

When `DiffFetchResult::RateLimited` is returned from `fetch_single_pr_diff()`:
- **Do not sleep** — remove the `tokio::time::sleep(wait).await` call
- Collect the current and remaining `pr_targets` into `skipped`
- Record the `RateLimitInfo`
- Break out of the loop
- Remove `MAX_RATE_LIMIT_SLEEPS` constant (no longer needed)

### Step 2: Propagate skipped diffs through `FetchResult`

**File**: `crates/ps-core/src/ingestion.rs` (lines 120–128)

Add a field to `FetchResult`:

```rust
pub struct FetchResult {
    pub items: Vec<ContributionInput>,
    pub next_cursor: Option<String>,
    pub rate_limit: Option<RateLimitInfo>,
    pub etag: Option<String>,
    /// PR diffs that were skipped due to rate limiting.
    /// Format: (item_index_in_batch, owner, repo, pr_number).
    /// The orchestrator should retry these after a durable sleep.
    pub skipped_diffs: Vec<(usize, String, String, u32)>,
}
```

Also add to `SerFetchResult` in `handlers/ingestion_common.rs` (line 19):

```rust
#[serde(default)]
pub skipped_diffs: Vec<(usize, String, String, u32)>,
```

### Step 3: Update `fetch_team_repos()` and `fetch_member_search()`

**File**: `crates/ps-workers/src/github/source/fetch.rs`

Both functions call `fetch_pr_diffs(ctx, &mut items).await;` (lines 148, 342). Update to:

```rust
let diff_outcome = fetch_pr_diffs(ctx, &mut items).await;

// ... existing cursor logic ...

Ok(FetchResult {
    items,
    next_cursor,
    rate_limit: diff_outcome.rate_limit.or(Some(page.rate_limit)),
    etag: None,
    skipped_diffs: diff_outcome.skipped,
})
```

If `diff_outcome.rate_limit` is `Some`, use it (it reflects the REST pool being exhausted). Otherwise fall back to the GraphQL rate limit for progress display.

### Step 4: Handle skipped diffs in `fetch_store_loop()`

**File**: `crates/ps-workers/src/handlers/ingestion_common.rs` (lines 187–280)

After the existing `store_batch()` and `advance_watermark()` calls, add diff retry logic:

```rust
// After store_batch + advance_watermark...

// If diffs were skipped due to rate limiting, sleep durably then retry.
if !batch.skipped_diffs.is_empty() {
    if let Some(ref rl) = batch.rate_limit
        && rl.remaining == 0
    {
        let wait = sleep_duration_until_reset(rl);
        tracing::info!(
            wait_secs = wait.as_secs(),
            skipped = batch.skipped_diffs.len(),
            "sleeping for REST rate limit reset before retrying diffs"
        );
        ctx.sleep(wait).await?;
    }

    // Retry just the skipped diffs
    retry_skipped_diffs(ctx, ing_ctx, &batch.items, &batch.skipped_diffs).await?;
}
```

### Step 5: Implement `retry_skipped_diffs()`

**File**: `crates/ps-workers/src/handlers/ingestion_common.rs` (new function)

This function only needs to exist for GitHub, but lives in ingestion_common since it uses `ctx.run()` for the re-enqueue step.

```rust
/// Retry fetching PR diffs that were skipped due to rate limiting,
/// then re-enqueue the affected contributions for enrichment.
async fn retry_skipped_diffs(
    ctx: &ObjectContext<'_>,
    ing_ctx: &IngestionContext,
    original_items: &[ContributionInput],
    skipped: &[(usize, String, String, u32)],
) -> Result<(), TerminalError> {
    // 1. Re-fetch diffs for skipped PRs (not journaled — external API)
    // 2. For each successful diff, rebuild enrichment_content with diff attached
    // 3. Re-enqueue enrichments inside ctx.run() with updated content + hash
}
```

The re-enqueue uses `bulk_enqueue_enrichments()` which does `ON CONFLICT (contribution_id) DO UPDATE SET content = EXCLUDED.content, content_hash = EXCLUDED.content_hash WHERE content_hash != EXCLUDED.content_hash` — so it correctly updates the queue entry with the new content (now including the diff).

To look up the `contribution_id` for each skipped PR, we need the `platform_id` (format: `{owner}/{repo}/pull/{number}`). We can query `activity.contributions` by `(platform, platform_id)` to get the ID, or — simpler — store the `(contribution_id, platform_id)` pairs from the original `store_batch()` result and pass them through.

**Simpler approach**: rather than threading upsert results through, add a repo method:

```rust
// In ActivityRepo
pub async fn get_contribution_ids_by_platform_ids(
    &self,
    platform: &str,
    platform_ids: &[&str],
) -> Result<Vec<(Uuid, String)>, Error>
```

### Step 6: Export `sleep_duration_until_reset()`

**File**: `crates/ps-workers/src/github/source/fetch.rs` (line 762)

Currently `fn sleep_duration_until_reset(...)` is private. Make it `pub(crate)` so `ingestion_common.rs` can use it. Or move the simple calculation inline — it's just:

```rust
let secs = (reset_at - now_utc).whole_seconds().max(1) + 1;
Duration::from_secs(secs as u64)
```

This is simple enough to inline in the one call site in `fetch_store_loop`.

## File Changes Summary

| File | Change |
| --- | --- |
| `crates/ps-core/src/ingestion.rs` | Add `skipped_diffs` field to `FetchResult` |
| `crates/ps-workers/src/handlers/ingestion_common.rs` | Add `skipped_diffs` to `SerFetchResult`, add diff retry logic in `fetch_store_loop`, add `retry_skipped_diffs()` function |
| `crates/ps-workers/src/github/source/fetch.rs` | Change `fetch_pr_diffs()` to return `DiffFetchOutcome`, remove `tokio::time::sleep` and `MAX_RATE_LIMIT_SLEEPS`, propagate skipped info in `fetch_team_repos()` and `fetch_member_search()`, make `sleep_duration_until_reset` pub(crate) or inline it |
| `crates/ps-core/src/repo/activity/contributions.rs` | Add `get_contribution_ids_by_platform_ids()` method (for re-enqueue lookup) |
| `crates/ps-workers/src/jira/source/fetch.rs` | No change (Jira doesn't fetch diffs) |
| `crates/ps-workers/src/discourse/source/fetch.rs` | No change (Discourse doesn't fetch diffs) |

## Edge Cases

1. **Rate limit resets during replay**: `ctx.sleep()` is journaled — on replay, Restate skips the already-completed sleep. Safe.

2. **Diff retry also hits rate limit**: If the retry batch is large enough to exhaust the fresh rate limit window, `fetch_single_pr_diff()` will return `RateLimited` again. The retry function should handle this by skipping remaining diffs with a warning — at this point we've waited once, and waiting again would be excessive. Log the count of permanently-skipped diffs.

3. **Handler abort during ctx.sleep()**: Restate handles this correctly — `ctx.sleep()` is durable. On restart, Restate replays the journal and resumes from where the sleep ended.

4. **Skipped diffs on the final batch**: The retry happens before the next loop iteration, so even the last batch gets its diffs retried.

5. **Jira and Discourse**: `skipped_diffs` defaults to empty (`#[serde(default)]`). No change needed in their fetch implementations — the `Default` impl on `Vec` handles this.

6. **REST vs GraphQL rate limits**: These are separate pools on GitHub. The diff fetch uses REST; PR/review fetch uses GraphQL. The `rate_limit` field in `FetchResult` should prefer the REST rate limit when diffs were skipped (it's the one that matters for the retry).

## Testing

- Unit test `DiffFetchOutcome` construction with skipped items
- Integration test: mock GitHub REST API to return 429, verify diffs are retried after sleep
- Verify `bulk_enqueue_enrichments` correctly updates content when re-enqueued with diff

## Journal Compatibility

This changes the sequence of `ctx.run()` / `ctx.sleep()` calls in `fetch_store_loop`. Any in-flight GitHub ingestion invocations will hit journal mismatch errors after deployment. **Restate journal must be wiped** (or affected invocations cancelled) when deploying this change.
