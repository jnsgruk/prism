# Plan: Discourse Community Participation Tracking

## Context

Prism currently tracks Discourse **topics** and **posts** but cannot give a full picture of someone's community participation. Key engagement signals are missing:

- **Replies** — posts are stored but there's no distinction between a topic-opening post and a reply to someone else. The Discourse API provides `reply_to_post_number` on every post; we just don't capture it.
- **Likes given** — no data at all. This is a core engagement signal showing who actively participates beyond authoring.
- **Likes received** — partially captured as `like_count` per post, but not surfaced as a per-person metric.

This plan adds the data needed to answer: "How does this person participate in the Discourse community?" — authoring, replying, and liking.

---

## Design Decisions

### 1. Replies: Enrich `DiscoursePostData`, not a new ContributionType

A reply is still a post. Splitting into `DiscourseReply` would complicate queries ("show all posts by person X" needs a union of two types). Instead, add `reply_to_post_number: Option<i32>` and `is_reply: bool` to `DiscoursePostData`. Backward-compatible via `#[serde(default)]` — existing JSONB rows deserialise with `None`/`false`.

### 2. Likes given: New `DiscourseLike` ContributionType

A like is a fundamentally different action from a post — different actor (liker, not author), different meaning (engagement vs content). It deserves its own type. The `platform_id` is `like-{post_id}-{username}` for idempotent upserts.

### 3. Likes received: Derive from existing data

`like_count` already lives on every `DiscoursePost`. Per-person "likes received" = `SUM(metrics->>'likes')` grouped by person. No new contribution type needed — avoids double-counting the same event from two perspectives.

### 4. Like fetching: Per-post, integrated into existing topic fetch

When processing topic detail, for each post with `like_count > 0`, fetch likers via Discourse's post-actions endpoint. This piggybacks on the existing loop with no architectural changes (no second ingestion phase, no cursor rework).

- **Opt-in** via `fetch_likes` source setting (default `false`) to protect existing deployments from unexpected API cost
- **Capped concurrency** via `buffer_unordered(5)` for like fetches within a topic
- **Timestamp limitation**: the post-actions endpoint doesn't return per-like timestamps; we use the post's `created_at` as a proxy. Acceptable for v1.

### 5. Identity mapping: Automatic via existing pipeline

`DiscourseLike` contributions set `platform_username` = liker's username. The existing `store_batch_impl` already collects all unique usernames from items and runs `batch_ensure_identities` — likes flow through identity resolution with zero store-layer changes. The liker's `display_name` goes into metadata for the existing display-name extraction path.

---

## Implementation Steps

### Step 1: Extend Discourse API client

**File:** `crates/ps-workers/src/discourse/client.rs`

- Add `reply_to_post_number: Option<i32>` (with `#[serde(default)]`) to `Post` struct
- Add response types for the post-actions likers endpoint:
  ```rust
  pub struct PostActionUsersResponse {
      pub post_action_users: Vec<PostActionUser>,
  }
  pub struct PostActionUser {
      pub id: i64,
      pub username: String,
      pub name: Option<String>,
  }
  ```
- Add `pub async fn post_likers(&self, post_id: i64) -> Result<Vec<PostActionUser>, ps_core::Error>` method — calls `GET {base_url}/post_action_users.json?id={post_id}&post_action_type_id=2` (type 2 = like)

### Step 2: Enrich `DiscoursePostData` with reply fields

**File:** `crates/ps-core/src/models/contribution_data.rs`

Add to `DiscoursePostData`:
```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub reply_to_post_number: Option<i32>,
#[serde(default)]
pub is_reply: bool,
```

### Step 3: Add `DiscourseLike` contribution type + data struct

**File:** `crates/ps-core/src/models/enums.rs`
- Add `DiscourseLike` variant to `ContributionType`
- Add `as_str` → `"discourse_like"`, `FromStr` arm

**File:** `crates/ps-core/src/models/contribution_data.rs`
- Add `DiscourseLikeData` struct:
  ```rust
  pub struct DiscourseLikeData {
      pub post_id: i64,
      pub topic_id: i64,
      pub post_number: i32,
      #[serde(default, skip_serializing_if = "Option::is_none")]
      pub post_author: Option<String>,
  }
  ```
