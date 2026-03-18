# Plan 40: Adopt Rig Framework for AI/Reasoning

## Context

W0 of [Phase 3](./14-phase3-intelligence.md) is complete. We have a working `ModelProvider` trait, Google Gemini and OpenRouter implementations, task routing, cost tracking, and the `ArtifactStore`/S3 infrastructure. Before starting W1-W3, we evaluated the [Rig](https://github.com/0xPlaygrounds/rig) Rust LLM framework and concluded it should replace our hand-rolled provider layer.

**Decision: adopt `rig-core` as the LLM framework for Prism.**

## Why Rig

Rig (`rig-core`, v0.31+, 6.6k stars, 533 releases, latest March 2026) is a mature Rust framework for building LLM applications. It provides:

| Capability | Our hand-rolled code | Rig equivalent |
|---|---|---|
| Provider abstraction | `ModelProvider` trait (42 lines) | `CompletionModel` + `EmbeddingModel` traits, battle-tested |
| Google Gemini client | `providers/google.rs` (~420 lines) | `rig::providers::gemini::Client` — built-in, maintained |
| OpenRouter client | `providers/openrouter.rs` (~360 lines) | `rig::providers::openrouter::Client` — built-in, maintained |
| Request/response types | `types.rs` (~147 lines) | Built-in completion types |
| Tool definitions | `ToolDefinition`, `ToolCall` structs | `#[tool_macro]` proc macro + `Tool` trait |
| Agent orchestration loop | Not yet built (planned ~500-800 lines for W3) | `Agent` struct with built-in tool-call loop |
| Structured extraction | Not yet built (planned for W1 enrichment) | `extractor` module — derive `JsonSchema`, get typed output |
| Streaming | Not yet built (planned for W3) | Built-in streaming completion support |
| Additional providers | Only Google + OpenRouter | 20+ providers (Anthropic, OpenAI, Groq, Ollama, Mistral, etc.) |

### What Rig gives us for free in W1-W3

**W1 (Enrichment Pipeline):** Rig's `extractor` module handles structured data extraction natively. Define Rust structs like `ReviewDepthScore` with `#[derive(Deserialize, Serialize, JsonSchema)]`, build an extractor, and Rig generates the JSON schema and parses responses into typed structs. No manual prompt engineering for output format.

**W2 (Embeddings):** Rig's `EmbeddingModel` trait replaces our `ModelProvider::embed()`. Provider clients expose `.embedding_model("gemini-embedding-2")` directly.

**W3 (Agentic — biggest win):** The plan calls for a manual orchestration loop (call model → check for tool calls → execute tools → repeat, max 10 iterations). This is exactly what Rig's `Agent` does out of the box. The `#[tool_macro]` proc macro turns our 9 planned tools into agent-compatible tools with zero boilerplate. Streaming is built in.

### What Rig does NOT replace (we keep)

| Component | Why we keep it |
|---|---|
| `CostTracker` + `estimate_cost()` | Rig returns token usage but doesn't persist costs to a database. Prism-specific. |
| `TaskRouter` (simplified) | Config-driven routing of task types to providers/models is Prism-specific. Becomes a thin wrapper picking Rig clients. |
| `ReasoningRepo` | All DB access stays in ps-core per our layering rules. |
| `ReasoningService` gRPC layer | Stays as-is, calls Rig instead of our providers. |
| `ArtifactStore` | Object storage has nothing to do with LLMs. |
| `ProviderError` | We keep our error type but implement `From` conversions from Rig errors. |

### Risks and mitigations

| Risk | Mitigation |
|---|---|
| Rig is pre-1.0, warns of breaking changes | Pin to a specific version. Wrap Rig behind a thin internal adapter so API changes are absorbed in one place. |
| No pgvector integration | Implement Rig's `VectorStoreIndex` trait wrapping our existing sqlx queries (~50 lines). |
| Rig's cost tracking is absent | We keep our `CostTracker` — extract token usage from Rig's responses and log to `reasoning.api_usage`. |
| Rig's provider may diverge from upstream API | Rig has 20+ active provider maintainers and 533 releases. If a provider breaks, we can fall back to our own `ModelProvider` impl temporarily. |

---

## Changes to Existing Code (W0 Retrofit)

### Delete

| File | Reason |
|---|---|
| `crates/ps-reasoning/src/provider.rs` | Replaced by Rig's `CompletionModel` + `EmbeddingModel` traits |
| `crates/ps-reasoning/src/providers/google.rs` | Replaced by `rig::providers::gemini` |
| `crates/ps-reasoning/src/providers/openrouter.rs` | Replaced by `rig::providers::openrouter` |
| `crates/ps-reasoning/src/types.rs` (partially) | Remove `CompletionMessage`, `CompletionRequest`, `CompletionResponse`, `Role`, `FinishReason`, `ToolDefinition`, `ToolCall`, `TokenUsage`. Keep `AiConfig`, `AiTaskConfig`, `AiTaskRouting`. |

### Modify

**`crates/ps-reasoning/src/routing.rs`** — `TaskRouter` becomes a thin wrapper:

```rust
use rig::providers::{gemini, openrouter};

pub struct TaskRouter {
    google: Option<gemini::Client>,
    openrouter: Option<openrouter::Client>,
    config: AiConfig,
}

impl TaskRouter {
    /// Get a completion model for a task type.
    pub fn completion_model(&self, task: TaskType) -> Result<impl CompletionModel, ProviderError> {
        let task_config = self.config.tasks.get(task);
        match task_config.provider {
            AiProvider::Google => {
                let client = self.google.as_ref().ok_or(/* ... */)?;
                Ok(client.completion_model(&task_config.model))
            }
            AiProvider::OpenRouter => {
                let client = self.openrouter.as_ref().ok_or(/* ... */)?;
                Ok(client.completion_model(&task_config.model))
            }
        }
    }

    /// Get an embedding model for the configured embeddings task.
    pub fn embedding_model(&self) -> Result<impl EmbeddingModel, ProviderError> {
        let task_config = self.config.tasks.get(TaskType::Embeddings);
        // similar dispatch
    }
}
```

**`crates/ps-reasoning/src/cost.rs`** — `CostTracker::log_usage()` signature unchanged. Callers extract token usage from Rig's `CompletionResponse` (which includes `usage` field) and pass it in. The `TokenUsage` struct moves from our types to a thin wrapper or re-export.

**`crates/ps-reasoning/src/lib.rs`** — Update exports. Remove `ModelProvider` re-export, add Rig re-exports as needed.

**`crates/ps-reasoning/Cargo.toml`** — Add `rig-core` dependency, remove `reqwest` (Rig handles HTTP internally). Keep `async-trait` only if still needed elsewhere.

```toml
[dependencies]
rig-core = { version = "0.31", features = ["gemini", "openrouter"] }
# reqwest removed — Rig handles HTTP
# async-trait removed if not needed
```

**`crates/ps-server/src/services/reasoning.rs`** — `ReasoningServiceImpl` methods that construct providers (`set_google()`, `set_openrouter()`) now construct Rig clients instead:

```rust
// Before
router.set_google(GoogleProvider::new(api_key));

// After
router.set_google(gemini::Client::new(&api_key));
```

The `test_provider()` method uses Rig's client to make a minimal completion call (or list models endpoint) instead of our hand-rolled `test_connection()`.

---

## Changes to Phase 3 Plan (W1-W3)

### W1: Enrichment Pipeline — use Rig extractors

The enrichment pipeline uses Rig's `extractor` module instead of manually constructing JSON-schema prompts:

```rust
use rig::extractor::Extractor;
use schemars::JsonSchema;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct ReviewDepthScore {
    /// Score from 1 (trivial/rubber-stamp) to 5 (thorough architectural review)
    score: u8,
    /// Brief rationale for the score
    rationale: String,
    /// Confidence in the assessment (0.0 to 1.0)
    confidence: f32,
}

// Build extractor from the configured model
let model = router.completion_model(TaskType::Enrichment)?;
let extractor = model
    .extractor::<ReviewDepthScore>()
    .preamble("You assess code review depth for an engineering insights platform. ...")
    .build();

let result: ReviewDepthScore = extractor.extract(&review_text).await?;
```

This replaces the planned manual approach of crafting JSON schema in prompts and parsing responses. Same pattern applies to `SentimentLabel`, `ContributionSignificance`, and `TopicClassification`.

Few-shot examples are provided via the preamble, matching the existing plan's approach of calibrated examples.

### W2: Embeddings — implement VectorStoreIndex for pgvector

Rig has no built-in pgvector integration, but provides a `VectorStoreIndex` trait. We implement it wrapping our existing `ReasoningRepo` queries:

```rust
use rig::vector_store::VectorStoreIndex;

pub struct PgVectorIndex {
    repo: ReasoningRepo,
    model: Box<dyn EmbeddingModel>,
}

#[async_trait]
impl VectorStoreIndex for PgVectorIndex {
    async fn top_n(&self, query: &str, n: usize) -> Result<Vec<(f64, Document)>> {
        let embedding = self.model.embed_text(query).await?;
        let results = self.repo.find_similar(&embedding, n).await?;
        // map to Rig's Document type
    }
}
```

This enables RAG-enabled agents in W3 — the pgvector index plugs directly into Rig's `Agent` builder as a dynamic context source.

Embedding generation itself uses `model.embed_text()` / `model.embed_texts()` instead of our `ModelProvider::embed()`.

### W3: Agentic Query Interface — use Rig agents

This is the biggest simplification. The planned manual orchestration loop (10 sections of the plan) becomes:

```rust
use rig::agent::Agent;
use rig::tool::tool_macro;

// Define tools using Rig's proc macro
#[tool_macro]
async fn query_team_metrics(
    /// The team ID to query
    team_id: String,
    /// Start of the period (ISO 8601)
    period_start: String,
    /// End of the period (ISO 8601)
    period_end: String,
) -> Result<String, ToolError> {
    // Call MetricsRepo, format results as JSON string
}

// Build the agent
let model = router.completion_model(TaskType::Agentic)?;
let agent = model
    .agent("You are Prism, an engineering insights assistant. ...")
    .tool(QueryTeamMetrics::new(repos.clone()))
    .tool(QueryContributions::new(repos.clone()))
    .tool(CompareTeams::new(repos.clone()))
    .tool(GetPersonProfile::new(repos.clone()))
    .tool(SearchSimilar::new(pgvector_index.clone()))
    .tool(SearchByText::new(pgvector_index.clone()))
    .tool(QueryEnrichments::new(repos.clone()))
    .tool(ListTeams::new(repos.clone()))
    .tool(ListPeople::new(repos.clone()))
    .dynamic_context(2, pgvector_index)  // RAG: pull relevant context
    .max_tokens(4096)
    .temperature(0.3)
    .build();

// Single call replaces the manual orchestration loop
let response = agent.chat(&question, chat_history).await?;
```

**What we no longer need to build:**
- Manual tool-call detection and dispatch loop
- Iteration counting (Rig handles max iterations)
- Tool result formatting and re-injection into conversation
- Streaming orchestration (Rig's `Agent` supports `.stream_chat()`)

**What we still build:**
- The tool implementations themselves (these call our repos — Prism-specific)
- Reasoning trace capture — wrap Rig's agent calls to log each tool invocation to `reasoning.conversations`
- The gRPC `QueryService` and streaming event mapping
- The conversation UI
- Iteration limit (configure on Rig's agent or wrap with a timeout)

#### Streaming events

Rig supports streaming via `.stream_chat()`. We map Rig's stream events to our `AskQuestionEvent` proto:

| Rig stream event | Proto event |
|---|---|
| Tool call initiated | `ToolCallStarted` |
| Tool call completed | `ToolCallCompleted` |
| Partial text chunk | `PartialAnswer` |
| Stream complete | `FinalAnswer` |
| Error | `Error` |

#### Reasoning trace capture

We wrap the Rig agent's tool execution with a tracing layer that captures each tool call (name, arguments, result summary, timestamp) into a `Vec<TraceStep>`. This is stored in `reasoning.conversations.reasoning_trace` as JSONB, matching the existing plan exactly.

---

## Updated Dependency Graph

```
rig-core (external)
  ├── providers::gemini    ← replaces our GoogleProvider
  ├── providers::openrouter ← replaces our OpenRouterProvider
  ├── agent::Agent         ← replaces manual orchestration (W3)
  ├── extractor            ← structured output for enrichment (W1)
  ├── tool::Tool           ← tool definitions for agent (W3)
  ├── embeddings           ← embedding generation (W2)
  └── vector_store         ← VectorStoreIndex trait (we impl for pgvector)

ps-reasoning (our crate)
  ├── routing.rs           ← TaskRouter (thin, picks Rig clients by config)
  ├── cost.rs              ← CostTracker (unchanged — logs to reasoning.api_usage)
  ├── types.rs             ← AiConfig, AiTaskConfig, AiTaskRouting (kept)
  ├── pgvector.rs          ← NEW: VectorStoreIndex impl for pgvector (W2)
  ├── features/
  │   ├── enrichment/      ← NEW: Rig extractors for review depth, sentiment, etc. (W1)
  │   ├── embeddings/      ← NEW: embedding pipeline using Rig's EmbeddingModel (W2)
  │   └── agentic/         ← NEW: Rig agent + tool definitions (W3)
  └── lib.rs
```

## Updated Crate Dependencies

```toml
# crates/ps-reasoning/Cargo.toml
[dependencies]
ps-core = { path = "../ps-core" }
rig-core = { version = "0.31", features = ["gemini", "openrouter"] }
schemars = "0.8"           # For JsonSchema derive on extractor structs
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
thiserror.workspace = true
time = { workspace = true, features = ["macros"] }
tokio.workspace = true
tracing.workspace = true
uuid.workspace = true
# reqwest REMOVED — Rig handles HTTP internally
# async-trait REMOVED if not needed
```

New dependency: `schemars` for `#[derive(JsonSchema)]` on enrichment output structs (used by Rig's extractor module).

---

## Implementation Order

### Step 1: W0 retrofit (prerequisite for W1-W3)

1. Add `rig-core` to `ps-reasoning/Cargo.toml` with `gemini` + `openrouter` features
2. Add `schemars` dependency
3. Rewrite `TaskRouter` to hold Rig clients instead of our provider structs
4. Delete `providers/google.rs`, `providers/openrouter.rs`, `provider.rs`
5. Trim `types.rs` — remove types now provided by Rig, keep `AiConfig`/`AiTaskConfig`/`AiTaskRouting`
6. Update `ReasoningServiceImpl` to construct Rig clients
7. Update `test_provider()` to use Rig client APIs
8. Keep `CostTracker` — adapt to extract token usage from Rig responses
9. Verify: `prek run -av` passes, AI Settings tab still works end-to-end

### Step 2: W1 with Rig extractors

Proceed per [14-phase3-intelligence.md W1](./14-phase3-intelligence.md) but use Rig extractors for structured output instead of manual JSON schema prompts.

### Step 3: W2 with Rig embeddings + pgvector VectorStoreIndex

Proceed per plan, using Rig's `EmbeddingModel` and implementing `VectorStoreIndex` for pgvector.

### Step 4: W3 with Rig agents

Proceed per plan, using Rig's `Agent` builder with `#[tool_macro]` tools. Focus effort on tool implementations, reasoning trace capture, and the conversation UI — not on orchestration plumbing.

---

## Lines of Code Impact

| Category | Before (hand-rolled) | After (Rig) | Delta |
|---|---|---|---|
| Provider trait + error types | ~42 | ~20 (thin adapter) | -22 |
| Google provider | ~420 | 0 (deleted) | -420 |
| OpenRouter provider | ~360 | 0 (deleted) | -360 |
| Completion types | ~147 | ~50 (keep AiConfig only) | -97 |
| Task router | ~132 | ~60 (thin Rig client wrapper) | -72 |
| Agent orchestration (W3) | ~500-800 (planned) | ~100 (Rig agent setup) | -400 to -700 avoided |
| Structured extraction (W1) | ~200-300 (planned) | ~50 (derive macros) | -150 to -250 avoided |
| pgvector VectorStoreIndex | 0 | +50 (new) | +50 |
| **Net** | | | **~1,400-1,900 lines saved** |

---

## Changes to Architecture Docs

### 06-ai-reasoning.md

The "Provider Abstraction" section should note that Rig provides the provider layer:

> The `ModelProvider` trait concept is fulfilled by Rig's `CompletionModel` and `EmbeddingModel` traits. `ps-reasoning` holds a `TaskRouter` that selects Rig provider clients based on config-driven task routing. Individual provider HTTP implementations (Google Gemini REST API, OpenRouter OpenAI-compatible API) are handled by Rig — we do not maintain these ourselves.

The "Agentic Query Architecture" section should note that the orchestration loop is provided by Rig's `Agent`:

> The agent orchestration loop (tool-call → execute → re-prompt) is handled by Rig's `Agent` abstraction. Prism defines tools via Rig's `#[tool_macro]`, provides them to the agent builder, and Rig manages the conversation loop. We capture reasoning traces by wrapping tool execution.

### 01-architecture-overview.md

In the "Key Technology Choices" table, add:

| Concern | Choice | Rationale |
|---|---|---|
| LLM framework | Rig (`rig-core`) | Provider abstraction, agent orchestration, structured extraction, tool framework. 20+ providers, actively maintained. Avoids hand-rolling HTTP clients for each LLM API. |

### 14-phase3-intelligence.md

The W0 section's "ModelProvider Trait" subsection should be updated to note the Rig adoption. W1/W2/W3 sections should reference Rig extractors, embeddings, and agents respectively. The orchestration loop description in W3 should note it's handled by Rig rather than built from scratch.

---

## Decision Record

| Attribute | Value |
|---|---|
| Decision | Adopt `rig-core` as the LLM framework for Prism's AI/reasoning layer |
| Date | 2026-03-18 |
| Status | Accepted |
| Drivers | Avoid maintaining ~1,100 lines of provider HTTP code; gain agent orchestration, structured extraction, and streaming for free; 20+ providers instead of 2 |
| Alternatives considered | Continue hand-rolling (more control but high maintenance), `llm-chain` (less mature), `langchain-rust` (too opinionated, Python port) |
| Risks | Pre-1.0 API instability (mitigated by version pinning + thin adapter layer) |
