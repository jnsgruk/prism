# Plan 37: Team-Level Discourse Activity Aggregation

## Problem

Discourse data is ingested (topics, posts, likes) and topics are counted in cross-source throughput, but there is no dedicated view of Discourse activity on team pages. A team manager looking at `/teams/:teamId` sees PR and review activity but has no visibility into how their team participates across Discourse instances (e.g., `discourse-ubuntu`, `discourse-snapcraft`). The data exists in `activity.contributions` — it just isn't surfaced.

## Design Principle: Progressive Disclosure

Activity gets more specific as you drill down through the team hierarchy:

| Level | What you see |
|-------|-------------|
| **Org root** (e.g., "Engineering") | Aggregate counts across all instances: total topics, posts, likes. Sparkline trends. Per-instance breakdown bar. |
| **Division** (e.g., "Desktop") | Same aggregates scoped to division + per-child-team comparison columns for Discourse activity. |
| **Leaf team** (e.g., "Desktop Shell") | Per-member breakdown table. Category distribution. Response time stats. Solved topic rate. Collapsible contribution drilldown showing individual topics/posts. |
| **Individual** (future, Phase 3) | Full activity timeline with links to every topic, post, and like on each instance. |

This mirrors the existing pattern: metric cards summarize at every level, comparison table shows children, and collapsible sections provide drilldown at leaf teams.

## Metrics to Compute

### Discourse Activity Metrics (new)

| Metric | Computation | Stored in |
|--------|------------|-----------|
| `discourse_topics_created` | Count of `DiscourseTopic` contributions in period | `team_snapshots.raw_metrics` |
| `discourse_posts` | Count of `DiscoursePost` contributions in period | `team_snapshots.raw_metrics` |
| `discourse_replies` | Count of `DiscoursePost` where `metrics.is_reply == true` | `team_snapshots.raw_metrics` |
| `discourse_likes_given` | Count of `DiscourseLike` contributions in period | `team_snapshots.raw_metrics` |
| `discourse_likes_received` | Sum of `metrics.likes` across team members' posts | `team_snapshots.raw_metrics` |
| `discourse_solved_topics` | Count of `DiscourseTopic` where `metrics.solved == true` | `team_snapshots.raw_metrics` |
| `discourse_active_participants` | Distinct `person_id` values with any Discourse contribution | `team_snapshots.raw_metrics` |

**Per-instance breakdown**: All metrics above are also broken down by Discourse instance (e.g., `{ "discourse-ubuntu": { "topics": 5, "posts": 23 }, "discourse-snapcraft": { "topics": 1, "posts": 4 } }`). This lives in `raw_metrics.discourse_by_instance`.

**Category distribution**: At leaf-team level, count contributions grouped by `metadata.category`. Computed on the fly from contribution queries (not snapshot-stored, since categories are high cardinality).

## Backend Changes

### 1. Metrics Computation (`ps-metrics`)

New module: `ps-metrics/src/discourse.rs`

```rust
pub struct DiscourseMetrics {
    pub topics_created: i32,
    pub posts: i32,
    pub replies: i32,
    pub likes_given: i32,
    pub likes_received: i32,
    pub solved_topics: i32,
    pub active_participants: i32,
    pub by_instance: HashMap<String, DiscourseInstanceMetrics>,
}

pub struct DiscourseInstanceMetrics {
    pub topics_created: i32,
    pub posts: i32,
    pub replies: i32,
    pub likes_given: i32,
    pub solved_topics: i32,
}

pub fn compute_discourse_metrics(contributions: &[ContributionMetricRow]) -> DiscourseMetrics
```

This function filters the existing `ContributionMetricRow` slice (already fetched for snapshot computation) by Discourse contribution types and aggregates. No new DB queries needed for the snapshot path.

### 2. Snapshot Storage

The `DiscourseMetrics` struct serializes into `raw_metrics` alongside the existing `throughput_by_source`. The `compute_team_snapshot` function in `ps-metrics/src/lib.rs` calls `compute_discourse_metrics` and merges the result into `raw_metrics`.

No new columns or tables — this uses the existing JSONB `raw_metrics` field in `metrics.team_snapshots`.

### 3. Proto Changes (`proto/prism/v1/metrics.proto`)