- Add `DiscourseLike(DiscourseLikeData)` variant to `ContributionData` enum

### Step 4: Update fetch logic

**File:** `crates/ps-workers/src/discourse/source/fetch.rs`

**4a. Reply tracking** — update `build_post_input` to populate `reply_to_post_number` and `is_reply` from `post.reply_to_post_number`.

**4b. Like fetching** — in `fetch_batch_impl`, after processing posts for a topic:
- Read `fetch_likes` bool from source settings (default `false`)
- If enabled, collect posts with `like_count > 0`
- Use `futures::stream::buffer_unordered(5)` to fetch likers concurrently
- For each (post, liker) pair, call `build_like_input` to create a `ContributionInput`

**4c. New function:**
```rust
fn build_like_input(
    liker: &PostActionUser,
    post: &Post,
    topic: &TopicSummary,
    cur: &Cursor,
) -> ContributionInput
```
- `platform_id`: `"like-{post.id}-{liker.username}"`
- `platform_username`: liker's username (for identity resolution)
- `contribution_type`: `ContributionType::DiscourseLike`
- `created_at`: post's `created_at` (best available proxy)
- `metadata`: `{ post_author, topic_id, topic_title, post_number, display_name }`
- `url`: link to the liked post

### Step 5: Verify store logic handles new type (likely no changes)

**File:** `crates/ps-workers/src/discourse/source/store.rs`

The existing `store_batch_impl` already:
1. Collects unique `(username, display_name)` from all items → likes will be included
2. Calls `batch_ensure_identities` → liker identities auto-created
3. Calls `bulk_upsert_contributions` → likes inserted alongside posts

Verify this works end-to-end. The `backfill_discourse_person_ids` query uses `metadata->>'username'` — add `username` to like metadata so backfill covers likes too.

### Step 6: Add tests

**File:** `crates/ps-core/src/models/contribution_data.rs`
- `round_trip_discourse_like` test
- Update `round_trip_discourse_post` with new reply fields
- Backward-compat test: deserialise old `DiscoursePostData` JSON without new fields → defaults to `None`/`false`

### Step 7: Update sqlx query cache (separate commit)

Run `cargo sqlx prepare --workspace` if any query macros changed (unlikely since ContributionType is TEXT and no new queries are added).

### Step 8: Frontend type display (follow-up)

The proto `contribution_type` field is a free-form string, so `"discourse_like"` passes through without proto changes. Add display labels/icons for the new type wherever contribution types are rendered (badge labels, filter dropdowns, table columns). This can be a separate follow-up PR.

---

## Files Modified

| File | Change |
|------|--------|
| `crates/ps-workers/src/discourse/client.rs` | `reply_to_post_number` on Post, `post_likers()` method, response types |
| `crates/ps-core/src/models/enums.rs` | `DiscourseLike` variant in `ContributionType` |
| `crates/ps-core/src/models/contribution_data.rs` | `DiscourseLikeData` struct, reply fields on `DiscoursePostData`, `ContributionData::DiscourseLike` |
| `crates/ps-workers/src/discourse/source/fetch.rs` | Reply tracking, like fetching, `build_like_input` |
| `crates/ps-workers/src/discourse/source/store.rs` | Verify identity resolution (likely no changes) |

## Commit Sequence

1. `feat: add reply_to_post_number to Discourse client Post struct`
2. `feat: add DiscourseLike contribution type and enrich DiscoursePostData with reply fields`
3. `feat: fetch Discourse likes and track reply relationships`
4. `chore: update sqlx query cache` (if needed, separate commit per project rules)

## Verification

1. `cargo clippy --allow-dirty` — zero warnings
2. `cargo test` — all existing + new tests pass
3. `nix fmt` — clean formatting
4. `prek run -av` — full lint/test/format suite green
5. Manual: configure a Discourse source with `fetch_likes: true`, run ingestion, verify:
   - Posts have `is_reply`/`reply_to_post_number` populated in metrics JSONB
   - `DiscourseLike` contributions appear with correct liker username and identity mapping
   - Liker identities are auto-created via `batch_ensure_identities`
   - Re-ingestion is idempotent (no duplicate likes)
