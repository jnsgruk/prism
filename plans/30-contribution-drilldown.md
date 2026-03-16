# Plan 30 — Contribution Drill-Down & Metric Traceability

## Problem

The teams page shows aggregate metrics (56 Merged PRs, 1.0h Review Turnaround P75) with no way to see what data produced them. A core principle of Prism is that every metric must be auditable back to source data, but currently the UI provides no "show how this was calculated" affordance.

Users need to:
1. Click a metric value and see the actual contributions behind it
2. Understand why a number is what it is (which PRs, which reviews, which people)
3. Identify outliers (the one 300-hour review dragging up the P75)
4. Verify correctness by cross-referencing with GitHub

## Current State

### What exists

- **`activity.contributions`** stores every PR and review with full metadata: title, URL, state, author (`person_id`), timestamps, JSONB `metrics` (additions, deletions, review_count, review_hours), JSONB `metadata` (repo, branch, labels, pr_number). Indexed on `(person_id, created_at DESC)`.
- **`metrics.team_snapshots`** stores aggregate numbers per team/period (throughput, avg_review_turnaround_hours, raw_metrics with p75/p90/p99). No link back to the contributions that produced them.
- **Metrics computation** ([ps-metrics/src/lib.rs](crates/ps-metrics/src/lib.rs)) reads contributions via `get_team_contributions()` which does a recursive team tree CTE + team_memberships join. The raw data is available — it's just not exposed to the frontend.
- **Teams page** shows stat cards (Merged PRs, Review Turnaround, Members, Active Contributors) and a child-team comparison table. Clicking a team navigates to its subtree. No click-through on metric values.
- **No RPC exists** for querying individual contributions.

### What's missing

- An RPC to list contributions filtered by team, period, and type
- Frontend components to display contribution lists
- Clickable metric values that open drill-down views
- Review turnaround distribution visibility (which reviews took how long)

## Design

### Data Model

No schema changes needed. `activity.contributions` already has everything. The drill-down is purely a read path — new query + RPC + frontend.

### New RPC: `ListTeamContributions`

```protobuf
message ListTeamContributionsRequest {
  string team_id = 1;
  Period period = 2;
  // Filter by contribution type: "pull_request", "pr_review", or empty for all.
  optional string contribution_type = 3;
  // Filter by state: "merged", "open", "closed", "APPROVED", etc.
  optional string state = 4;
  // Pagination
  int32 page_size = 5;
  string page_token = 6;
  // Sorting
  optional string sort_field = 7;   // "created_at", "title", "author", "review_hours"
  optional bool sort_desc = 8;
}

message Contribution {
  string id = 1;
  string person_name = 2;
  string platform = 3;
  string contribution_type = 4;
  string platform_id = 5;
  string title = 6;
  string url = 7;
  string state = 8;
  google.protobuf.Timestamp created_at = 9;
  google.protobuf.Timestamp closed_at = 10;
  // Flattened from JSONB metrics for convenience
  int32 additions = 11;
  int32 deletions = 12;
  int32 changed_files = 13;
  int32 review_count = 14;
  float review_hours = 15;         // hours from PR creation to first review
  // Repository info (extracted from platform_id or metadata)
  string repo = 16;
}

message ListTeamContributionsResponse {
  repeated Contribution contributions = 1;
  int32 total_count = 2;
  string next_page_token = 3;
}
```

Add to the `MetricsService`:

```protobuf
service MetricsService {
  // ... existing RPCs
  rpc ListTeamContributions(ListTeamContributionsRequest) returns (ListTeamContributionsResponse);
}
```

### Repository Method

New method on `MetricsRepo` (or `ActivityRepo` — either works, but MetricsRepo already does the team-tree CTE):

```rust
pub async fn list_team_contributions(
    &self,
    team_id: Uuid,
    period_start: Date,
    period_end: Date,
    contribution_type: Option<&str>,
    state: Option<&str>,
    sort_field: &str,
    sort_desc: bool,
    page_size: i32,
    offset: i32,
) -> Result<(Vec<ContributionDetailRow>, i64), Error>
```

Query structure:

```sql
WITH RECURSIVE team_tree AS (
    SELECT id FROM org.teams WHERE id = $1
    UNION ALL
    SELECT t.id FROM org.teams t JOIN team_tree tt ON t.parent_team_id = tt.id
)
SELECT c.id, p.name AS person_name, c.platform, c.contribution_type,
       c.platform_id, c.title, c.url, c.state,
       c.created_at, c.closed_at,
       c.metrics, c.metadata,
       COUNT(*) OVER() AS total_count
FROM activity.contributions c
JOIN org.team_memberships tm ON tm.person_id = c.person_id
    AND tm.team_id IN (SELECT id FROM team_tree)
    AND (tm.end_date IS NULL OR tm.end_date > $3::date)
    AND tm.start_date <= $3::date
JOIN org.people p ON p.id = c.person_id
WHERE c.created_at >= $2::date::timestamptz
  AND c.created_at < ($3::date + INTERVAL '1 day')::timestamptz
  AND ($4::text IS NULL OR c.contribution_type = $4)
  AND ($5::text IS NULL OR c.state = $5)
ORDER BY c.created_at DESC
LIMIT $6 OFFSET $7
```