Extend `TeamMetrics` with Discourse fields:

```protobuf
message TeamMetrics {
  // ... existing fields 1-15 ...

  // Discourse activity
  int32 discourse_topics_created = 16;
  int32 discourse_posts = 17;
  int32 discourse_replies = 18;
  int32 discourse_likes_given = 19;
  int32 discourse_likes_received = 20;
  int32 discourse_solved_topics = 21;
  int32 discourse_active_participants = 22;
  repeated DiscourseInstanceMetrics discourse_by_instance = 23;
}

message DiscourseInstanceMetrics {
  string instance = 1;  // e.g., "ubuntu", "snapcraft"
  int32 topics_created = 2;
  int32 posts = 3;
  int32 replies = 4;
  int32 likes_given = 5;
  int32 solved_topics = 6;
}
```

New RPC for first-reply-time and category breakdown (leaf-team drill-down):

```protobuf
message GetDiscourseActivityRequest {
  string team_id = 1;
  Period period = 2;
}

message GetDiscourseActivityResponse {
  repeated CategoryCount category_distribution = 1;
  repeated DiscourseActivityDataPoint activity_trend = 2;
  repeated TopContributor top_contributors = 3;
}

message CategoryCount {
  string category = 1;
  int32 topics = 2;
  int32 posts = 3;
}

message DiscourseActivityDataPoint {
  string date = 1;
  int32 topics = 2;
  int32 posts = 3;
  int32 likes = 4;
  string instance = 5;  // optional per-instance breakdown
}

message TopContributor {
  string person_id = 1;
  string name = 2;
  int32 topics = 3;
  int32 posts = 4;
  int32 likes_received = 5;
  int32 solved = 6;
}
```

Add to `MetricsService`:

```protobuf
rpc GetDiscourseActivity(GetDiscourseActivityRequest) returns (GetDiscourseActivityResponse);
```

### 4. Repository Queries (`ps-core/src/repo/metrics.rs`)

New queries:

- `get_discourse_category_distribution(team_member_ids, period)` — groups `DiscoursePost` and `DiscourseTopic` contributions by `metadata.category`, returns `(category, topic_count, post_count)`.
- `get_discourse_activity_trend(team_member_ids, period)` — daily counts of topics/posts/likes, optionally grouped by platform (instance).
- `get_discourse_top_contributors(team_member_ids, period)` — per-person aggregates of topics, posts, likes received, solved count.

### 5. Service Layer (`ps-server`)

`MetricsService::get_discourse_activity` — resolves team members (recursive for parent teams), calls the three repo queries above via `tokio::try_join!`, maps to proto response.

## Frontend Changes

### 1. Discourse Metric Cards

Add a new conditional row of metric cards to `TeamMetricCards` when any Discourse data exists (`discourse_topics_created > 0 || discourse_posts > 0`):

| Card | Icon | Value | Description |
|------|------|-------|-------------|
| Topics Created | `MessageSquarePlus` | count | "New Discourse topics started by team members" |
| Posts & Replies | `MessagesSquare` | count | "Total posts, including N replies" (secondary text) |
| Likes Given | `ThumbsUp` | count | "Likes given by team members on Discourse" |
| Solved | `CheckCircle` | count | "Topics with accepted answers from team members" |

When multiple Discourse instances exist, show a small per-instance breakdown in each card's secondary text (e.g., "Ubuntu: 12 · Snapcraft: 3").

### 2. Comparison Table Columns

Add optional Discourse columns to the child-teams comparison table (only when `sourcePlatforms` includes any `discourse-*`):

- **Topics** — `discourse_topics_created`
- **Posts** — `discourse_posts`
- **Engagement** — `discourse_likes_given + discourse_likes_received` (combined engagement score)

These columns use the same sortable header pattern as existing flow metric columns.

### 3. Discourse Activity Section (Collapsible)

New collapsible card on the team page, between the trend charts and the PRs section:

```
<Collapsible>
  <Card>
    <CardHeader>
      <MessageCircle icon />
      <CardTitle>Discourse Activity</CardTitle>
      <Badge>{totalPosts + totalTopics}</Badge>
    </CardHeader>
    <CollapsibleContent>
      <!-- Instance tabs (if multiple) -->
      <!-- Activity trend chart (stacked area: topics + posts + likes) -->
      <!-- Category distribution (horizontal bar chart) -->
      <!-- Top contributors table (leaf teams only) -->
    </CollapsibleContent>
  </Card>
</Collapsible>
```

