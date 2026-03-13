# Phase 3: Intelligence — Detailed Implementation Plan

Phase 3 layers AI capabilities over the metrics foundation built in Phases 1-2. By this point, multiple data sources (GitHub, Jira, Discourse) are flowing, metrics are computed, and team/individual views exist in the UI.

**Exit criteria:** AI-enriched metrics with full traceability. Users can ask natural-language questions and get sourced, auditable answers.

**Code structure:** All new code follows feature-first organisation per [18-code-structure.md](./18-code-structure.md). The `ps-reasoning` crate uses `features/` internally (e.g. `features/enrichment/`, `features/embeddings/`, `features/agentic/`). Frontend AI features go in `views/insights/` or extend existing views (e.g. enrichment badges in `views/teams/`). Shared reasoning types belong in `ps-core` only when consumed by multiple crates.

**References:**
- [01 Architecture Overview](./01-architecture-overview.md) — system components, `ps-reasoning` crate
- [02 Domain Model](./02-domain-model.md) — Reasoning context (Enrichment, Insight, Embedding)
- [04 Database Design](./04-database-design.md) — `reasoning` schema tables
- [06 AI Reasoning](./06-ai-reasoning.md) — provider abstraction, tool design, cost model

---

## Workstreams

Phase 3 decomposes into three parallel workstreams with a shared foundation:

| # | Workstream | Description | Can start |
|---|-----------|-------------|-----------|
| **W0** | Provider Foundation & Object Storage | `ModelProvider` + `ArtifactStore` traits, provider implementations, S3 deployment, config, cost tracking | Immediately |
| **W1** | Enrichment Pipeline | AI-generated metadata on contributions (depth, sentiment, categorisation) | After W0 |
| **W2** | Embeddings & Similarity | Vector generation, pgvector indexing, similarity search API | After W0 |
| **W3** | Agentic Query Interface | Natural-language question interface with tool-use agent | After W0 + W1 partially |

```
W0: Provider Foundation & Object Storage ──┐
                                            ├── W1: Enrichment Pipeline
                                            ├── W2: Embeddings & Similarity
                                            └── W3: Agentic Interface (also depends on W1 for enrichment data)
```

W1 and W2 are fully parallel once W0 is done. W3 benefits from W1 and W2 data being available but can begin tool scaffolding in parallel.

---

## W0: Provider Foundation & Object Storage

### Deliverables

