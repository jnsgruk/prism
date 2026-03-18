# Plan 42: Enrichment Queue — Ingest-Time Content Capture

## Context

The current enrichment pipeline queries `activity.contributions` for content to send to AI models, but most contribution types have empty or insufficient content:

- **GitHub PRs**: `content: None` — PR description (`bodyText`) not fetched in GraphQL query
- **GitHub Reviews**: `body` fetched but often empty — inline file-level comments (the real review substance) not captured
- **Jira Tickets**: `content: None` — ticket description not fetched
- **Discourse Topics**: `content: None` — first post body is available during fetch but not stored on the topic

This means the model receives prompts like `"PR Title: fix bug\nDescription: (no description)\nLines added: 12..."` — producing low-quality enrichments based on titles and line counts alone.

### New approach

Capture rich content **during ingestion** (when the API response is in hand) and store it in a transient enrichment queue. The enrichment handler consumes from this queue. After enrichment succeeds, the raw content is deleted — only the derived enrichment (score, label, rationale, confidence) persists.

Raw content is transient fuel for enrichment, not a permanent record.

---

## Storage Decision: Database, not S3

Store enrichment content as a `JSONB` column on the queue table.

- **Transactional**: queue entries are created in the same store step as contributions — a DB column participates in the same transaction. S3 writes cannot.
- **Ephemeral**: content lifetime is minutes to hours. DB bloat is temporary and self-healing via `VACUUM`.
- **Size is bounded**: PR diffs truncated to ~20KB, review comments ~1-50KB, Discourse posts similar. Typical queue row is under 50KB.
- **Simpler**: no additional infrastructure dependency. RustFS isn't wired into the worker binary yet.
- **Future-proof**: if a future enrichment type needs large artifacts (full repo scans), the existing `ArtifactStore` + `object_store` crate can be wired in. The queue table can gain an optional `artifact_key TEXT` column at that point.

---

## Schema: `reasoning.enrichment_queue`

```sql
-- Migration: 0019_create_enrichment_queue.sql

CREATE TABLE reasoning.enrichment_queue (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id UUID NOT NULL UNIQUE
                    REFERENCES activity.contributions(id) ON DELETE CASCADE,
    content         JSONB NOT NULL,          -- structured blob, shape varies by contribution type
    content_hash    TEXT NOT NULL,            -- SHA-256 of content for change detection
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- FK lookups on contribution delete
CREATE INDEX idx_eq_contribution_id
    ON reasoning.enrichment_queue(contribution_id);
```

One row per contribution (not per enrichment type). The JSONB blob contains all data needed for any enrichment type applicable to that contribution. Prompt assembly happens in the enrichment handler, not at ingestion time.

Row deleted when all applicable enrichment types have been satisfied for that contribution.

### Content blob shapes

```jsonc
// pull_request
{
  "title": "refactor: extract auth middleware",
  "description": "This PR extracts the auth...",
  "labels": ["refactor", "auth"],
  "additions": 142, "deletions": 87, "changed_files": 5,
  "draft": false,
  "diff": "diff --git a/src/auth.rs...(truncated to ~20KB)"
}

// pr_review
{
  "pr_title": "refactor: extract auth middleware",
  "pr_number": 28,
  "state": "CHANGES_REQUESTED",
  "body": "Overall looks good but...",
  "inline_comments": [
    {"path": "src/auth.rs", "body": "This should use the new middleware trait"},
    {"path": "src/server.rs", "body": "nit: unnecessary clone"}
  ]
}

// discourse_topic
{
  "title": "How to configure snaps for...",
  "category": "Support",
  "body": "I'm trying to set up..."
}

// jira_ticket (future)
{
  "summary": "Users see 500 on login",
  "description": "When a user tries to...",
  "issue_type": "Bug",
  "priority": "High",
  "labels": ["auth", "urgent"]
}
```

---

## Content Needed Per Source

### GitHub PRs → `significance` enrichment

**Currently fetched**: title, additions, deletions, changed_files, labels, draft
**Need to add**:
- `bodyText` on PR node in GraphQL query (plain-text description, avoids markdown parsing)
- **PR diff** via `https://github.com/{owner}/{repo}/pull/{number}.diff` — simple HTTP GET, one request per PR, returns full unified diff. Truncate to ~20KB before storing in queue. Fetched concurrently with `buffer_unordered(N)` during ingestion, using the existing `reqwest::Client` + PAT auth header.

### GitHub Reviews → `review_depth` + `sentiment` enrichments

**Currently fetched**: `body` (top-level review comment), `state`
**Need to add**: `comments(first: 50) { nodes { body path } }` in GraphQL review fragment — inline code comments (the real substance of most reviews)

Skip queue entry if both `body` is empty AND `comments` is empty (pure approval click — not scorable).

### Discourse Topics → `topic` enrichment

**Currently fetched**: title, post_count, views, category, solved
**Already available during fetch**: first post `raw` (post_number=1) — just not stored on the topic contribution. No new API calls needed, just capture it.

### Jira Tickets → future enrichment (no enrichment type yet)

