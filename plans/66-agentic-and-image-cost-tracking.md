# Plan 66 — AI Usage Tracking for Agentic Queries & Image Generation

## Problem

The admin AI tab tracks usage from enrichment and embedding pipelines but is blind to two significant sources:

1. **Agentic chat queries** — token counts exist in `EventLoopResult` (provider-reported via OpenCode SSE) but are never written to `api_usage`.
2. **Image generation** — `ps-mcp` calls Google's image API with no tracking at all.

The existing cost estimation (hardcoded pricing table in `ps-reasoning/src/cost.rs`) is inherently inaccurate and unmaintainable — Google doesn't expose pricing via API. Real spend should be checked in the Google Cloud billing dashboard.

## Goals

- Log agentic query token counts and image generation request counts to `api_usage`
- Remove cost estimation — track tokens and requests only
- Simplify the admin AI tab to show Google config + usage stats on one clean page

---

## Proposed AI Tab Wireframe

```
┌─────────────────────────────────────────────────────────────┐
│  Configure AI provider and review usage.                    │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Google Gemini          🔑 Configured    [⚙] [🔌]  │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                             │
│  ─────────────────────────────────────────────────────────  │
│                                                             │
│  Usage                     [1w] [2w] [1m] [1q] [1y] [all]  │
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ Requests     │  │ Input tokens │  │ Output tokens│      │
│  │ 1,284        │  │ 4.2M         │  │ 812K         │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                             │
│  By task                                                    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │ Task              Input tokens  Output tokens  Reqs │    │
│  │ ──────────────────────────────────────────────────  │    │
│  │ ENRICHMENT          3,100,000       640,000    312  │    │
│  │ AGENTIC               980,000       150,000     47  │    │
│  │ EMBEDDINGS            120,000             —    820  │    │
│  │ IMAGE GENERATION            —             —     15  │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                             │
│  By model                                                   │
│  ┌─────────────────────────────────────────────────────┐    │
│  │ Model                    Task        Tokens   Reqs  │    │
│  │ ──────────────────────────────────────────────────  │    │
│  │ gemini-2.5-flash         enrichment  3.7M     312   │    │
│  │ gemini-2.5-flash         agentic     1.1M      47   │    │
│  │ text-embedding-004       embeddings  120K     820   │    │
│  │ gemini-3.1-flash-image…  image gen.    —       15   │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**Changes from current UI:**
- Remove all dollar values (stat cards, chart, table columns)
- Remove the daily spend bar chart (tokens over time is less useful than totals)
- Three stat cards: total requests, total input tokens, total output tokens
- Two tables stay but lose the cost column
- Token counts formatted with SI suffixes (4.2M, 812K) in stat cards, full numbers in tables

---

## Design

### 1. Add `ImageGeneration` task type

**File:** `crates/ps-core/src/models/enums.rs`

Add `ImageGeneration` variant with string value `"image_generation"`.

### 2. Log agentic usage in `finalize_query()`

**File:** `crates/ps-server/src/features/reasoning/agent_query/mod.rs`

After storing the assistant message, call `log_api_usage()` with provider/model/task/tokens from the already-accumulated `EventLoopResult`. Thread `model_name` into `finalize_query()`. Best-effort (don't fail the query).

### 3. Log image generation usage on tool completion

**File:** `crates/ps-server/src/features/reasoning/agent_query/artifact.rs`

When intercepting `prism_generate_image` completion, log an `api_usage` row with `task_type = "image_generation"`, tokens = 0 (image API doesn't report tokens), giving us request counts per model.

### 4. Remove cost estimation

- **Delete** `crates/ps-reasoning/src/cost.rs`
- **Simplify** `ReasoningRepo::log_api_usage()` — drop `estimated_cost_usd` param
- **Update** enrichment + embedding callers to use simplified signature
- **Migration** — drop `estimated_cost_usd` from `reasoning.api_usage`, drop `total_estimated_cost_usd` from `reasoning.conversations`
- **Simplify** repo query types (`TaskSpend`, `ModelSpend`, `DailySpend`) — remove cost fields

### 5. Simplify proto messages

**File:** `proto/canonical/prism/v1/reasoning.proto`

Remove `cost_usd` from `DailySpend`, `TaskSpend`, `ModelSpend`. Remove `today_spend_usd` from `GetCostSummaryResponse`. Rename RPC to `GetUsageSummary` (or keep name, just remove cost fields).

### 6. Rework admin UI

**File:** `frontend/views/admin/components/ai-cost-tab.tsx`

Replace with the wireframe above: three stat cards, two tables, no chart, no dollar values. Rename to `ai-usage-section.tsx`.

---

## Implementation Steps

1. Add `ImageGeneration` to `TaskType` enum
2. Migration — drop cost columns from `api_usage` and `conversations`
3. Delete `ps-reasoning/src/cost.rs`, simplify `log_api_usage()` and callers
4. Log agentic usage in `finalize_query()`
5. Log image usage in `artifact.rs`
6. Update proto — remove cost fields
7. Rework frontend usage section
8. Update sqlx cache + tests

## Files Changed

| File | Change |
|------|--------|
| `crates/ps-core/src/models/enums.rs` | Add `ImageGeneration` to `TaskType` |
| `crates/ps-core/src/repo/reasoning/api_usage.rs` | Drop cost param + cost fields from types/queries |
| `crates/ps-core/src/repo/reasoning/conversations.rs` | Remove cost from conversation totals |
| `crates/ps-reasoning/src/cost.rs` | **Delete** |
| `crates/ps-workers/src/features/reasoning/enrichment.rs` | Simplified `log_api_usage()` call |
| `crates/ps-workers/src/features/reasoning/embedding.rs` | Same |
| `crates/ps-server/src/features/reasoning/agent_query/mod.rs` | Log agentic usage |
| `crates/ps-server/src/features/reasoning/agent_query/artifact.rs` | Log image usage |
| `crates/ps-server/src/features/reasoning/cost.rs` | Simplify to usage-only |
| `proto/canonical/prism/v1/reasoning.proto` | Remove cost fields |
| `frontend/views/admin/components/ai-cost-tab.tsx` | Rewrite as usage section per wireframe |
| `migrations/NNNN_drop_cost_estimates.sql` | Drop cost columns |

## Risks

- **Cumulative token counts** — OpenCode emits running totals in `Part::StepFinish`. Verify multi-turn conversations don't double-count.
- **Image generation = request count only** — no token granularity, but that's accurate to what the API reports.
- **Proto breaking change** — removing cost fields breaks any external consumers. Deprecate if needed.
