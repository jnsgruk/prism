# Plan 39 — Team Page UX Redesign: Clarity, Grouping, and Traceability

## Problem

The team page stacks too many things vertically with no visual hierarchy. Specific issues:

1. **Metric card wall** — up to 11 individual cards across 3 rows (4 GitHub + 4 DORA flow + 3 Discourse). They all look the same, so nothing stands out.
2. **No connection between numbers and source data** — "56 Merged PRs" is a static label. The expandable PR/review/Discourse sections exist below, but nothing links the card to the section.
3. **Mixed domains** — PR throughput, review turnaround, DORA flow metrics, and Discourse community metrics sit in identical cards with no grouping. A user has to read every label to find what they care about.
4. **Orphan charts** — Throughput Trend and WIP Trend float between the comparison table and the drill-down sections, unrelated to anything around them.
5. **Comparison chart problem** — the bar chart plots throughput (count) and review P75 (hours) on the same Y-axis, making neither readable.

### What works well

- The **expandable PR/Reviews/Discourse/Members sections** are good — progressive disclosure of source data.
- The **comparison table** for child teams is useful for manager-level overview.
- The **team selector, breadcrumb, and period selector** work fine.

The redesign preserves these and fixes the metrics area above them.

## Design

### Principle: grouped metric panels, not a card grid

Replace the flat grid of 11 identical cards with **2–3 themed panels** — each panel is a Card containing a cluster of related metrics, an optional inline chart, and a link to the relevant drill-down section below.

### Current layout (what changes)

```
[Period selector] [Breadcrumb]

[Card][Card][Card][Card]         ← 4 identical GitHub cards
[Card][Card][Card][Card]         ← 4 identical DORA cards (conditional)
[Card][Card][Card]               ← 3 identical Discourse cards (conditional)

[Comparison Table]               ← child teams
[Bar Chart: throughput + P75]    ← mixed-axis chart

[Throughput Trend chart]         ← orphan
[WIP Trend chart]                ← orphan, low value

▸ Pull Requests (56)             ← expandable (keep)
▸ Reviews (P75 1.2h)             ← expandable (keep)
▸ Discourse Activity (170)       ← expandable (keep)
▸ Members (12)                   ← expandable (keep)
```

### Proposed layout