### Frontend Components

#### 1. ContributionTable (shared component)

A reusable table for displaying contributions, built on `DataTable` + `DataTablePagination`:

```
views/teams/components/contribution-table.tsx
```

Columns:
| Column | Content |
|--------|---------|
| Title | PR/review title, linked to GitHub URL |
| Author | Person name |
| Repo | Extracted from platform_id (e.g. "juju/juju") |
| State | Badge (merged/open/closed/approved/changes_requested) |
| Created | Relative time + full timestamp |
| Stats | Additions/deletions for PRs, turnaround hours for reviews |

Filters (button groups, matching PeopleTab pattern):
- Type: All / PRs / Reviews
- State: All / Merged / Open / Closed

Pagination via `DataTablePagination` (server-side, cursor-based).

#### 2. Clickable Metric Values

Make the stat card values and table cells clickable. Clicking opens a sheet/panel (not a dialog — it should feel like navigation, not a modal):

```
views/teams/components/metric-drilldown-sheet.tsx
```

The sheet shows:
- Which metric was clicked (e.g. "Merged PRs" or "Review Turnaround P75")
- The ContributionTable pre-filtered to the relevant data
- For "Merged PRs": `contribution_type=pull_request, state=merged`
- For "Review Turnaround": `contribution_type=pr_review`, sorted by `review_hours DESC` so the slowest reviews are at the top — makes outliers immediately visible

#### 3. Team Metrics Cards (update)

Update [team-metric-cards.tsx](frontend/views/teams/components/team-metric-cards.tsx) to make values clickable:

```tsx
<button onClick={() => onDrillDown("throughput")}>
  <span className="text-3xl font-bold">{metrics.throughput}</span>
</button>
```

The parent page handles `onDrillDown` by opening the sheet with the appropriate filters.

#### 4. Child Team Table (update)

Make the "Merged PRs" and "Review P75" cells in the comparison table clickable too — clicking drills into that specific child team's contributions for the selected period.

### Review Turnaround Distribution

For the review turnaround metric specifically, the drill-down should show:

1. **Distribution histogram** — bucket review turnaround hours into ranges (< 1h, 1-4h, 4-8h, 8-24h, 24-72h, 72h+) and show a bar chart. This immediately reveals whether the P75 is driven by a few outliers or a systemic issue.

2. **Sorted review list** — all reviews sorted by turnaround time descending, showing:
   - Review author
   - PR title (linked)
   - Turnaround time
   - PR author (who waited)

This makes it trivial to spot "why is our P75 16 hours?" — you can see the 5 reviews that took 40+ hours.

## Implementation Order

### Phase 1: RPC + Repo (backend only)
- Add `ContributionDetailRow` struct
- Add `list_team_contributions()` to MetricsRepo
- Define proto messages (`Contribution`, `ListTeamContributionsRequest/Response`)
- Implement `ListTeamContributions` RPC in MetricsService
- `buf generate`
- Update sqlx cache

### Phase 2: Contribution Table (frontend, no drill-down yet)
- Create `ContributionTable` component using `DataTable` + `DataTablePagination`
- Create `useListTeamContributions()` hook
- Add a temporary route or tab to test the table in isolation

### Phase 3: Metric Drill-Down Sheet
- Create `MetricDrilldownSheet` component (slides in from right)
- Wire up `TeamMetricCards` click handlers
- Wire up comparison table cell clicks
- Pre-filter based on which metric was clicked

### Phase 4: Review Turnaround Distribution
- Add distribution histogram component (Tremor bar chart)
- Show in the drill-down sheet when Review Turnaround is clicked
- Sorted review list below the histogram

### Phase 5: Unresolved Identities
- Track skipped usernames during ingestion (accumulate in progress JSON)
- Show in run detail dialog: "These GitHub users contributed but aren't linked to anyone"
- Actionable: link to Admin > People to create the mapping

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Large teams + long periods = slow queries | Drill-down query returns thousands of rows | Server-side pagination (25/50/100 per page), team tree CTE is already indexed |
| `COUNT(*) OVER()` expensive on large result sets | Slow total count on first page | Use estimated count or cap at 10,000; exact count only needed for pagination display |
| Review turnaround computation differs between drill-down and snapshot | User sees inconsistent numbers | Both use same logic (first review timestamp - PR created_at); drill-down shows raw values, snapshot shows percentiles |
| Sheet feels heavy with table + chart | Cluttered UX | Start with table only (Phase 3), add histogram later (Phase 4); progressive disclosure |
