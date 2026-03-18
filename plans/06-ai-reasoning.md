# AI & Reasoning Approach

## Philosophy

The spec identifies two modes of reasoning:

1. **Static metrics** — deterministic calculations (DORA, flow metrics, activity counts). These are reliable, reproducible, and fast. They form the foundation.

2. **AI-driven insights** — using LLMs to interpret data, spot patterns, and generate narrative observations. These are flexible and can adapt to new questions without code changes.

Both are valuable. The strategy is: **compute what you can deterministically, then use AI to interpret and extend.**

3. **Traceability** — every number, metric, or insight must be auditable. If an insight says "Team X's review depth has declined 30%", you need to be able to see *which reviews*, *what the scores were*, and *how the number was computed*. This is non-negotiable — these outputs can affect people's jobs, so manual validation must always be possible.

## Traceability & Provenance

Every output the system produces — whether a static metric or an AI-generated insight — must carry enough context to answer "how was this calculated?"

### For Static Metrics
- **Query reproducibility:** each metric snapshot records the query parameters (team, period, metric type) and the set of contributions that fed into it
- **Drill-down:** the UI should let you click a metric and see the underlying contributions (e.g. click "avg review turnaround: 18h" → see the individual reviews and their turnaround times)

### For AI Enrichments
- **Input/output logging:** each enrichment stores the model used, the input text (or a reference to it), and the raw output
- **Confidence scores:** where applicable, the enrichment includes a confidence score so low-confidence assessments can be flagged or filtered

### For AI-Generated Insights
- **Supporting evidence:** every insight links to the specific data points that support it — contribution IDs, metric snapshots, scan results
- **Reasoning trace:** the agent's chain of tool calls (what it queried, what it found) is stored alongside the insight, so you can see the path from question to answer
- **Model attribution:** which model produced the insight, and when

### In the UI
- Metrics always have a "show underlying data" affordance
- Insights have an expandable "how was this generated?" section showing the evidence and reasoning trace
- Enrichments (sentiment, depth scores) link back to the source content they were derived from

## Where AI Adds Value

### At Ingestion Time (Enrichment)

Low-latency, focused tasks run as data is ingested:

| Enrichment | Input | Output | When |
|------------|-------|--------|------|
| Review depth assessment | PR review comments | Score (1-5) + brief rationale | On every code review |
| Sentiment analysis | PR review comments | Sentiment label + score | On every code review |
| Topic classification | Discourse posts | Category tags | On every post |
| Contribution significance | PR title + description + diff stats | Impact estimate (routine/notable/significant) | On every PR |

These are best done with a **small, fast model** (e.g. Gemini 3.1 Flash Lite, or a comparable model via OpenRouter) or even a fine-tuned classifier where volume is high. The goal is to tag data as it arrives so it's queryable later.

**Cost control:** Not every contribution needs enrichment. Start with:
- All code reviews (high signal for "review depth" conversations)
- PRs above a threshold (e.g. >50 lines changed)
- Discourse posts that are topic-starters (not replies)

### At Query Time (Insights)

When a user asks "why is Team X's review turnaround increasing?" or "summarise this person's contributions this quarter", an LLM can:

1. Receive pre-computed metrics + relevant raw data
2. Generate a narrative explanation
3. Highlight anomalies or patterns

This is the **agentic** part of the system. The LLM doesn't just answer — it can request more data, run additional queries, and build up a picture.

### Periodic Insight Generation

Scheduled (e.g. weekly) generation of insights:
- "Team A merged 40% fewer PRs this month — but average PR size doubled"
- "Three people on Team B have no Discourse activity in 6 weeks despite being in a Discourse-heavy project"
- "Review turnaround for Team C has improved from 48h to 12h since last month"