```
┌─────────────────────────────────────────────────────────────┐
│  PageHeader: Team Name                    [Team Selector]   │
├─────────────────────────────────────────────────────────────┤
│  [1w] [2w] [1m] [1q] [1y] [all]                            │
│  Org > Group > Team                                         │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─ Delivery ──────────────────────────────────────────┐   │
│  │                                                      │   │
│  │   56          1.2h           8 / 12                  │   │
│  │   Merged PRs  Review P75    Active / Members         │   │
│  │               P90 4.8h                               │   │
│  │               P99 24.0h                              │   │
│  │                                                      │   │
│  │   Throughput Trend                                   │   │
│  │   ┌──────────────────────────────────────────────┐   │   │
│  │   │  ██  ████  ██████  ████  ██                  │   │   │
│  │   └──────────────────────────────────────────────┘   │   │
│  │                                                      │   │
│  │   "56 merged pull requests from 8 contributors.      │   │
│  │    75% of reviews completed within 1.2 hours."       │   │
│  │                                          See PRs →   │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─ Flow ─────────────────────  (only if data exists) ──┐  │
│  │                                                       │  │
│  │   3.2h           2.1          5.4h          62%       │  │
│  │   Cycle Time     WIP          Lead Time     Flow Eff  │  │
│  │                                                       │  │
│  │   "Average 3.2h from first commit to merge.           │  │
│  │    2.1 PRs open at any given time."                   │  │
│  └───────────────────────────────────────────────────────┘  │
│                                                             │
│  ┌─ Community ────────────────  (only if data exists) ──┐  │
│  │                                                       │  │
│  │   42              128             89                   │  │
│  │   Topics          Posts           Likes Given          │  │
│  │                   (96 replies)                         │  │
│  │                                                       │  │
│  │   "42 new topics and 128 posts across Discourse."     │  │
│  │                                    See activity →     │  │
│  └───────────────────────────────────────────────────────┘  │
│                                                             │
│  ┌─ Child Teams ─────────────────────────────────────────┐  │
│  │                                                        │  │
│  │  Team            Merged  Review P75  Cycle Time  Mbrs  │  │
│  │  ──────────────────────────────────────────────────── │  │
│  │  Alice's Squad      42     1.2h        3.0h        5  │  │
│  │  Bob's Squad        38     2.1h        4.5h        7  │  │
│  │                                                        │  │
│  │  Throughput by Team          Review P75 by Team        │  │
│  │  ┌──────────────────┐       ┌──────────────────┐      │  │
│  │  │  ██              │       │    ██            │      │  │
│  │  │  ████  ██        │       │  ████  ████      │      │  │
│  │  └──────────────────┘       └──────────────────┘      │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                             │
│  ▸ Pull Requests (56)                         (unchanged)   │
│  ▸ Reviews — P75 1.2h (142)                   (unchanged)   │
│  ▸ Discourse Activity (170)                   (unchanged)   │
│  ▸ Members (12)                               (unchanged)   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Change 1: Themed metric panels

Replace the flat card grid with **3 panel Cards**, each grouping metrics that belong together:

**Delivery panel** — the primary panel, always visible:
- Merged PRs (big number, clickable → scrolls to PR section)
- Review P75 with P90/P99 as secondary text
- Active Contributors / Total Members
- Throughput Trend chart (moved here from its orphan position)
- A plain-English summary sentence
- "See PRs →" link that scrolls to the Pull Requests section

**Flow panel** — conditional, only shown when DORA data exists:
- Cycle Time, WIP, Lead Time, Flow Efficiency
- A plain-English summary sentence
- No chart (WIP Trend removed — low signal)

**Community panel** — conditional, only shown when Discourse data exists:
- Topics, Posts & Replies, Likes Given
- Instance breakdown as secondary text (if multiple instances)
- "See activity →" link that scrolls to the Discourse section

**Why panels instead of individual cards:**
- Fewer visual elements to scan (3 panels vs 11 cards)
- Grouped by what the user is thinking about ("how's delivery going?" vs "how's community engagement?")
- Room for contextual content inside each panel (charts, summaries, links)

### Change 2: Plain-English metric summaries

Each panel includes a **one-sentence summary** that translates the numbers into a human-readable statement:

- Delivery: "56 merged pull requests from 8 contributors. 75% of reviews completed within 1.2 hours."
- Flow: "Average 3.2 hours from first commit to merge. 2.1 PRs open at any given time."
- Community: "42 new topics and 128 posts across Discourse."

These summaries are generated client-side from the metrics values. They make the page scannable without having to interpret each number individually.

### Change 3: Clickable metric values → scroll to source

Each headline number in a panel is a button that scrolls to the relevant expandable section and opens it:

| Click target | Action |
|---|---|
| "56 Merged PRs" | Scroll to Pull Requests section, expand it, filter to `state=merged` |
| "1.2h Review P75" | Scroll to Reviews section, expand it |
| "42 Topics" | Scroll to Discourse Activity section, expand it |
| "8 Active Contributors" | Scroll to Members section, expand it |

Implementation: each clickable value calls a handler that sets the section's open state to `true` and uses `scrollIntoView({ behavior: "smooth" })` on the section's ref.

### Change 4: Throughput Trend moves into the Delivery panel

The Throughput Trend bar chart belongs with the delivery metrics. It moves inside the Delivery panel Card, below the numbers and above the summary sentence. This eliminates one orphan section.

### Change 5: Remove WIP Trend chart

The WIP Trend line chart is removed. WIP as a number stays in the Flow panel — the trend over time doesn't provide enough insight to justify the space.

### Change 6: Split the comparison bar chart

The current "Throughput by Team" chart puts throughput (count) and review P75 (hours) on the same Y-axis. Replace with **two side-by-side charts**:

- **Throughput by Team** — bar chart, Y-axis = PR count
- **Review P75 by Team** — bar chart, Y-axis = hours

Use a `grid grid-cols-2 gap-4` layout within the comparison card.

### Change 7: Streamline comparison table columns

The comparison table currently shows up to 10 columns (Name, Throughput, Review P75, Cycle Time, WIP, Lead Time, Topics, Posts, Engagement, Members). Reduce the default visible columns to the most useful:

- Name, Throughput, Review P75, Cycle Time, Members

Discourse columns only appear if Discourse sources exist. WIP and Lead Time move to an overflow or are dropped from the comparison (they're visible per-team in the Flow panel when you drill into a child team).

## Component changes

| File | Change |
|---|---|
| `teams-page.tsx` | Replace `<TeamMetricCards>` + orphan trend charts with `<DeliveryPanel>`, `<FlowPanel>`, `<CommunityPanel>`. Add refs + scroll handlers for expandable sections. Remove `<WipTrendChart>`. |
| `team-metric-cards.tsx` | Delete. Replaced by the three panel components. |
| `trend-charts.tsx` | Remove `WipTrendChart` export. Move `ThroughputTrendChart` into `DeliveryPanel` (or keep as import). |
| `comparison-table.tsx` | Split bar chart into two charts. Remove WIP and Lead Time columns by default. |

### New components

| File | Purpose |
|---|---|
| `views/teams/components/delivery-panel.tsx` | Delivery metrics + throughput trend + summary + "See PRs" link |
| `views/teams/components/flow-panel.tsx` | DORA flow metrics + summary (conditional) |
| `views/teams/components/community-panel.tsx` | Discourse metrics + summary + "See activity" link (conditional) |

### No new hooks or backend changes

All data already exists via `useCompareTeams` and `useGetFlowMetrics`. This is a purely frontend layout change.

## Implementation order

1. **Create `DeliveryPanel`** — extract Merged PRs, Review P75, Members/Active from `TeamMetricCards`. Embed `ThroughputTrendChart` inside it. Add summary sentence. Add "See PRs →" scroll link.
2. **Create `FlowPanel`** — extract Cycle Time, WIP, Lead Time, Flow Efficiency. Add summary sentence.
3. **Create `CommunityPanel`** — extract Discourse metrics. Add "See activity →" scroll link.
4. **Wire up scroll-to-section** — add refs to the 4 expandable sections in `teams-page.tsx`. Pass scroll handlers to the panels.
5. **Split comparison chart** — replace the dual-bar chart with two side-by-side single-bar charts.
6. **Remove WIP Trend** — delete `WipTrendChart`, remove from page.
7. **Clean up** — delete `team-metric-cards.tsx`, remove unused exports from `trend-charts.tsx`.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Panels take more vertical space than the card grid | Each panel is compact — numbers in a row, chart below, summary sentence. Should be similar height to 2 rows of cards. |
| Summary sentences feel AI-generated or patronising | Keep them factual and terse. Template-based, not generative. Users who don't need them learn to skip them. |
| Scroll-to-section jarring on long pages | Use `scrollIntoView({ behavior: "smooth", block: "start" })` for animated scroll. |

## Out of scope

- Tabs or fundamentally different page structure — keep the single-scroll layout
- Sparklines inside metric panels — revisit later
- Cross-team comparison (selecting arbitrary teams) — separate feature
- PDF/export of team report — future enhancement