**Currently fetched**: summary, status, priority, labels, story_points
**Need to add**: `description` to API fields list (ADF JSON in API v3, needs plain-text extraction)

Queue entries created during ingestion so they're ready when Jira enrichment types are defined.

---

## Data Flow

```
Ingestion fetch (outside ctx.run — API calls not journaled)
  → Fetch contributions as today
  → Additionally: fetch PR diffs via .diff URL (concurrent, capped)
  → Build structured JSONB content blobs per contribution
  → Attach as enrichment_content: Option<serde_json::Value> on ContributionInput

Ingestion store (inside ctx.run — journaled, idempotent)
  → bulk_upsert_contributions RETURNING id, platform_id
  → bulk_enqueue_enrichments (queue table, ON CONFLICT refreshes content)

Enrichment handler (Restate service — follows existing handler pattern)
  → For each enrichment type:
      → find_queued (journaled ctx.run — DB read, consistent on replay)
      → process_enrichment_batch (NOT journaled — AI calls are idempotent, responses large)
      → log_cost (journaled ctx.run — DB write)
  → After all types: delete_fully_enriched (journaled ctx.run — DB write)
```

### Restate journaling rules (matching existing enrichment handler)

- **DB reads inside `ctx.run()`**: `find_queued_for_enrichment()` — journaled so replay skips the DB query
- **AI calls OUTSIDE `ctx.run()`**: responses are large (wasteful to journal), re-enriching is safe (upsert), no secrets in the journal
- **DB writes inside `ctx.run()`**: enrichment upserts, cost logging, queue cleanup — idempotent on replay
- **Progress updates OUTSIDE `ctx.run()`**: best-effort, doesn't affect replay correctness
- **Secrets OUTSIDE `ctx.run()`**: API keys live in `TaskRouter`, never journaled

---

## Implementation Phases

### Phase 1: Schema + repo layer

1. Migration `0019_create_enrichment_queue.sql`
2. Add types to `ps-core/src/repo/reasoning.rs`:
   - `EnrichmentQueueEntry` (for insertion: contribution_id, content JSONB, content_hash)
   - `QueuedContribution` (for consumption: id, contribution_id, contribution_type, content JSONB)
3. Add repo methods to `ReasoningRepo`:
   - `bulk_enqueue_enrichments(&[EnrichmentQueueEntry])` — UNNEST upsert, `ON CONFLICT (contribution_id) DO UPDATE` refreshes content/hash if changed
   - `find_queued_for_enrichment(enrichment_type, limit) -> Vec<QueuedContribution>` — JOIN queue with contributions, LEFT JOIN enrichments to find entries missing this enrichment type
   - `delete_fully_enriched_entries()` — remove queue rows where all applicable enrichment types are satisfied
   - `get_queue_stats() -> QueueStats` — total pending, by contribution type (for status UI)
4. Add `#[serde(default)] pub enrichment_content: Option<serde_json::Value>` to `ContributionInput` — structured JSONB blob, populated during fetch, consumed during store
5. Change `bulk_upsert_contributions` to `RETURNING id, platform_id` so store can map IDs to queue entries
6. `cargo sqlx prepare --workspace`

**Files**:
- `migrations/0019_create_enrichment_queue.sql` (new)
- [reasoning.rs](crates/ps-core/src/repo/reasoning.rs) — new types + repo methods
- [ingestion.rs](crates/ps-core/src/ingestion.rs) — add `enrichment_content` field
- [contributions.rs](crates/ps-core/src/repo/activity/contributions.rs) — `RETURNING` change

### Phase 2: GitHub fetch changes

Content assembly stays in the GitHub source directory (feature-first: `github/` owns all GitHub-specific logic).

1. **GraphQL** (`github/graphql.rs`): add `bodyText` to PR node in `SEARCH_PRS_QUERY` and `FETCH_PRS_QUERY`
2. **GraphQL** (`github/graphql.rs`): add `comments(first: 50) { nodes { body path } }` to review fragment
3. **Types** (`github/types.rs`): add `body_text: Option<String>` on `GraphQLSearchPr`/`GraphQLPr`, `GraphQLReviewComment`, `GraphQLReviewCommentConnection`
4. **Diff fetch** (`github/source/fetch.rs`): after GraphQL fetch, concurrently fetch `.diff` URL for each PR (`buffer_unordered`, capped). Truncate to ~20KB. Construct URL from existing `owner/repo` + PR number.
5. **Content assembly** (`github/source/fetch.rs`): in `search_pr_to_contributions()`, build structured JSON blobs and set `enrichment_content` on each `ContributionInput`:
   - PR: `{"title", "description", "labels", "additions", "deletions", "changed_files", "draft", "diff"}`
   - Review: `{"pr_title", "pr_number", "state", "body", "inline_comments": [{"path", "body"}]}`
   - Skip review enrichment content if body is empty AND no inline comments
6. Update test fixtures with new response shapes

**Files** (all within `crates/ps-workers/src/github/`):
- [graphql.rs](crates/ps-workers/src/github/graphql.rs)
- [types.rs](crates/ps-workers/src/github/types.rs)
- [fetch.rs](crates/ps-workers/src/github/source/fetch.rs)

