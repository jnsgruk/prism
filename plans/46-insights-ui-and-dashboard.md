# Plan 46 — Insights UI & Dashboard

**Status:** Draft
**Depends on:** Plan 42 (enrichment queue), Plan 43 (cross-team contributions)
**Date:** 2026-03-19

## Context

Prism now has ~4,500 AI enrichments (review depth, sentiment, PR significance, Discourse topic classification) with 13k+ items queued. The enrichment pipeline is working and cost-effective ($0.000125/item). The team and individual views are information-dense with delivery, flow, and community metrics — but none of the enrichment data is surfaced beyond per-contribution badges.

The dashboard (`/`) is currently a placeholder. With enrichment data maturing and multiple teams ingesting, this is the right time to design both the dashboard and the insights integration into existing views.

The enrichment insights report (`reports/enrichment-insights.html`) demonstrates the kind of cross-cutting analysis that's possible: review depth by team, sentiment breakdowns, significance distributions, exemplary contributions with rationale. This plan brings that analysis into the live UI.

## Design Principles

1. **Insights are aggregations of enrichments, not new AI calls.** The UI queries pre-computed or on-the-fly aggregations of existing enrichment records. No LLM calls happen at page-load time.
2. **Every insight traces back to source data.** Aggregate numbers are clickable — they drill down to the contributions that produced them. Rationale text is always shown alongside scores.
3. **Insights respect the team hierarchy.** When viewing a parent team, insights aggregate across all descendant teams. Drilling into a child team narrows the scope. The comparison table gains enrichment columns.
4. **Graceful degradation.** Insights sections only appear when enrichment coverage is sufficient. A coverage indicator shows what percentage of contributions have been enriched, so users understand data completeness.
5. **Cross-source correlation.** Where possible, insights should connect data across platforms — e.g., linking PR significance with review depth, or Discourse participation with code contribution patterns.

---

## 1. Backend: Enrichment Aggregation APIs

New RPCs on a dedicated `InsightsService` (in a new `insights.proto`), separate from `ReasoningService`. Reasoning owns the enrichment pipeline (queue, processing, cost, admin). Insights owns read-only aggregation queries that consume enrichment data for UI presentation. This separation carries through the full stack: proto service, gRPC service impl, repository, and frontend hooks.

All queries are read-only against `reasoning.enrichments` joined with `activity.contributions` and `org` tables.

### 1.1 `GetTeamInsights`

```protobuf
message GetTeamInsightsRequest {
  string team_id = 1;
  string period = 2;            // "last_week", "last_month", etc.
  bool include_descendants = 3; // true for parent team roll-ups
}

message TeamInsights {
  // Coverage
  EnrichmentCoverage coverage = 1;

  // Review quality
  ReviewQualitySummary review_quality = 2;

  // PR impact
  SignificanceSummary pr_significance = 3;

  // Discourse content mix
  TopicCategorySummary discourse_topics = 4;

  // Notable items (highest-signal contributions)
  repeated NotableContribution notable_items = 5;
}

message EnrichmentCoverage {
  int32 total_contributions = 1;
  int32 enriched_contributions = 2;
  // Per-type breakdown
  repeated TypeCoverage by_type = 3;
}

message TypeCoverage {
  string enrichment_type = 1;
  int32 eligible = 2;
  int32 enriched = 3;
}

message ReviewQualitySummary {
  double avg_depth = 1;              // 1.0–5.0
  repeated int32 depth_distribution = 2; // [count_score1, ..., count_score5]
  int32 total_reviews = 3;
  double rubber_stamp_pct = 4;       // % scoring 1
  double deep_review_pct = 5;        // % scoring 4 or 5

  // Sentiment breakdown
  int32 constructive_count = 6;
  int32 neutral_count = 7;
  int32 critical_count = 8;
  int32 hostile_count = 9;

  // Top reviewers by depth (min N reviews threshold)
  repeated ReviewerDepth top_reviewers = 10;
}

message ReviewerDepth {
  string person_id = 1;
  string person_name = 2;
  int32 review_count = 3;
  double avg_depth = 4;
}

message SignificanceSummary {
  int32 significant_count = 1;
  int32 notable_count = 2;
  int32 routine_count = 3;
  double avg_confidence = 4;
}

message TopicCategorySummary {
  repeated CategoryCount categories = 1;
  int32 total_classified = 2;
}

message CategoryCount {
  string category = 1;
  int32 count = 2;
}

message NotableContribution {
  string contribution_id = 1;
  string title = 2;
  string url = 3;
  string person_name = 4;
  string platform = 5;
  string contribution_type = 6;
  string enrichment_type = 7;  // which enrichment flagged this
  string value_summary = 8;    // e.g. "Score 5 — thorough architectural review"
  string rationale = 9;
  double confidence = 10;
}
```