These are stored in `reasoning.insights` and surfaced in the UI. Where insights are rendered as exportable artifacts (PDFs, formatted reports), the rendered output is stored in S3-compatible object storage and referenced via `artifact_key` — see [01-architecture-overview.md — Object Storage Strategy](./01-architecture-overview.md#object-storage-strategy).

## Embedding Strategy

### What to Embed

| Content | Embedding Model | Purpose |
|---------|----------------|---------|
| PR descriptions + review comments | Gemini Embedding 2 (default) | Cluster similar work, find related PRs |
| Discourse post bodies | Gemini Embedding 2 (default) | Topic clustering, similarity search |
| Jira ticket descriptions | Gemini Embedding 2 (default) | Cross-platform linkage (find the Jira ticket related to a PR) |

### What NOT to Embed (initially)
- Code diffs (too noisy, too large)
- Mailing list messages (low signal-to-noise for embedding)
- Google Drive content (access and format challenges)

### Storage
- `pgvector` in PostgreSQL — no separate vector DB needed at this scale
- Dimension: 1024 (Gemini Embedding 2 with MRL truncation from native 3072) — see Phase 3 plan for rationale
- IVFFlat index for approximate nearest neighbour search
- A few hundred thousand vectors is well within single-machine pgvector capacity

## Agentic Query Architecture

For the "flexible, adapt to changing data requirements" aspiration:

```
User asks a question in the UI
  │
  ▼
API server receives the question
  │
  ▼
Configured LLM (via ModelProvider trait) receives:
  - The question
  - Available tool descriptions (query metrics, search contributions,
    find similar items, get team info, etc.)
  - Context about the org structure
  │
  ▼
LLM decides what data it needs, calls tools
  │
  ▼
Tools execute queries against PostgreSQL
  │
  ▼
LLM synthesises an answer with supporting data
  │
  ▼
Response returned to UI with citations
```

### Available Tools for the Agent

The LLM agent would have access to a set of well-defined tools:

- `query_team_metrics(team_id, period)` — get pre-computed DORA/flow metrics
- `query_contributions(filters)` — search contributions with filters (person, team, platform, date range, type)
- `compare_teams(team_ids, period, metrics)` — side-by-side comparison
- `get_person_profile(person_id, period)` — individual activity summary
- `search_similar(contribution_id, limit)` — vector similarity search
- `query_enrichments(contribution_id)` — get AI-generated enrichments for a contribution
- `scan_repos(scope, question)` — schedule analysis containers to scan repositories for tool adoption, practices, or patterns
- `get_repo_scan_results(scan_id)` — retrieve results of a previous repo scan
- `run_in_container(repo, commands)` — execute commands in an ephemeral analysis container with a cloned repo
- `get_container_status(job_id)` — check status / stream logs from a running analysis job

This is **not a general-purpose SQL agent** — the tools are scoped and safe. Container tools are sandboxed via k8s resource limits and network policies.

### Repository Analysis via Containers

Repository scanning is a first-class use case for the agentic layer, backed by ephemeral k8s pods.

**Flow:**
1. User asks: "How many of our repos have migrated from tox to uv?"
2. Agent determines what signals to look for and what tools to run
3. API server schedules k8s Jobs — each pod clones a repo, runs analysis with real tools (`ripgrep`, `tokei`, etc.), and reports structured results
4. Results are stored in `repo_scans` / `repo_scan_results` for future reference and re-running. Raw output and logs are stored in object storage (`artifact_key` on `repo_scan_results`), structured findings in JSONB.
5. Agent aggregates and produces a summary with per-team breakdown

**Saved scan rules:** Frequently asked questions can be saved as named scan rules and re-run to track adoption over time (e.g. monthly "uv adoption" scans to see migration progress).

## Model Providers

The system must support **two providers from the start**:

### OpenRouter
- Unified API gateway to many models (Claude, Llama, Mistral, etc.)
- Single API key, flexible model selection
- Good for reasoning/enrichment tasks — pick the best model for the job without provider lock-in
- OpenAI-compatible API, well-supported in Rust via existing HTTP clients

### Google Gemini API (Default Provider)
- Gemini 3.1 series for reasoning and tool use — from Flash Lite (cheapest) to Pro (most capable)
- Gemini Embedding 2 — natively multimodal, 3072 dimensions with MRL truncation support
- Single API key covers all tasks (completions + embeddings), reducing credential management
- Also available via OpenRouter for teams that prefer a single aggregator

### Provider Abstraction

**Updated March 2026:** The provider layer is implemented using [Rig](https://github.com/0xPlaygrounds/rig) (`rig-core`), a mature Rust LLM framework with 20+ built-in providers. See [40-adopt-rig-framework.md](./40-adopt-rig-framework.md) for the full adoption plan.

Rig's `CompletionModel` and `EmbeddingModel` traits replace our hand-rolled `ModelProvider` trait. Individual provider HTTP implementations (Google Gemini REST API, OpenRouter OpenAI-compatible API) are handled by Rig — we do not maintain these ourselves. `ps-reasoning` holds a `TaskRouter` that selects Rig provider clients based on config-driven task routing.

Rig also provides agent orchestration (tool-call loop), structured extraction (typed output via `JsonSchema` derive), and streaming — all used in W1-W3. See the adoption plan for details.

Configuration determines which provider handles which task (stored in `config.global_settings`):

```toml
[ai.tasks]
enrichment = { provider = "google", model = "gemini-3.1-flash-lite" }
insights = { provider = "google", model = "gemini-3.1-pro" }
agentic = { provider = "google", model = "gemini-3-flash" }
embeddings = { provider = "google", model = "gemini-embedding-2" }
```

This keeps model selection flexible — swap models or providers per task without code changes.

## Model Selection

| Use Case | Default Provider | Default Model | Rationale |
|----------|-----------------|---------------|-----------|
| Enrichment at ingestion | Google | Gemini 3.1 Flash Lite | High volume, fast, cheapest current model |
| Periodic insight generation | Google | Gemini 3.1 Pro | Deep reasoning for actionable insights ($2/M input, $12/M output) |
| Agentic queries | Google | Gemini 3 Flash | Tool use, configurable thinking levels ($0.50/M input, $3/M output) |
| Embeddings | Google | Gemini Embedding 2 | 3072-dim with MRL truncation, $0.20/1M tokens |

All of these are configurable. If a new model becomes compelling (or pricing changes), update the config. OpenRouter is fully supported as an alternative provider.

## Implementation Phases

These align with the [system-wide implementation plan](./11-implementation-plan.md). AI capabilities are introduced in Phase 3:

### Phases 1–2: Static metrics only
- Compute DORA, flow, review metrics deterministically
- No AI involvement
- Validates the data pipeline end-to-end with multiple data sources

### Phase 3: Intelligence (enrichment + embeddings + agentic)
- Add sentiment and depth scoring for code reviews (enrichment at ingestion)
- Store enrichments in the database, surface in the UI
- Generate embeddings for PR descriptions and discourse posts
- Enable "find similar" and clustering features
- Cross-platform linkage (find the Jira ticket for this PR)
- Build the tool-use agent for natural language questions
- Add a query interface to the frontend

### Phase 4: Periodic insights
- Scheduled AI analysis producing actionable summaries
- Cross-platform correlation insights

## Cost Estimation

Rough numbers for a deployment tracking ~200 people across 6 sources:

- **Enrichment (Gemini 3.1 Flash Lite):** ~500-2000 contributions/day ≈ $0.10-0.50/day
- **Embeddings (Gemini Embedding 2):** ~500-2000 texts/day at $0.20/1M tokens ≈ ~$0.05/day
- **Insights (Gemini 3.1 Pro):** 1-2 runs/day ≈ $0.50-2/day
- **Agentic (Gemini 3 Flash):** ~5-20 queries/day ≈ $0.30-2/day
- **Total:** Roughly $1-5/day, or $30-150/month

Gemini's pricing is significantly lower than equivalent Anthropic/OpenAI models. The config-driven provider abstraction means switching to OpenRouter (for access to Claude, Llama, etc.) is a settings change if quality or pricing shifts.