**Instance tabs**: When a team has activity across multiple Discourse instances, show tabs (one per instance + "All") at the top of the section. Selecting an instance filters all charts and tables below.

**Activity trend chart**: Recharts `AreaChart` with stacked areas for topics (primary color), posts (secondary), likes (tertiary). Same date axis as the throughput trend.

**Category distribution** (leaf teams): Horizontal `BarChart` showing top 10 categories by post count. Useful for understanding where the team focuses (e.g., "snap-store" vs "desktop" vs "kernel").

**Top contributors table** (leaf teams): Small `DataTable` with columns: Name, Topics, Posts, Likes Received, Solved. Sorted by total activity descending. Shows how participation is distributed across team members.

### 4. Throughput Trend Enhancement

The existing `ThroughputTrendChart` already supports per-source breakdown via `sourcePlatforms`. Discourse instances already appear as separate sources (e.g., `discourse-ubuntu`). No changes needed — this already works.

### 5. Hook & API Client

New hook in `frontend/views/teams/hooks/use-discourse-activity.ts`:

```typescript
export const useDiscourseActivity = (teamId: string, period: Period) =>
  useQuery({
    queryKey: ["discourse-activity", teamId, period],
    queryFn: () => metricsClient.getDiscourseActivity({ teamId, period }),
    enabled: !!teamId,
  });
```

## Progressive Detail by Level

### At any level (via snapshot data, no extra RPC)
- Discourse metric cards (topics, posts, likes, solved)
- Per-instance breakdown in card secondary text
- Discourse columns in child comparison table

### At leaf teams (via `GetDiscourseActivity` RPC, fetched on expand)
- Activity trend chart
- Category distribution
- Top contributors table

The collapsible section lazily fetches the `GetDiscourseActivity` data only when expanded, keeping the initial page load fast.

## Implementation Order

### Step 1: Metrics computation + snapshot storage
- [ ] Add `ps-metrics/src/discourse.rs` with `compute_discourse_metrics`
- [ ] Wire into `compute_team_snapshot` in `ps-metrics/src/lib.rs`
- [ ] Map Discourse metrics from `raw_metrics` JSON into `TeamMetrics` proto fields
- [ ] Add new proto fields to `TeamMetrics`
- [ ] `buf generate`

### Step 2: Discourse metric cards + comparison columns
- [ ] Update `TeamMetricCards` with conditional Discourse row
- [ ] Add Discourse columns to `ComparisonTable`
- [ ] Wire through existing `useCompareTeams` data (no new RPCs)

### Step 3: Discourse activity RPC + leaf-team drilldown
- [ ] Add repo queries (category distribution, activity trend, top contributors)
- [ ] Add `GetDiscourseActivity` proto definition + RPC
- [ ] Implement `MetricsService::get_discourse_activity`
- [ ] `buf generate`, `cargo sqlx prepare`

### Step 4: Frontend Discourse activity section
- [ ] New `DiscourseActivitySection` component in `views/teams/components/`
- [ ] Activity trend chart (stacked area)
- [ ] Category distribution chart (horizontal bar)
- [ ] Top contributors table
- [ ] Instance tab filtering
- [ ] Hook for `GetDiscourseActivity`

### Step 5: Polish
- [ ] Empty states when no Discourse sources configured
- [ ] Loading skeletons for lazy-loaded section
- [ ] Tooltip descriptions for all new metric cards

## Dependencies

- Discourse ingestion (plan 13, phase 2 — **done**)
- Participation tracking with replies + likes (plan 35 — **done**)
- Identity resolution for linking Discourse users to team members (plan 36 — **in progress**, resolve-only store complete). Only contributions with a resolved `person_id` appear on team pages, so unresolved users are naturally excluded — not a blocker.

## Non-Goals

- Individual person Discourse profile (Phase 3)
- Cross-team Discourse leaderboards
- Discourse admin/moderation metrics (flags, deletions)
- Sentiment analysis on post content (Phase 3+ reasoning layer)