**Query strategy:** Join `reasoning.enrichments` → `activity.contributions` → `org.people` → `org.team_memberships`, filtered by team (with recursive CTE for descendants when `include_descendants = true`) and period. Aggregate in SQL with `GROUP BY` for distributions.

### 1.2 `GetPersonInsights`

```protobuf
message GetPersonInsightsRequest {
  string person_id = 1;
  string period = 2;
}

message PersonInsights {
  EnrichmentCoverage coverage = 1;

  // Their review quality when they review others
  ReviewerProfile reviewer_profile = 2;

  // Quality of reviews they receive on their PRs
  ReviewsReceivedSummary reviews_received = 3;

  // Impact of their PRs
  SignificanceSummary pr_impact = 4;

  // Their Discourse content
  TopicCategorySummary discourse_topics = 5;

  // Standout contributions
  repeated NotableContribution highlights = 6;
}

message ReviewerProfile {
  double avg_depth = 1;
  repeated int32 depth_distribution = 2;
  int32 total_reviews_given = 3;
  double rubber_stamp_pct = 4;

  // Sentiment of their reviews
  int32 constructive_count = 5;
  int32 neutral_count = 6;
  int32 critical_count = 7;
}

message ReviewsReceivedSummary {
  double avg_depth_received = 1;
  int32 total_reviews_received = 2;
  // How thorough is the feedback they're getting?
  double deep_review_pct = 3;
}
```

**Key insight for individuals:** We show both sides — how deep are *their* reviews of others, and how deep are the reviews *they receive* on their PRs. This is a uniquely useful cross-reference.

### 1.3 `GetOrgInsights` (for Dashboard)

```protobuf
message GetOrgInsightsRequest {
  string period = 1;
}

message OrgInsights {
  // High-level enrichment coverage
  EnrichmentCoverage coverage = 1;

  // Org-wide review quality
  ReviewQualitySummary review_quality = 2;

  // Team comparison (review depth + sentiment per team)
  repeated TeamReviewComparison team_comparison = 3;

  // PR significance org-wide
  SignificanceSummary pr_significance = 4;

  // Discourse content mix
  TopicCategorySummary discourse_topics = 5;

  // Cross-source highlights
  repeated NotableContribution org_highlights = 6;

  // Delivery summary (existing metrics, aggregated)
  OrgDeliverySummary delivery = 7;
}

message TeamReviewComparison {
  string team_id = 1;
  string team_name = 2;
  int32 review_count = 3;
  double avg_depth = 4;
  double rubber_stamp_pct = 5;
  int32 constructive_count = 6;
  int32 neutral_count = 7;
  int32 critical_count = 8;
}

message OrgDeliverySummary {
  int32 total_prs_merged = 1;
  int32 total_reviews = 2;
  int32 total_jira_closed = 3;
  int32 total_discourse_topics = 4;
  int32 total_discourse_posts = 5;
  int32 active_contributors = 6;
  int32 active_teams = 7;
}
```

### 1.4 Repository Layer — `InsightsRepo`

Insights aggregation lives in a new `InsightsRepo` in `ps-core/src/repo/insights.rs`, separate from `ReasoningRepo`. The reasoning repo owns enrichment pipeline mechanics (queue management, upsert, cost tracking, pipeline status). The insights repo owns read-only aggregation queries that consume enrichment data for UI presentation.

Both repos query the `reasoning` schema, but their concerns are distinct:

| Repo | Concern | Write? | Consumers |
|------|---------|--------|-----------|
| `ReasoningRepo` | Enrichment pipeline: queue, upsert, cost, status | Yes | `ps-reasoning` (enrichment handler), admin UI |
| `InsightsRepo` | Aggregation: distributions, comparisons, highlights | No (read-only) | `ps-server` (insights RPCs), dashboard/team/person views |

`InsightsRepo` is added to the `Repos` bundle and receives the same `PgPool`.