1. **`ModelProvider` trait** in `ps-reasoning` crate
2. **OpenRouter provider** implementation
3. **Google Gemini API provider** implementation
4. **Task-to-provider routing** — config-driven mapping of AI tasks to providers/models
5. **Cost tracking middleware** — log token usage per request, aggregate daily
6. **Rate limiting** — per-provider request throttling to stay within budget
7. **`ArtifactStore` trait and S3 deployment** — see [01-architecture-overview.md — Object Storage Strategy](./01-architecture-overview.md#object-storage-strategy)
   - `ArtifactStore` trait in `ps-core` wrapping `object_store` crate (Apache Arrow)
   - S3-compatible server deployed to K8s (evaluate RustFS vs Garage at deployment time — do not lock into MinIO)
   - `ArtifactKey` typed wrapper encoding artifact type + ID in the key path
   - K8s StatefulSet with PVC, HTTPRoute for pre-signed URL access
   - Tiltfile updated with object storage resource
   - In tests: `object_store::local::LocalFileSystem` — no S3 server needed
   - Bucket layout: `ps-artifacts/{insights,scans,conversations,cache}/`
8. **Admin UI: AI Settings tab** — new tab in the admin panel for AI provider configuration:
   - **Provider credentials:** password-style inputs for OpenRouter API key and Google API key, stored via `ConfigService.SetSecret` (secret keys: `openrouter_api_key`, `google_api_key`). Same encrypted `config.secrets` pattern as source credentials from Phase 1.
   - **Task-to-provider routing:** table showing each AI task (enrichment, insights, agentic, embeddings) with dropdowns for provider and model. Reads/writes `config.global_settings` under `ai.tasks`.
   - **Budget cap:** input for daily budget limit in USD (writes to `config.global_settings` under `ai.budget_cap_usd`).
   - **Test Provider button:** validates each provider's API key by making a minimal API call (e.g. a single-token completion). Shows success/failure status per provider.
   - **Object storage health:** status indicator showing whether the S3-compatible store is reachable.
9. **AI Cost Dashboard** — admin page (or section on the Ingestion Status page) showing:
   - Daily and weekly spend by task type and model (from `reasoning.api_usage`)
   - Budget utilisation bar (current day spend vs daily cap)
   - Alert banner when daily budget is exceeded or a provider is returning errors
   - Breakdown table: provider, model, task type, token counts, estimated cost
10. **Integration tests** — against live APIs with small payloads; artifact store tests against local filesystem backend

### ModelProvider Trait

The trait abstracts over providers so any task can use any backend:

```rust
#[async_trait]
trait ModelProvider: Send + Sync {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>;
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}
```

`CompletionRequest` carries: messages, model name, temperature, max tokens, optional tool definitions (for agentic use). `CompletionResponse` carries: content, token usage (prompt + completion), model name, finish reason.

### Provider Configuration

Task-to-provider routing is stored in `config.global_settings` under the `ai.tasks` key, matching the design in [06-ai-reasoning.md](./06-ai-reasoning.md):

| Task | Default Provider | Default Model | Rationale |
|------|-----------------|---------------|-----------|
| Enrichment | Google | `gemini-3.1-flash-lite` | High volume, fast, cheapest current Gemini model. Structured JSON output for scores/labels. |
| Insights | Google | `gemini-3.1-pro` | Deep reasoning for actionable team-level insights. Most capable Gemini model ($2/M input, $12/M output). |
| Agentic | Google | `gemini-3-flash` | Best balance of tool-use capability, speed, and cost ($0.50/M input, $3/M output). Configurable thinking levels — dial up for complex queries, dial down for simple lookups. |
| Embeddings | Google | `gemini-embedding-2` | Natively multimodal, 3072 dimensions (scalable via MRL), replaces text-embedding-004. |

Google Gemini is the default provider because: (a) a single API key covers all tasks including embeddings, reducing credential management; (b) the free tier and low pricing make development and experimentation cheap; (c) the model lineup covers the full range from high-volume/low-cost (`flash-lite`) to deep reasoning (`pro`). OpenRouter remains fully supported as an alternative — swapping a model or provider is a config change, not a code change. Teams with existing OpenRouter credits or a preference for Anthropic/OpenAI models can switch via the AI Settings admin tab.

**Note on model freshness:** The Gemini model lineup evolves rapidly. The models listed above are current as of March 2026. The 2.x series is being deprecated (2.5 Pro/Flash by June 2026, 2.0 by June 2026). The config-driven routing means updating to newer models is a settings change, not a code change.

**Provider API keys** are stored in `config.secrets` using the same encrypted-at-rest pattern established in Phase 1 for source credentials:

| `secret_key` | Value |
|--------------|-------|
| `openrouter_api_key` | OpenRouter API key |
| `google_api_key` | Google AI API key |

These are global secrets (not tied to a `source_id`). The `config.secrets` table supports this via a `NULL` `source_id` for global secrets. Set via the AI Settings admin tab using `ConfigService.SetSecret(source_id=NULL, "openrouter_api_key", ...)`.

### Cost Tracking

Every API call logs:

| Field | Purpose |
|-------|---------|
| `provider` | Which provider was called |
| `model` | Which model was used |
| `task_type` | enrichment / insight / embedding / agentic |
| `prompt_tokens` | Input token count |
| `completion_tokens` | Output token count |
| `estimated_cost_usd` | Computed from known per-token pricing |
| `timestamp` | When the call was made |

A `reasoning.api_usage` table stores this. A simple dashboard widget shows daily/weekly spend and alerts if the daily budget is exceeded.

### Database Addition

```sql
CREATE TABLE reasoning.api_usage (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    task_type TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_api_usage_daily
    ON reasoning.api_usage(task_type, created_at DESC);
```

---

## W1: Enrichment Pipeline

### Deliverables

1. **Enrichment scheduler** — processes un-enriched contributions on a schedule
2. **Review depth analyser** — scores code reviews 1-5 with rationale
3. **Sentiment analyser** — labels review tone (constructive, neutral, critical, hostile)
4. **Contribution categoriser** — tags PRs as routine/notable/significant
5. **Topic classifier** — categorises Discourse posts
6. **Enrichment API** — gRPC endpoints to query enrichments
7. **Enrichment UI** — enrichment badges/scores visible on contributions in the frontend (see [UI integration points](#enrichment--similarity-ui-integration-points) below)
8. **Traceability display** — every enrichment shows its provenance in the UI
9. **AI Pipeline Status section** — added to the existing Ingestion Status page, showing:
   - Enrichment pipeline: pending count (contributions awaiting enrichment), last run time, error count, throughput rate (enrichments/hour)
   - Budget status: current day spend vs daily cap, with pause indicator if cap is hit
   - Per-enrichment-type breakdown: how many review depth, sentiment, significance, and topic classifications have been produced

### What Gets Enriched

Not everything. Volume control is critical for cost:

| Contribution Type | Enrichment(s) | Filter |
|-------------------|---------------|--------|
| Code reviews (GitHub) | Depth score (1-5), sentiment | All reviews |
| Pull requests (GitHub) | Significance (routine/notable/significant) | PRs with >50 lines changed |
| Discourse topics | Topic classification tags | Topic starters only (not replies) |
| Jira tickets | None initially | Revisit after seeing data patterns |

### Enrichment Scheduling

The enrichment pipeline runs as a **periodic background task** (every 30-60 minutes), separate from ingestion:

1. Query `activity.contributions` for rows that lack a corresponding `reasoning.enrichments` entry
2. Filter by the rules above (contribution type, thresholds)
3. Batch into groups of 10-20 (to amortise API call overhead where batching is supported)
4. Call the configured model via `ModelProvider`
5. Parse structured output, store in `reasoning.enrichments`
6. Log usage to `reasoning.api_usage`

The scheduler is idempotent — if interrupted, it picks up un-enriched contributions on the next run.

### Prompt Design Principles

- Prompts request **structured JSON output** with explicit fields (score, label, rationale)
- Each prompt includes 2-3 few-shot examples calibrated to the Canonical engineering context
- Prompts are versioned and stored as constants in the codebase (not in the database) so changes are tracked in git
- The `enrichment_type` field distinguishes prompt versions; if we change a prompt significantly, we use a new type name and can re-enrich

### Enrichment Schema

Already defined in [04-database-design.md](./04-database-design.md) — the `reasoning.enrichments` table. Key traceability columns:

| Column | Purpose |
|--------|---------|
| `model_name` | Exactly which model produced this |
| `input_hash` | SHA-256 of input text — detect if source content changed |
| `input_preview` | First ~500 chars of input for quick audit without re-fetching |
| `confidence` | Model's self-reported confidence where applicable |
| `value` | Structured JSON result (score, label, rationale) |

### Re-enrichment

If a prompt is improved or a model is upgraded, we need to re-enrich. Strategy:

- Delete enrichments of the old type, let the scheduler backfill
- The `input_hash` column lets us skip re-enrichment if the source content hasn't changed and we're happy with the current result
- Never silently overwrite — re-enrichment is an explicit admin action

---

## W2: Embeddings & Similarity

### Deliverables

1. **Embedding generation pipeline** — batch process contributions into vectors
2. **pgvector setup** — migration to enable extension, create indexes
3. **Similarity search API** — "find contributions similar to X"
4. **Cross-platform linkage** — find the Jira ticket related to a PR (or vice versa)
5. **Similarity UI** — "related items" panel on contribution detail views (see [UI integration points](#enrichment--similarity-ui-integration-points) below)
6. **Embedding pipeline status** — added to the AI Pipeline Status section (from W1 deliverable 9): pending count (contributions awaiting embedding), last run time, coverage percentage (embedded / total eligible), error count

### What Gets Embedded

| Content | Source Text | Purpose |
|---------|------------|---------|
| Pull requests | Title + description (concatenated) | Find similar PRs, cluster work themes |
| Code reviews | Review body text | Find similar review patterns |
| Discourse topics | Topic title + first post body | Topic clustering, cross-platform linkage |
| Jira tickets | Summary + description | Cross-platform linkage with PRs |

**Not embedded initially:** code diffs (too noisy/large), mailing list messages (low signal), Google Drive content (not yet integrated).

### pgvector Setup

Migration `XXXX_enable_pgvector.sql`:

```sql
CREATE EXTENSION IF NOT EXISTS vector;
```

The `reasoning.embeddings` table is already defined in [04-database-design.md](./04-database-design.md). The IVFFlat index parameters need tuning based on actual data volume:

| Parameter | Starting Value | Rationale |
|-----------|---------------|-----------|
| Dimensions | 3072 (or lower via MRL) | Gemini Embedding 2 native output dimension. MRL (Matryoshka Representation Learning) allows truncating to 768/1024/1536 for space/speed trade-off. Start with 1024 as a balance. |
| Index type | IVFFlat | Good enough for <1M vectors, simpler than HNSW |
| Lists | 100 | Reasonable for up to ~100k vectors; increase if volume grows |
| Distance metric | Cosine | Standard for text similarity |

Gemini Embedding 2 supports MRL, meaning the 3072-dimensional output can be truncated to a smaller dimension (768, 1024, 1536) with minimal quality loss. Start with 1024 dimensions for a good balance of quality and pgvector performance. If we switch embedding models, we would re-generate all embeddings and update the column dimension — this is a migration, not a config change.

### Embedding Generation Pipeline

Similar cadence to enrichment — runs every 30-60 minutes:

1. Query contributions that have content but no embedding row
2. Batch texts (Google's API supports up to 2048 texts per call)
3. Call `ModelProvider::embed()`
4. Store vectors in `reasoning.embeddings`
5. Log usage to `reasoning.api_usage`

Gemini Embedding 2 pricing is $0.20/1M tokens for text input. At our scale (a few hundred new contributions/day), embedding cost should be negligible — well under $1/month.

### Similarity Queries

Two primary use cases:

**1. "Find similar" from a specific contribution:**

```sql
SELECT c.id, c.title, c.platform, c.contribution_type,
       e.embedding <=> target_embedding AS distance
FROM reasoning.embeddings e
JOIN activity.contributions c ON c.id = e.contribution_id
WHERE e.embedding <=> target_embedding < 0.3
ORDER BY e.embedding <=> target_embedding
LIMIT 10;
```

**2. Cross-platform linkage** — given a PR, find related Jira tickets:

Same query but filtered to `c.platform = 'jira'`. This enables the "show me the Jira ticket for this PR" use case without requiring explicit links in the source data.

### Similarity API

gRPC endpoints:

- `FindSimilar(contribution_id, limit, platform_filter)` — returns ranked similar contributions
- `SearchByText(query_text, limit, platform_filter)` — embed the query text on-the-fly and search

---

## W3: Agentic Query Interface

### Deliverables

1. **Agent tool definitions** — typed Rust functions the LLM can call
2. **Agent orchestration loop** — send question, let LLM call tools, collect answer
3. **Conversation API** — gRPC endpoints for the query interface
4. **Conversation UI** — chat-style interface in the frontend
5. **Reasoning trace storage** — full audit trail of agent tool calls
6. **Insight persistence** — save notable agent-generated insights
7. **Artifact generation** — rendered reports (PDF/Markdown) stored via `ArtifactStore`, referenced by `artifact_key` on `reasoning.insights`. Pre-signed URLs for frontend download.

### Agent Tools

The agent has access to a scoped set of read-only tools (not raw SQL). Each tool is a Rust function with a typed schema the LLM sees:

| Tool | Input | Output |
|------|-------|--------|
| `query_team_metrics` | team_id, period_start, period_end | DORA + flow + review metrics |
| `query_contributions` | filters (person, team, platform, type, date range) | List of contributions with key fields |
| `compare_teams` | team_ids[], period, metric_names[] | Side-by-side metric comparison |
| `get_person_profile` | person_id, period | Activity summary across platforms |
| `search_similar` | contribution_id, limit | Vector similarity results |
| `search_by_text` | query_text, limit, platform_filter | Text-to-vector search |
| `query_enrichments` | contribution_id | Enrichment scores and rationale |
| `list_teams` | org_name (optional) | Team hierarchy |
| `list_people` | team_id (optional) | People with team membership |

Repository analysis tools (`scan_repos`, `run_in_container`) from [06-ai-reasoning.md](./06-ai-reasoning.md) are included but gated behind an explicit user confirmation step in the UI — they schedule real k8s workloads.

### Agent Orchestration

The orchestration loop in `ps-reasoning`:

1. User submits a question via the conversation API
2. Server constructs a system prompt with: available tools, org context (team names, current period), instructions to cite sources
3. Server calls `ModelProvider::complete()` with tool definitions
4. If the model returns tool calls, execute them against the database, return results
5. Repeat steps 3-4 until the model produces a final text response (max 10 iterations as a safety bound)
6. Return the response to the frontend along with the reasoning trace

The entire trace (each tool call, its arguments, its results, and the model's intermediate reasoning) is captured and stored.

### Conversation API

```protobuf
service QueryService {
    rpc AskQuestion(AskQuestionRequest) returns (stream AskQuestionEvent);
}
```

Streaming response so the UI can show progress as the agent works. Event types:

| Event | Content |
|-------|---------|
| `ToolCallStarted` | Tool name, arguments (so user sees what the agent is doing) |
| `ToolCallCompleted` | Tool name, summary of result |
| `PartialAnswer` | Incremental text from the model |
| `FinalAnswer` | Complete answer text + supporting data references |
| `Error` | If something went wrong |

### Conversation UI

- Chat-style panel, accessible from the main navigation
- Pre-seeded suggested questions based on current team context ("How has Team X's review quality changed this quarter?")
- While the agent works, show a live feed of tool calls (collapsible)
- Final answer rendered as Markdown with inline citations
- Each citation links to the source contribution or metric snapshot
- "How was this generated?" expandable section showing the full reasoning trace
- Option to save an answer as a named insight (stored in `reasoning.insights`)

### Iteration Limits and Safety

- Max 10 tool-call iterations per question
- Max 60-second wall-clock timeout for the full agent loop
- If the agent cannot answer, it says so rather than hallucinating
- Container-scheduling tools require explicit user confirmation before execution

---

## Traceability Requirements

This is the non-negotiable constraint of Phase 3. These outputs can affect people's jobs — every AI-generated claim must be manually verifiable.

### Enrichments Traceability

| Requirement | Implementation |
|-------------|---------------|
| Know what the model saw | `input_hash` (SHA-256) + `input_preview` (first 500 chars) on every enrichment |
| Know which model produced it | `model_name` column (e.g. `openrouter/anthropic/claude-3.5-haiku`) |
| Know the raw output | `value` JSONB stores the complete structured response including rationale |
| Know when it was produced | `created_at` timestamp |
| Reproduce the result | Same input (verified by hash) + same model + same prompt version = same output (modulo temperature) |
| Manual override | Future: allow a human to override an enrichment score with a flag indicating manual review |

### Insights Traceability

| Requirement | Implementation |
|-------------|---------------|
| Know the evidence | `supporting_data` JSONB: array of `{type, id}` references to contributions, metric snapshots, enrichments |
| Know the reasoning path | `reasoning_trace` JSONB: ordered list of `{tool, args, result_summary, timestamp}` for each agent step |
| Know the model | `model_name` column |
| Verify the evidence | Every reference in `supporting_data` is a real ID that can be fetched and displayed |

### UI Traceability Affordances

- **Enrichment badges** (e.g. review depth score) are clickable. Clicking shows: the score, the rationale, the model used, a preview of the input text, and a link to the full source contribution.
- **Metric values** that incorporate enrichment data (e.g. "avg review depth: 3.2") show a breakdown of individual enrichment scores on click.
- **Agent answers** have an expandable "Evidence & Reasoning" section that shows:
  - Each tool the agent called, with arguments and results
  - The specific contributions, metrics, or enrichments cited
  - Links to view each cited item in full
- **No black boxes.** If something cannot be traced, it is a bug.

---

## Database Schemas

Phase 3 uses the `reasoning` schema tables defined in [04-database-design.md](./04-database-design.md), plus one addition:

### Existing Tables (from 04)

- `reasoning.embeddings` — vector storage per contribution
- `reasoning.enrichments` — AI-generated metadata per contribution
- `reasoning.insights` — AI-generated observations with evidence

### New Tables for Phase 3

**API usage tracking:**

```sql
CREATE TABLE reasoning.api_usage (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    task_type TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

**Conversation history** (for the agentic query interface):

```sql
CREATE TABLE reasoning.conversations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    question TEXT NOT NULL,
    answer TEXT,
    reasoning_trace JSONB,
    supporting_data JSONB,
    model_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'processing',
    token_usage JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);
```

**Saved insights** (when a user saves an agent answer):

The existing `reasoning.insights` table covers this. A `conversation_id` foreign key can optionally link an insight back to the conversation that produced it:

```sql
ALTER TABLE reasoning.insights
    ADD COLUMN conversation_id UUID REFERENCES reasoning.conversations(id);
```

---

## Cost Management

### Budget Envelope

Target: **$1-5/day** ($30-150/month) for a deployment tracking ~200 people across 6 sources. Gemini's pricing is significantly lower than equivalent Anthropic/OpenAI models, and the free tier covers embeddings entirely.

| Task | Volume | Model | Estimated Daily Cost |
|------|--------|-------|---------------------|
| Enrichment | ~500-2000 contributions/day | Gemini 3.1 Flash Lite | $0.10-0.50 |
| Embeddings | ~500-2000 texts/day | Gemini Embedding 2 | ~$0.05 ($0.20/1M tokens) |
| Insights | 1-2 runs/day (weekly team + on-demand) | Gemini 3.1 Pro | $0.50-2 |
| Agentic queries | ~5-20 queries/day | Gemini 3 Flash | $0.30-2 |
| **Total** | | | **$0.95-4.55** |

### Cost Controls

1. **Daily budget cap** — configurable in `config.global_settings`. The enrichment scheduler checks cumulative daily spend before processing a batch. If the cap is hit, enrichment pauses until the next day.

2. **Selective enrichment** — not every contribution is enriched. The filters described in W1 (review type, PR size threshold, topic starters only) keep volume manageable.

3. **Batch efficiency** — group enrichment requests to reduce per-call overhead. Where the model supports it, send multiple items in one prompt.

4. **Cheap embeddings** — Gemini Embedding 2 at $0.20/1M tokens means embedding cost is negligible at our volume.

5. **Usage dashboard** — the `reasoning.api_usage` table powers a simple admin view showing daily spend by task type and model. This is visible on the ingestion/admin status page.

6. **Model downgrade path** — if costs rise, switch enrichment to a cheaper model (or disable non-critical enrichment types) via config change.

---

## Dependencies Between Workstreams

```
Week 1-2:  W0 (Provider Foundation & Object Storage)
           ├── ModelProvider trait + implementations
           ├── ArtifactStore trait + S3 deployment
           ├── OpenRouter + Google Gemini providers
           └── Config + cost tracking

Week 2-4:  W1 (Enrichment)          W2 (Embeddings)
           ├── Review depth          ├── pgvector migration
           ├── Sentiment             ├── Generation pipeline
           ├── Categorisation        ├── Similarity API
           ├── Scheduler             └── Similarity UI
           └── Enrichment UI

Week 3-5:  W3 (Agentic Interface)
           ├── Tool definitions (can start Week 2)
           ├── Orchestration loop (needs W0)
           ├── Conversation API
           ├── Conversation UI
           └── Reasoning trace storage

Week 5-6:  Integration & Polish
           ├── Cross-workstream testing
           ├── Traceability audit (verify every output is traceable)
           ├── Cost monitoring validation
           └── Documentation
```

### Key Dependency Chains

1. **W0 must complete before W1/W2/W3 can call AI providers** — but tool definitions (W3) and database migrations (W2) can proceed in parallel.
2. **W1 enrichment data improves W3 agent answers** — the agent can reference enrichment scores. W3 is more useful with W1 data available, but not blocked by it.
3. **W2 embeddings enable W3 similarity tools** — `search_similar` and `search_by_text` tools need embeddings to exist. W3 can start without these tools and add them when W2 is ready.
4. **pgvector migration (W2) is a prerequisite for embedding storage** — but is a simple migration that can run early.

---

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Object storage server | RustFS or Garage (decide at deploy time) | S3-compatible, single binary, not MinIO. `object_store` crate abstracts the backend. |
| Embedding model dimensions | 1024 (Gemini Embedding 2 via MRL truncation from native 3072) | Good balance of quality and pgvector performance; MRL allows adjusting without re-embedding |
| Enrichment scheduling | Background job, 30-60 min cadence | Decoupled from ingestion to avoid blocking data collection |
| Enrichment at ingestion vs post-ingestion | Post-ingestion | Keeps ingestion simple and fast; enrichment is separate concern |
| Agent tool design | Scoped read-only functions, not raw SQL | Safety — the agent cannot modify data or run arbitrary queries |
| Agent iteration limit | 10 tool calls, 60s timeout | Prevents runaway cost and latency |
| Conversation streaming | Server-sent events via gRPC server streaming | User sees progress as agent works |
| IVFFlat vs HNSW for pgvector | IVFFlat initially | Simpler, sufficient for <1M vectors; migrate to HNSW if needed |
| Re-enrichment strategy | Delete old + backfill, never silent overwrite | Auditability — you always know which prompt/model version produced a result |

---

## `psctl` Extensions

Phase 3 adds AI capabilities. `psctl` gains subcommands for triggering enrichment, inspecting cost, and interacting with the query interface:

| Command | Description | Backing RPC |
|---------|-------------|-------------|
| `psctl enrich [--type TYPE] [--since DATE]` | Trigger an enrichment run, optionally filtered by type (sentiment, depth, significance) or date range | Enrichment scheduler trigger |
| `psctl cost-report [--since DATE] [--by-task]` | Show AI API usage and estimated cost, broken down by task type and model | `reasoning.api_usage` query |
| `psctl ask "QUESTION"` | Submit a natural-language question to the agentic query interface and stream the answer to the terminal | `QueryService.AskQuestion` |

`psctl ask` streams tool-call progress and the final answer to stdout, making it useful for scripting and quick lookups without opening the UI. The `--json` flag outputs structured JSON (question, answer, supporting data references, token usage) for programmatic use.

`psctl cost-report` helps monitor AI spend from the command line — useful for checking daily budget consumption without navigating to the admin dashboard.

### Backup/Restore Extension

Phase 3 introduces the `reasoning` schema — a significant new body of data that the backup/restore bundle must cover. Extend `CreateBackup` and `RestoreBackup` to include:

- **`reasoning.enrichments`** — all AI-generated enrichment rows (depth scores, sentiment labels, categorisation). These are expensive to regenerate (LLM API calls + cost), so preserving them across environment resets saves both time and money.
- **`reasoning.embeddings`** — vector embeddings per contribution. Re-generating embeddings is cheaper (Google free tier) but slow for large datasets; include them in the bundle.
- **`reasoning.api_usage`** — cost tracking history. Useful for maintaining spend visibility across environment resets.
- **`reasoning.conversations`** — saved agentic query conversations and their reasoning traces.
- **`reasoning.insights`** — AI-generated insights with supporting data and evidence chains.
- **AI provider config** — `config.global_settings` entries under `ai.tasks.*` (provider routing, model selection, budget caps). Already covered by Phase 1's global settings backup, but verify the new keys are included.

The `PreviewBackup` RPC response should be updated to show reasoning-specific counts (e.g. "2,847 enrichments, 3,102 embeddings, 14 saved conversations, 8 insights").

**Note on pgvector:** The backup format (JSONL) should serialise embedding vectors as float arrays. On restore, vectors are inserted back into the `reasoning.embeddings` table and pgvector indexes are rebuilt automatically.

---

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| AI provider API instability or rate limits | Medium | Enrichment backlog builds up | Retry with backoff; multi-provider support means we can failover |
| Enrichment quality is inconsistent | Medium | Bad scores undermine trust | Few-shot prompts calibrated to real data; confidence scores let UI flag low-confidence results; manual override path |
| Embedding model changes require re-embedding everything | Low | Hours of API calls + migration | Track model name per embedding; re-embedding is a batch job, not blocking |
| Agent hallucination in answers | Medium | Users act on wrong information | Tool-use architecture constrains the agent to real data; reasoning trace makes hallucination detectable; all claims cite sources |
| Cost exceeds budget | Low | Spend $20+/day instead of $10 | Daily budget cap, selective enrichment, usage dashboard, model downgrade path |
| pgvector performance at scale | Low | Slow similarity queries | IVFFlat is fast for <1M vectors; if needed, tune `lists` parameter or move to HNSW |
| Users don't trust AI outputs | Medium | Feature goes unused | Traceability is the answer — every output is auditable. Build trust through transparency, not opacity |

---

## Enrichment & Similarity UI Integration Points

Phase 3 AI features need to integrate with existing Phase 2 pages. This section specifies exactly where enrichment badges, similarity panels, and AI-generated data appear in the UI.

### Contribution detail view (new page: `/contributions/[contributionId]`)

Phase 2 has no standalone contribution detail page — metrics and traceability links point to contributions but there's no dedicated view. Phase 3 requires one:

- **Route:** `/contributions/[contributionId]`
- **Content:** Full contribution metadata (title, state, timestamps, platform, author), plus:
  - **Enrichment badges:** All enrichments for this contribution (e.g. review depth 4/5, sentiment: constructive, significance: notable). Each badge is clickable → expands to show rationale, model name, input preview, confidence score, and timestamp.
  - **Related items panel:** "Similar contributions" list from the embeddings similarity API. Shows top 5 related items across all platforms with distance score. Clicking navigates to that contribution's detail view.
  - **Cross-platform links:** If embedding similarity finds a related Jira ticket for a PR (or vice versa), surface it prominently as "Likely related: PROJ-123" above the general similarity list.
- **Navigation:** Reachable from individual profile page (click a contribution row), team detail page (click a metric's source link), and agent answers (click a citation).

### Existing page enhancements

| Page | Addition | Details |
|------|----------|---------|
| **`/people/[personId]`** (individual profile) | Enrichment summary stats | Avg review depth, sentiment distribution, significance breakdown in the GitHub contribution section. Each stat links to the underlying enrichments. |
| **`/people/[personId]`** | Enrichment badges on contribution rows | Each contribution row in the breakdown sections shows inline enrichment badges (e.g. depth score pill, sentiment icon). |
| **`/teams/[teamId]`** (team detail) | Aggregated enrichment metrics | Team-level avg review depth, % constructive reviews, % significant PRs. These appear as additional columns/cards alongside flow metrics. |
| **`/teams`** (comparison) | Optional enrichment columns | Enrichment-derived columns (avg review depth, % constructive) available as toggleable columns. Not shown by default — keeps the page from becoming overwhelming. |
| **Main navigation** | "Ask" entry | Chat/query interface accessible from the top-level navigation bar. |
| **Ingestion Status page** | AI Pipeline Status section | Enrichment and embedding pipeline health (W1 deliverable 9, W2 deliverable 6). |
| **Admin panel** | AI Settings tab | Provider config, credentials, budget (W0 deliverable 8). |
| **Admin panel** | AI Cost Dashboard | Spend tracking and budget monitoring (W0 deliverable 9). |

---

## Testing Strategy

### Per-workstream automated tests

**W0 — Provider Foundation & Object Storage:**
- Unit tests for `ModelProvider` trait with mock provider returning canned responses
- Integration tests against live OpenRouter and Google Gemini APIs with minimal payloads (single-token completion, single-text embedding)
- `ArtifactStore` trait tests against `LocalFileSystem` backend: write, read, delete, pre-signed URL generation
- Cost tracking: verify `reasoning.api_usage` rows are created for each API call with correct token counts
- Rate limiting: verify throttling kicks in at configured limits
- Admin UI: component tests for AI Settings tab — provider credential input, task routing dropdowns, Test Provider button, budget cap input
- Config persistence: verify task-to-provider routing writes to/reads from `config.global_settings`

**W1 — Enrichment Pipeline:**
- Unit tests per enrichment type with known input → expected structured JSON output (mocked `ModelProvider`)
- Review depth analyser: test with 5 review samples spanning 1-5 depth, verify scores and rationale
- Sentiment analyser: test with constructive, neutral, critical, and hostile review samples
- Contribution categoriser: test with small/routine PR and large/significant PR
- Scheduler idempotency: run twice with same un-enriched contributions, verify no duplicates
- Budget cap enforcement: mock cumulative daily spend exceeding cap, verify scheduler pauses
- `input_hash` correctness: verify SHA-256 matches source content; verify re-enrichment skips unchanged inputs
- Traceability: verify every enrichment row has `model_name`, `input_hash`, `input_preview`, `confidence`, `value`
- Pipeline status API: verify pending count, last run time, error count are accurate
- Enrichment UI: component tests for enrichment badges, click-to-expand provenance display

**W2 — Embeddings & Similarity:**
- pgvector migration test: verify `vector` extension is enabled, `reasoning.embeddings` table accepts vectors
- Embedding pipeline: mock `ModelProvider::embed()`, verify vectors stored with correct `contribution_id` and `model_name`
- Similarity search: seed 20 embeddings, query `FindSimilar`, verify results sorted by distance
- Cross-platform linkage: seed a PR embedding and a Jira ticket embedding with similar content, verify `FindSimilar` with `platform_filter = 'jira'` returns the ticket
- `SearchByText`: verify on-the-fly embedding + search works end-to-end
- Pipeline status: verify pending/coverage/error counts match actual state
- Similarity UI: component tests for "related items" panel rendering and click navigation

**W3 — Agentic Query Interface:**
- Tool definition tests: each tool returns expected structure for known inputs (mock DB)
- Orchestration loop: mock `ModelProvider` returning tool calls → verify tool execution → verify final answer assembly
- Iteration limit: mock model that always returns tool calls, verify loop stops at 10
- Timeout: mock slow tool, verify 60s timeout fires
- Streaming: verify `AskQuestionEvent` stream produces correct event sequence (`ToolCallStarted` → `ToolCallCompleted` → `FinalAnswer`)
- Reasoning trace storage: verify `reasoning.conversations` row created with full trace
- Insight persistence: save an answer as insight, verify `reasoning.insights` row with `conversation_id` link
- Artifact generation: verify PDF/Markdown output stored via `ArtifactStore`, `artifact_key` set on insight
- Conversation UI: component tests for chat interface, tool-call progress display, citation links, "How was this generated?" expandable trace

### Per-workstream manual testing

**After W0 (Provider Foundation):**
1. Open admin UI → AI Settings tab
2. Enter OpenRouter API key → click "Test Provider" → verify success status
3. Enter Google API key → click "Test Provider" → verify success status
4. Verify task-to-provider routing table shows default assignments (enrichment → Haiku, insights → Sonnet, etc.)
5. Change embedding task to a different model → save → verify config persisted
6. Set daily budget cap to $5 → save → verify displayed
7. Check object storage health indicator shows green
8. Open AI Cost Dashboard → verify it renders correctly with $0.00 spend (no API calls yet)

**After W1 (Enrichment Pipeline):**
1. Open Ingestion Status page → verify AI Pipeline Status section appears
2. Check enrichment pending count — should show un-enriched contributions from Phase 1/2 data
3. Wait for enrichment scheduler to run (or trigger via `psctl enrich`)
4. Refresh AI Pipeline Status → verify pending count decreased, throughput rate populated
5. Navigate to `/people/[personId]` → find a code review in the GitHub section → verify enrichment badges (depth score, sentiment) appear
6. Click an enrichment badge → verify provenance popover shows: score, rationale, model name, input preview, timestamp
7. Navigate to `/contributions/[contributionId]` → verify all enrichments displayed with full traceability
8. Check AI Cost Dashboard → verify enrichment spend is logged with correct task type and model
9. Set daily budget cap to $0.01 → trigger enrichment → verify pipeline pauses with "budget exceeded" indicator

**After W2 (Embeddings & Similarity):**
1. Check AI Pipeline Status → verify embedding section shows pending count and coverage percentage
2. Wait for embedding pipeline to run (or check after next scheduled cycle)
3. Verify coverage percentage increases as embeddings are generated
4. Navigate to a PR's contribution detail view → verify "Related items" panel shows similar contributions
5. Check if a PR shows a "Likely related" Jira ticket (if similar content exists)
6. Click a related item → verify navigation to that contribution's detail view
7. Check AI Cost Dashboard → embeddings should show ~$0 (Google free tier)

**After W3 (Agentic Query Interface):**
1. Click "Ask" in main navigation → verify chat interface opens
2. Verify suggested questions appear based on team context
3. Type: "How has Team X's review quality changed this quarter?" → submit
4. Watch tool-call progress feed — verify tool names and arguments are visible
5. Read the final answer — verify it cites specific contributions and metrics
6. Click a citation link → verify it navigates to the source contribution or metric
7. Expand "How was this generated?" → verify full reasoning trace is displayed
8. Click "Save as Insight" → verify saved and retrievable
9. Try `psctl ask "What are the top 3 teams by throughput?"` → verify streaming output in terminal
10. Try `psctl cost-report` → verify daily spend breakdown matches the AI Cost Dashboard

### Cross-cutting

- **Traceability audit:** For every AI-generated element visible in the UI (enrichment badge, similarity link, agent answer), verify that clicking it reveals: the model that produced it, the input data, and links to source contributions. **No black boxes.**
- **Budget enforcement end-to-end:** Set a low daily cap, run enrichment + agentic queries, verify the system pauses enrichment when the cap is hit while still allowing agentic queries (which are user-initiated and should warn, not block).
- **Provider failover:** Disable one provider's API key, verify the system logs errors and fails gracefully without crashing the pipeline.
- **Navigation flow:** Verify the full path: `/teams` → `/teams/[teamId]` (with enrichment columns) → `/people/[personId]` (with enrichment badges) → `/contributions/[contributionId]` (with full provenance + similarity) → cited contribution → back via breadcrumbs.

---

## Exit Criteria Checklist

- [ ] `ModelProvider` trait implemented with OpenRouter and Google Gemini providers
- [ ] `ArtifactStore` trait implemented, S3-compatible object storage deployed (RustFS or Garage)
- [ ] Provider selection is config-driven, no code changes needed to swap models
- [ ] Enrichment pipeline runs on schedule, enriching code reviews (depth + sentiment) and PRs (significance)
- [ ] Every enrichment row has: `model_name`, `input_hash`, `input_preview`, structured `value` with rationale
- [ ] Embeddings generated for PRs, reviews, Discourse topics, Jira tickets
- [ ] Similarity search works across platforms (find Jira ticket related to a PR)
- [ ] Agentic query interface accepts natural-language questions and returns sourced answers
- [ ] Agent reasoning trace is stored and viewable in the UI
- [ ] UI shows provenance for every AI-generated element (enrichment badge, insight, agent answer)
- [ ] Daily API cost stays within configured budget cap
- [ ] Cost usage dashboard is visible to admins
- [ ] A non-technical user can click any AI-generated claim and see exactly what data produced it