### Phase 3: Discourse + Jira fetch changes

Content assembly stays in each source's directory (feature-first).

**Discourse** (`discourse/source/fetch.rs`):
1. In the fetch loop where topics are built: when the first post is available (post_number=1), build enrichment content `{"title", "category", "body"}` from topic title + category + first_post.raw
2. Set `enrichment_content` on the topic `ContributionInput`

**Jira** (`jira/source/`):
1. Add `description` to the API fields list in `client.rs`
2. Add ADF → plain text extraction helper in `fetch.rs` (Jira API v3 returns Atlassian Document Format JSON)
3. Build enrichment content `{"summary", "description", "issue_type", "priority", "labels"}` on each ticket `ContributionInput`

**Files**:
- [fetch.rs](crates/ps-workers/src/discourse/source/fetch.rs)
- [fetch.rs](crates/ps-workers/src/jira/source/fetch.rs)
- [client.rs](crates/ps-workers/src/jira/source/client.rs)

### Phase 4: Store integration

The "extract enrichment_content from items → build queue entries → bulk insert" logic is identical across all three sources. Extract a shared helper into `ingestion_common.rs` (which already holds shared boilerplate for all ingestion handlers).

Each source's `store_batch_impl` calls the shared helper after `bulk_upsert_contributions`.

1. In `ingestion_common.rs`: add `enqueue_enrichments(repos, items, upserted_ids)` helper
   - Maps `(id, platform_id)` pairs from upsert to `enrichment_content` blobs from items
   - Computes content_hash (SHA-256)
   - Calls `repos.reasoning.bulk_enqueue_enrichments(entries)`
2. Call from each source's `store_batch_impl` after upsert

**Files**:
- [ingestion_common.rs](crates/ps-workers/src/handlers/ingestion_common.rs) — shared helper
- [store.rs](crates/ps-workers/src/github/source/store.rs) — call helper
- [store.rs](crates/ps-workers/src/discourse/source/store.rs) — call helper
- [store.rs](crates/ps-workers/src/jira/source/store.rs) — call helper

### Phase 5: Enrichment handler migration

The handler (`handlers/enrichment.rs`) keeps the same Restate service pattern: `run_cycle()` method, `ctx.run()` wrappers for journaled steps, AI calls outside `ctx.run()`.

1. Replace `find_unenriched` ctx.run wrapper with `find_queued` — calls `repos.reasoning.find_queued_for_enrichment()` inside `ctx.run()`, journaled for replay consistency
2. `process_enrichment_batch` in `ps-reasoning` receives `QueuedContribution` items — reads structured JSON, extracts fields needed for this enrichment type, assembles prompt
3. Prompt assembly moves from `build_input_text` (plain text builder) to per-type builders in `ps-reasoning/src/features/enrichment/mod.rs` that read from the JSON blob. Prompts in `prompts.rs` stay unchanged — only the input text construction changes.
4. On success: upsert enrichment (as today, inside `ctx.run()`)
5. After processing all types in a cycle: call `delete_fully_enriched_entries()` inside a new `ctx.run()` wrapper (journaled DB write)
6. Update `get_enrichment_status()` to include queue depth

**Files**:
- [enrichment.rs](crates/ps-workers/src/handlers/enrichment.rs) — handler orchestration
- [mod.rs](crates/ps-reasoning/src/features/enrichment/mod.rs) — batch processing + input builders
- [prompts.rs](crates/ps-reasoning/src/features/enrichment/prompts.rs) — unchanged (preambles stay the same)
- [reasoning.rs](crates/ps-core/src/repo/reasoning.rs) — query changes

### Phase 6: Backfill existing contributions

Existing contributions won't have queue entries since they were ingested before this change.

- **Reset watermarks** for GitHub and Discourse sources → triggers re-ingestion → new fetch code populates queue entries as a side effect. Simplest approach, and contributions get the latest data anyway.
- Delete existing low-quality enrichments so they get re-done with real content: `DELETE FROM reasoning.enrichments WHERE enrichment_type IN ('review_depth', 'sentiment', 'significance')`

---

## Re-enrichment

If prompts change or models upgrade and you want to re-score:

1. `DELETE FROM reasoning.enrichments WHERE enrichment_type = 'significance'` — clears old results
2. Queue rows still exist (they're only deleted when *all* enrichment types are satisfied) — so the handler will pick them up on the next cycle with the new prompt/model
3. If queue rows were already cleaned up, reset watermarks → re-ingestion recreates them

No re-ingestion needed for prompt-only changes, since the structured content in the queue is prompt-agnostic.

---

## Verification

1. Run `prek run -av` — all tests pass, zero warnings
2. Trigger a GitHub ingestion run → verify queue entries created with non-empty content
3. Trigger enrichment → verify queue entries consumed, enrichments created, queue rows deleted
4. Check Restate journal → fetch result includes enrichment_payloads, queue operations visible
5. Check `reasoning.enrichment_queue` is empty after successful enrichment cycle
6. Check `reasoning.enrichments` has entries with meaningful `input_preview` (actual PR descriptions, review comments)