Methods on `InsightsRepo`:

| Method | Purpose |
|--------|---------|
| `get_review_quality_for_team(team_id, descendant_ids, since)` | Depth distribution, sentiment counts, top reviewers |
| `get_review_quality_for_person(person_id, since)` | Individual reviewer profile |
| `get_reviews_received_for_person(person_id, since)` | Reviews on their PRs |
| `get_significance_summary(team_id_or_person_id, since)` | PR classification counts |
| `get_topic_categories(team_id_or_person_id, since)` | Discourse topic breakdown |
| `get_notable_contributions(scope, since, limit)` | Highest-signal items (score 5 reviews, significant PRs) |
| `get_enrichment_coverage(scope, since)` | Coverage stats per enrichment type |
| `get_team_review_comparison(team_ids, since)` | Side-by-side review metrics for multiple teams |

All queries use the existing `reasoning.enrichments` → `activity.contributions` join path. Team scoping uses the recursive CTE pattern already established for team metrics.

### 1.5 Minimum Coverage Thresholds

To avoid misleading insights from sparse data, enforce minimum thresholds:

| Enrichment Type | Min for Section | Min for Comparison |
|----------------|-----------------|-------------------|
| review_depth | 10 reviews | 5 per team |
| sentiment | 10 reviews | 5 per team |
| significance | 5 PRs | 3 per team |
| topic | 5 topics | 3 per team |

Below threshold: show the section with a muted "Insufficient data — N of M contributions enriched" message and a progress indicator, rather than hiding it completely. This communicates that insights *will* appear as the pipeline catches up.

---

## 2. Team View — Insights Panel

### 2.1 Placement

Insert an **"Insights" section** between the existing Delivery Panel and Flow Panel. This positions it prominently without displacing the quantitative metrics that users already rely on.

```
PageHeader (breadcrumb, period selector)
├── Delivery Panel (throughput, review P75, active members, trend chart)
├── ★ Insights Panel (NEW)
├── Flow Panel (cycle time, WIP, lead time, flow efficiency)
├── Community Panel (Discourse metrics)
├── Comparison Table (child teams — now with enrichment columns)
├── Collapsible: Pull Requests
├── Collapsible: Reviews
├── Collapsible: Discourse Activity
└── Collapsible: Members
```

### 2.2 Insights Panel Layout

A `Card` with a `Sparkles` icon header ("Insights") and a coverage badge.

**Three sub-sections in a responsive grid:**

#### Review Quality (2/3 width on desktop)

- **Metric cards row:** Avg Depth (1–5 scale, colour-coded), Rubber-stamp % (red if >30%), Deep Review % (green if >20%), Sentiment split (constructive/neutral/critical as mini stacked bar)
- **Depth distribution histogram** — 5-bar chart (scores 1–5), matching the report style. Each bar is clickable → filters the Reviews collapsible section below to show reviews with that score.
- **Top reviewers mini-table** — Top 5 by avg depth (min 10 reviews), showing name (clickable → person profile), review count, avg depth. Only shown for teams with enough data.

#### PR Impact (1/3 width on desktop)

- **Significance donut or stacked bar** — significant / notable / routine counts with percentages.
- Only shown when ≥5 PRs have significance enrichments.

#### Discourse Content (1/3 width, or shares row with PR Impact)

- **Topic category tags** with counts — e.g., `tutorial (8)`, `question (4)`, `blog (5)`. Rendered as Badge components.
- Only shown when Discourse sources are configured AND ≥5 topics classified.

#### Notable Contributions (full width)

- 2–3 highlighted contributions with highest signal:
  - Score-5 reviews (deepest feedback)
  - Significant PRs (major changes)
  - Shown in the "quote" style from the report: person name, rationale text, confidence badge, link to contribution.
- Each links to the external source URL and shows the enrichment provenance (model, confidence) on hover via the existing `EnrichmentBadge` popover.

### 2.3 Dynamic Behaviour on Drill-Down

When navigating from a parent team to a child team:
- The Insights panel re-fetches with the new `team_id`
- Parent teams show `include_descendants = true` (aggregate across all children)
- Leaf teams show only their direct members' contributions
- The coverage indicator updates to reflect the narrower scope (a leaf team may have lower coverage than the org aggregate)

### 2.4 Comparison Table Enhancement

Add enrichment columns to the existing child-team comparison table:

| Existing Columns | New Columns |
|-----------------|-------------|
| Team Name, Throughput, Review P75, Cycle Time, Discourse Topics/Posts, Members | **Avg Review Depth**, **Rubber-stamp %** |

- New columns only appear when enrichment coverage is above the comparison threshold (5 reviews per child team)
- Depth values are colour-coded: green (≥2.8), amber (2.2–2.8), red (<2.2) — matching the report's visual language
- Columns are sortable, enabling quick identification of teams needing review culture attention

---

## 3. Individual View — Insights Panel

### 3.1 Placement

Insert between the existing Profile Metric Cards and the Activity Chart:

```
PageHeader (breadcrumb, period selector)
├── Profile Metric Cards (contributions, platforms, percentile, identities)
├── ★ Insights Panel (NEW)
├── Activity Chart by Platform
├── Peer Context Panel
├── Collapsible: Pull Requests
├── Collapsible: Reviews
├── Collapsible: Discourse
└── Collapsible: Identities
```

### 3.2 Insights Panel Layout

A `Card` with `Sparkles` icon header ("Insights") and coverage badge.

**Two-column layout:**

#### As a Reviewer (left column)

"How they review others' code"

- **Avg depth score** — large number (1–5), colour-coded
- **Mini depth distribution** — 5-bar sparkline histogram of their review scores
- **Rubber-stamp rate** — percentage with contextual colour
- **Sentiment breakdown** — constructive / neutral / critical counts as small badges
- **Comparison to team average** — "2.9 vs team avg 2.5" with delta indicator (arrow up/down)

#### Reviews They Receive (right column)

"Quality of feedback on their PRs"

- **Avg depth received** — how thorough is the feedback they get?
- **Deep review rate** — % of reviews scoring 4+
- This answers: "Is this person's code getting properly reviewed, or mostly rubber-stamped?"

#### PR Impact (below, full width if data exists)

- **Significance breakdown** — significant / notable / routine as badges with counts
- Only if the person has ≥3 PRs with significance enrichments

#### Highlights (below, full width)

- 1–2 standout contributions (their deepest review, their most significant PR) with rationale quotes
- Links to external source, enrichment provenance on hover

### 3.3 Peer Enrichment Context

Extend the existing Peer Context Panel to include enrichment-derived metrics alongside the existing throughput/cycle-time percentiles:

- **Review depth percentile** — "Your avg review depth is higher than 72% of peers at your level"
- **Rubber-stamp rate percentile** — contextualised against peers
- These use the same peer-comparison infrastructure (group by level, compute percentiles)

---

## 4. Dashboard View

The dashboard (`/`) becomes the org-wide view — a landing page showing health and insights across all teams.

### 4.1 Layout

```
PageHeader: "Dashboard" / "Organisation overview"
├── Team Selector + Period Selector (top bar)
├── Delivery Summary Cards (scoped KPIs)
├── Insights Summary (enrichment highlights)
├── Team Health Grid (compact child-team comparison)
└── Recent Activity (optional, lower priority)
```

**Team selector:** Uses the same searchable team dropdown pattern as the team page breadcrumb. Defaults to the first root team (showing org-wide roll-up). Selecting a team scopes all dashboard sections to that subtree. The URL reflects the selection: `/?team=<id>&period=last_month` via `useSearchParams()`. This mirrors the team page pattern — the dashboard is effectively a summary view that can be scoped to any level of the hierarchy.

### 4.2 Delivery Summary Cards

Top-level KPI cards in a 4–6 column grid:

| Card | Source | Notes |
|------|--------|-------|
| PRs Merged | `OrgDeliverySummary.total_prs_merged` | Period-scoped |
| Reviews Given | `OrgDeliverySummary.total_reviews` | Period-scoped |
| Jira Tickets Closed | `OrgDeliverySummary.total_jira_closed` | Hidden if no Jira sources |
| Discourse Topics | `OrgDeliverySummary.total_discourse_topics` | Hidden if no Discourse |
| Active Contributors | `OrgDeliverySummary.active_contributors` | Unique people with ≥1 contribution |
| Active Teams | `OrgDeliverySummary.active_teams` | Teams with ≥1 contribution |

Each card is clickable — navigates to the relevant view (e.g., clicking "PRs Merged" goes to the top team's PR section).

### 4.3 Insights Summary

An "Insights" card showing org-wide enrichment analysis:

#### Review Culture Health

- **Org avg review depth** (large number, 1–5)
- **Depth distribution histogram** (5-bar, org-wide)
- **Sentiment stacked bar** (constructive / neutral / critical / hostile)
- **Rubber-stamp rate** with trend context

#### PR Impact Distribution

- Significant / Notable / Routine breakdown as a stacked bar
- "X significant PRs this period" with links to the top 3

#### Coverage Indicator

- Progress bars per enrichment type showing pipeline completeness
- "85% of reviews enriched, 12% of PRs enriched" — sets expectations

### 4.4 Team Health Grid

A compact comparison of all root-level teams (or top N teams by headcount):

| Team | Throughput | Review P75 | Avg Depth | Rubber-stamp % | Sentiment |
|------|-----------|------------|-----------|----------------|-----------|
| Juju | 142 | 4.2h | 2.62 | 30% | ████░ (mostly constructive) |
| JIMM | 89 | 6.1h | 2.50 | 32% | ███░░ |

- Each row is clickable → navigates to `/teams/:teamId`
- Sortable by any column
- Colour-coded values using the same thresholds as the team view
- Mini sentiment bars (5-segment, same as enrichment report)
- This is essentially the report's "Team Review Culture" table, but live and interactive

### 4.5 Org-Wide Notable Contributions

A "Highlights" section showing 3–5 of the most notable contributions across the org in the selected period:

- Score-5 reviews with rationale quotes
- Significant PRs with impact descriptions
- Links to the contribution's external URL and the person's profile
- Enrichment provenance (model, confidence) accessible via popover

### 4.6 Empty/Onboarding State

Keep the existing onboarding card ("Get started with Prism → Configure Sources") when no sources exist. When sources exist but enrichment coverage is low (<10%), show the delivery summary cards with a banner: "AI insights are building up — X% of contributions enriched so far. Insights will appear here as the pipeline processes more data."

---

## 5. Traceability

Every aggregate number in the UI must support drill-down to source data:

| Insight | Drill-down |
|---------|------------|
| "Avg review depth: 2.62" | Click → filters Reviews section to show all reviews with their depth scores |
| "29% rubber-stamps" | Click → filters to score-1 reviews |
| "Score 5" bar in histogram | Click → shows those specific reviews |
| Top reviewer row | Click → navigates to person profile |
| Notable contribution quote | Click → opens external URL; hover → shows enrichment provenance popover |
| Team comparison row | Click → navigates to that team's view |
| Significance count | Click → filters PRs section to that classification |
| Topic category tag | Click → filters Discourse section to that category |
| Coverage indicator | Click → navigates to Admin > AI Pipeline status |

The existing `EnrichmentBadge` component with its provenance popover (model, confidence, input hash, input preview, created_at) remains the standard for per-contribution traceability. The new aggregate views add *statistical* traceability — showing the N that produced the aggregate, and allowing drill-down to those N items.

---

## 6. Frontend Components

### 6.1 New Shared Components

| Component | Location | Purpose |
|-----------|----------|---------|
| `InsightsPanel` | `components/insights-panel.tsx` | Shared wrapper: Card with Sparkles header, coverage badge, min-coverage gate |
| `DepthHistogram` | `components/depth-histogram.tsx` | 5-bar review depth chart, clickable bars |
| `SentimentBar` | `components/sentiment-bar.tsx` | Stacked horizontal bar (constructive/neutral/critical/hostile) |
| `SignificanceBreakdown` | `components/significance-breakdown.tsx` | Donut or stacked bar for PR classification |
| `NotableContribution` | `components/notable-contribution.tsx` | Quote-style card with rationale, person, link, provenance |
| `CoverageIndicator` | `components/coverage-indicator.tsx` | Per-type progress bars with counts |
| `CategoryTags` | `components/category-tags.tsx` | Discourse topic category badges with counts |

These live in `components/` (not `views/`) because they're shared across dashboard, team, and person views.

### 6.2 View-Specific Components

| Component | Location | Purpose |
|-----------|----------|---------|
| `TeamInsightsSection` | `views/teams/components/team-insights-section.tsx` | Composes shared components for team context |
| `PersonInsightsSection` | `views/people/components/person-insights-section.tsx` | Composes shared components for individual context |
| `TeamHealthGrid` | `views/dashboard/components/team-health-grid.tsx` | Sortable team comparison table |
| `OrgInsightsSummary` | `views/dashboard/components/org-insights-summary.tsx` | Dashboard insights card |
| `DeliverySummaryCards` | `views/dashboard/components/delivery-summary-cards.tsx` | Org-wide KPI cards |
| `OrgHighlights` | `views/dashboard/components/org-highlights.tsx` | Notable contributions across org |

### 6.3 Hooks

| Hook | Location | Purpose |
|------|----------|---------|
| `useTeamInsights(teamId, period)` | `views/teams/hooks/use-insights.ts` | Fetches `GetTeamInsights` |
| `usePersonInsights(personId, period)` | `views/people/hooks/use-insights.ts` | Fetches `GetPersonInsights` |
| `useOrgInsights(period)` | `views/dashboard/hooks/use-insights.ts` | Fetches `GetOrgInsights` |

All hooks use React Query with appropriate cache times (insights change slowly — 5-minute stale time is fine).

---

## 7. Implementation Sequence

### Phase A: Backend — Repository & Proto

1. **InsightsRepo** — Create `ps-core/src/repo/insights.rs` with aggregation queries (review quality, significance, topic categories, depth×significance cross-ref, notable contributions, coverage); add to `Repos` bundle
2. **Migration** — Create `reasoning.insight_snapshots` and `reasoning.insight_snapshot_sources` tables
3. **Proto definitions** — Create `insights.proto` with `InsightsService` defining `GetTeamInsights`, `GetPersonInsights`, `GetOrgInsights`
4. **Service implementation** — Create `InsightsService` in `ps-server/src/services/insights.rs`, wire through `InsightsRepo`
5. **`buf generate`** — Regenerate Rust + TypeScript types

### Phase B: Backend — Insights Handler

6. **Computation logic** — Create `ps-reasoning/src/features/insights/` module with `compute_team_insights()`, snapshot building, depth×significance correlation
7. **InsightsHandler** — Create `ps-workers/src/handlers/insights.rs` (Restate service handler, follows MetricsComputeHandler pattern)
8. **Handler registration** — Add to `HANDLER_DEFS`, bind in `ps-workers/src/main.rs`
9. **Enrichment → Insights trigger** — Add delayed invocation from enrichment handler completion to InsightsHandler

### Phase C: Team View Insights

10. **Shared components** — `DepthHistogram`, `SentimentBar`, `SignificanceBreakdown`, `NotableContribution`, `CoverageIndicator`, `InsightsPanel`, `DepthBySignificance`
11. **`TeamInsightsSection`** — Compose into team view between Delivery and Flow panels
12. **Comparison table columns** — Add avg depth + rubber-stamp % columns
13. **Drill-down wiring** — Click handlers that filter collapsible sections

### Phase D: Individual View Insights

14. **`PersonInsightsSection`** — Reviewer profile + reviews received + PR impact + highlights
15. **Peer context extension** — Add enrichment percentiles to existing peer panel

### Phase E: Dashboard

16. **Team selector** — Searchable dropdown (same pattern as team breadcrumb), URL-persisted
17. **`DeliverySummaryCards`** — Scoped KPIs
18. **`OrgInsightsSummary`** — Review culture + significance + coverage + depth×significance
19. **`TeamHealthGrid`** — Interactive child-team comparison with enrichment columns
20. **`OrgHighlights`** — Notable contributions across scope
21. **Empty/building state** — Coverage-aware messaging + onboarding

### Phase F: Trends & Polish

22. **Trend queries** — `InsightsRepo` methods to fetch consecutive snapshots for delta computation
23. **Trend UI** — Delta badges on aggregate values, optional sparklines
24. **Drill-down links** — All aggregate numbers link to filtered views
25. **Loading states** — Skeletons for insights sections
26. **Coverage threshold enforcement** — Graceful degradation when data is sparse
27. **Responsive layout** — Ensure insights panels work on narrow viewports

---

## 8. Data Dependencies & Enrichment Coverage

The value of this UI depends on enrichment pipeline throughput. Current state (19 Mar 2026):

| Type | Coverage | Needed for Full UI |
|------|----------|-------------------|
| review_depth | 31.8% (4,437/13,947) | ≥50% for reliable team comparisons |
| sentiment | 0.1% (20/13,947) | ≥20% for meaningful sentiment bars |
| significance | 0.5% (20/4,116) | ≥20% for useful PR impact sections |
| topic | 3.1% (20/645) | ≥30% for category distributions |

**Recommendation:** Prioritise running enrichment cycles to increase coverage before this UI ships. The UI should be built to handle partial coverage gracefully (Phase E, item 18), but the value proposition is much stronger with ≥50% coverage across types.

The enrichment pipeline processes ~4,400 items per run at $0.56. Three more full runs (~$5 total) would bring all types to near-complete coverage.

---

## 9. Review Depth × Significance Cross-Reference

A key correlation: are significant PRs getting deeper reviews? This cross-references two enrichment types on related contributions (a PR's significance enrichment linked to the review depth enrichments on reviews of that same PR).

### 9.1 Data Model

Reviews already carry `pr_platform_id` in their metrics, linking them to the PR they review. PRs have significance enrichments. The join path:

```
reasoning.enrichments (type=significance, on PR)
  → activity.contributions (the PR, via contribution_id)
  → activity.contributions (reviews of that PR, via pr_platform_id match)
  → reasoning.enrichments (type=review_depth, on those reviews)
```

### 9.2 Aggregation

New field on `TeamInsights` and `OrgInsights`:

```protobuf
message DepthBySignificance {
  double avg_depth_significant = 1;  // avg review depth on significant PRs
  double avg_depth_notable = 2;
  double avg_depth_routine = 3;
  int32 significant_review_count = 4;
  int32 notable_review_count = 5;
  int32 routine_review_count = 6;
}
```

### 9.3 UI

Shown in the Insights panel as a compact comparison:

| PR Classification | Avg Review Depth | Reviews |
|------------------|-----------------|---------|
| Significant | **3.1** | 42 |
| Notable | 2.6 | 89 |
| Routine | 2.1 | 156 |

The insight: "Significant PRs receive 48% deeper reviews on average" — or conversely, "Significant PRs are getting rubber-stamped at the same rate as routine changes" (a warning signal).

Only shown when both enrichment types have sufficient coverage on overlapping contributions (≥10 reviews across ≥5 PRs with significance enrichments).

---

## 10. Insights Snapshots & Trends

To track how insights evolve over time (e.g., "review depth improved from 2.3 to 2.6 over the last quarter"), the system periodically snapshots enrichment aggregates, following the same pattern as `metrics.team_snapshots`.

### 10.1 Schema

```sql
CREATE TABLE reasoning.insight_snapshots (
    id UUID PRIMARY KEY,
    team_id UUID NOT NULL REFERENCES org.teams(id),
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    period_type TEXT NOT NULL,  -- 'week', 'month', 'quarter'

    -- Review quality
    avg_review_depth REAL,
    review_count INTEGER,
    rubber_stamp_pct REAL,
    deep_review_pct REAL,
    depth_distribution INTEGER[],  -- [score1_count, ..., score5_count]

    -- Sentiment
    constructive_count INTEGER,
    neutral_count INTEGER,
    critical_count INTEGER,
    hostile_count INTEGER,

    -- PR significance
    significant_count INTEGER,
    notable_count INTEGER,
    routine_count INTEGER,

    -- Cross-reference
    avg_depth_on_significant REAL,
    avg_depth_on_notable REAL,
    avg_depth_on_routine REAL,

    -- Coverage at snapshot time
    enrichment_coverage JSONB DEFAULT '{}',

    -- Raw/overflow data
    raw_insights JSONB DEFAULT '{}',

    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (team_id, period_start, period_type)
);

CREATE TABLE reasoning.insight_snapshot_sources (
    snapshot_id UUID NOT NULL REFERENCES reasoning.insight_snapshots(id) ON DELETE CASCADE,
    enrichment_id UUID NOT NULL REFERENCES reasoning.enrichments(id) ON DELETE CASCADE,
    PRIMARY KEY (snapshot_id, enrichment_id)
);
```

**Key design decisions:**
- `UNIQUE (team_id, period_start, period_type)` allows idempotent recomputation (same as team_snapshots)
- `insight_snapshot_sources` links back to the enrichments that produced the aggregate — full traceability
- `enrichment_coverage` JSONB captures how complete the data was at snapshot time, so consumers can judge reliability
- `raw_insights` JSONB for overflow data (top reviewers, Discourse categories, etc.)

### 10.2 Trend UI

When historical snapshots exist, the Insights panel gains trend indicators:

- **Delta badges** next to aggregate values: "2.62 ↑0.09 vs last month"
- **Sparkline** (optional): tiny line chart showing avg depth over last 6 periods
- Trend data comes from querying `insight_snapshots` for the same team across consecutive periods

Trends are only shown when ≥2 snapshots exist for the same team and period type.

---

## 11. Insights Handler (Restate)

An `InsightsHandler` in `ps-workers` that periodically recomputes insight snapshots, following the same pattern as `MetricsComputeHandler`.

### 11.1 Handler Definition

```rust
#[restate_sdk::service]
pub trait InsightsHandler {
    async fn compute_current_periods() -> Result<(), TerminalError>;
}
```

Registered in `HANDLER_DEFS`:
```rust
HandlerDef {
    name: "InsightsHandler",
    methods: &["compute_current_periods"],
    description: "Recomputes insight snapshots from enrichment data for all teams across current periods",
    requires_key: false,
}
```

### 11.2 Execution Flow

```
TriggerHandler("InsightsHandler", "compute_current_periods")
  ↓
ctx.run("create_run") → activity.ingestion_runs record
  ↓
For each period_type in [Week, Month, Quarter]:
  ├── Compute period boundaries
  ├── For each team (buffer_unordered(4)):
  │   ├── Query enrichment aggregates via InsightsRepo
  │   ├── Compute depth×significance cross-reference
  │   ├── Build InsightSnapshotInput
  │   ├── Upsert to reasoning.insight_snapshots
  │   └── Update insight_snapshot_sources traceability
  └── Track items computed
  ↓
ctx.run("complete_run") → mark run complete
```

### 11.3 Key Differences from MetricsComputeHandler

| Aspect | MetricsComputeHandler | InsightsHandler |
|--------|----------------------|-----------------|
| Input data | Raw contributions | Enrichments (which reference contributions) |
| Output | `metrics.team_snapshots` | `reasoning.insight_snapshots` |
| Traceability | snapshot → contributions | snapshot → enrichments → contributions |
| Computation | Numeric aggregation (avg, p75, etc.) | Numeric aggregation + cross-type correlation |
| Dependencies | Needs contributions ingested | Needs contributions ingested AND enriched |
| Frequency | After each ingestion cycle | After enrichment cycles (less frequent) |
| Crate | `ps-metrics` | `ps-reasoning` (new module: `insights`) |

### 11.4 Scheduling

The InsightsHandler should run:
- **After enrichment cycles complete** — the enrichment handler could trigger it via Restate delayed invocation
- **On manual trigger** — via the Admin UI "Run" button (same pattern as metrics/enrichment)
- **Not on a fixed schedule** — insights are only stale when new enrichments have been produced

The enrichment handler's completion step can optionally fire a delayed invocation to `InsightsHandler.compute_current_periods()` with a short delay (e.g., 30 seconds), ensuring snapshots stay fresh without a separate cron.

### 11.5 Computation Logic Location

The insight computation logic lives in `ps-reasoning/src/features/insights/` (new module), parallel to `ps-reasoning/src/features/enrichment/`. This keeps the reasoning crate as the home for all AI-adjacent computation, while the handler in `ps-workers` orchestrates execution with Restate durability.

```
crates/ps-reasoning/src/features/
├── enrichment/    # Existing: LLM calls, prompt building, extraction
│   ├── mod.rs
│   ├── types.rs
│   └── prompts.rs
└── insights/      # New: aggregation, cross-reference, snapshot building
    ├── mod.rs     # compute_team_insights(), compute_all_snapshots()
    └── types.rs   # InsightSnapshotInput, DepthBySignificance, etc.
```

---

## 12. Resolved Decisions

| Question | Decision |
|----------|----------|
| Dashboard scope | Team selector at top (same dropdown pattern as team page), defaults to first root team, URL-persisted via `useSearchParams()` |
| Enrichment filtering on contribution tables | Not needed yet — defer to a future iteration |
| Trend over time | Yes — insight snapshots (§10) with delta badges and optional sparklines |
| Depth × significance cross-reference | Yes — implemented as a first-class aggregation (§9) |
| Insights recomputation | InsightsHandler in ps-workers (§11), triggered after enrichment cycles |
