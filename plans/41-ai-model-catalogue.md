# Plan 41 — AI Model Catalogue

## Problem

Model selection in the admin AI settings page is a free-text input. Admins must know valid model IDs by heart. Typos silently break enrichment. There's no way to know which models a provider actually offers.

## Goal

1. Fetch the list of available models from each configured AI provider
2. Store a cached catalogue in the database
3. Replace the free-text model input with a searchable dropdown populated from real provider data
4. Keep the catalogue fresh via a Restate handler (consistent with existing orchestration patterns)

## Design

### Provider Model List APIs

**Google Gemini:** `GET https://generativelanguage.googleapis.com/v1beta/models?key={API_KEY}`
Returns a list of `Model` objects with `name`, `displayName`, `description`, `supportedGenerationMethods`, `inputTokenLimit`, `outputTokenLimit`.

**OpenRouter:** `GET https://openrouter.ai/api/v1/models` (Authorization: Bearer {API_KEY})
Returns `data[]` with `id`, `name`, `description`, `context_length`, `pricing.prompt`, `pricing.completion`, `top_provider`, `architecture`.

Both are simple REST GETs — no pagination needed (full list returned).

### Database Schema

New table in the `config` schema (not `reasoning` — this is provider configuration metadata, not AI-generated output):

```sql
-- migrations/NNNN_create_model_catalogue.sql
CREATE TABLE config.ai_models (
    id              TEXT        NOT NULL,  -- provider-native model ID (e.g. "gemini-2.5-flash", "anthropic/claude-sonnet-4")
    provider        TEXT        NOT NULL,  -- "google" or "openrouter"
    display_name    TEXT        NOT NULL,  -- human-friendly name
    description     TEXT,                  -- optional model description
    context_length  INTEGER,              -- max context window tokens
    input_price     DOUBLE PRECISION,     -- USD per 1M input tokens (NULL if unknown)
    output_price    DOUBLE PRECISION,     -- USD per 1M output tokens (NULL if unknown)
    capabilities    TEXT[]      NOT NULL DEFAULT '{}', -- e.g. {"completion", "embeddings", "tool_use"}
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (provider, id)
);

-- For filtering by provider in the UI
CREATE INDEX idx_ai_models_provider ON config.ai_models (provider);

-- Track when each provider's catalogue was last refreshed
-- Stored in config.global_settings as "ai.models_refreshed.<provider>" → ISO timestamp
```

No separate "last refreshed" table — reuse `config.global_settings` with keys like `ai.models_refreshed.google` and `ai.models_refreshed.openrouter`.

### Capabilities Mapping

Models should be tagged with capabilities so the UI can filter appropriately per task type:

| Task Type   | Required Capability |
|-------------|-------------------|
| Enrichment  | `completion`      |
| Insights    | `completion`      |
| Agentic     | `tool_use`        |
| Embeddings  | `embeddings`      |

**Google:** derive from `supportedGenerationMethods` — `generateContent` → `completion`, `embedContent` → `embeddings`. Tool use inferred from model family (flash/pro support it).

**OpenRouter:** derive from model metadata — most chat models support `completion` + `tool_use`. The `architecture.modality` field hints at capabilities.

### Restate Handler: `ModelCatalogueHandler`

New handler in `ps-workers` following the existing pattern:

```rust
// crates/ps-workers/src/handlers/model_catalogue.rs

pub struct ModelCatalogueHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::service]
pub trait ModelCatalogueHandler {
    /// Refresh the model catalogue for all configured providers.
    async fn refresh_catalogue() -> Result<(), TerminalError>;
}
```

**Why a Restate handler (not a server-side RPC)?**
- Consistent with how all external API work is done in Prism (Restate handlers, not synchronous RPCs)
- Durable execution — if a provider API is temporarily down, Restate retries automatically
- Can be triggered on-demand (admin clicks "Refresh") OR on a schedule
- No long-running HTTP request tying up a gRPC thread

**Handler flow:**
1. For each provider that has a configured API key:
   a. `ctx.run("fetch_google_models")` — call the provider's model list API via `reqwest`
   b. Parse response into `Vec<AiModel>` domain type
   c. `ctx.run("store_google_models")` — bulk upsert into `config.ai_models` via `UNNEST`
   d. `ctx.run("update_google_timestamp")` — set `ai.models_refreshed.google` in global_settings
2. Log results with tracing

**API key access:** The handler has `SharedState` which includes `repos` and `secret_key`. It decrypts provider API keys the same way the enrichment handler's startup code does. Decryption happens OUTSIDE `ctx.run()` (per the security convention — no secrets in the Restate journal).

**Triggering:**
- **On demand:** Admin clicks "Refresh models" in the UI → gRPC RPC → server sends Restate invocation
- **On key save:** When `SetProviderSecret` succeeds, the server sends a Restate invocation to refresh that provider's catalogue
- **Periodic:** Optional — could use Restate delayed self-invocation (like scheduled ingestion) to refresh daily. Low priority since model lists change infrequently.

### Repository Layer

New methods in `ConfigRepo` (this is provider configuration, not reasoning output):

```rust
// ps-core/src/repo/config.rs

impl ConfigRepo {
    /// Bulk upsert models for a provider (replaces stale entries).
    pub async fn upsert_ai_models(&self, models: &[AiModel]) -> Result<(), sqlx::Error>;

    /// List models for a provider, optionally filtered by capability.
    pub async fn list_ai_models(
        &self,
        provider: &str,
        capability: Option<&str>,
    ) -> Result<Vec<AiModel>, sqlx::Error>;

    /// Delete all models for a provider (used before re-import if doing full replace).
    pub async fn delete_ai_models_for_provider(&self, provider: &str) -> Result<(), sqlx::Error>;
}
```

The `upsert_ai_models` method uses the `UNNEST` batch pattern (per performance conventions). Full-replace strategy: delete existing models for the provider, then insert the fresh list — wrapped in a transaction. This handles model deprecation/removal cleanly.

### Domain Type

```rust
// ps-core/src/models/ai_model.rs (or within config.rs model file)

pub struct AiModel {
    pub id: String,
    pub provider: AiProvider,
    pub display_name: String,
    pub description: Option<String>,
    pub context_length: Option<i32>,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    pub capabilities: Vec<String>,
}
```

### Proto Changes

Add to `reasoning.proto` (or a new section in it, since this is accessed via the ReasoningService):

```protobuf
message AiModel {
  string id = 1;
  string provider = 2;
  string display_name = 3;
  string description = 4;
  int32 context_length = 5;
  optional double input_price_per_million = 6;
  optional double output_price_per_million = 7;
  repeated string capabilities = 8;
}

message ListAiModelsRequest {
  string provider = 1;           // filter by provider (empty = all)
  string capability = 2;         // filter by capability (empty = all)
}

message ListAiModelsResponse {
  repeated AiModel models = 1;
  map<string, string> last_refreshed = 2; // provider → ISO timestamp
}

message RefreshModelCatalogueRequest {}
message RefreshModelCatalogueResponse {
  int32 google_count = 1;
  int32 openrouter_count = 2;
}
```

New RPCs on `ReasoningService`:

```protobuf
rpc ListAiModels(ListAiModelsRequest) returns (ListAiModelsResponse);
rpc RefreshModelCatalogue(RefreshModelCatalogueRequest) returns (RefreshModelCatalogueResponse);
```

`ListAiModels` is a direct DB read (fast, synchronous).
`RefreshModelCatalogue` dispatches to the Restate handler and returns the counts when complete (or returns immediately with a "refresh started" ack — TBD based on UX preference).

### Server Implementation

`ReasoningServiceImpl` gets two new RPC methods:

- **`list_ai_models`**: Reads from `ConfigRepo::list_ai_models()`, maps to proto, returns.
- **`refresh_model_catalogue`**: Sends a Restate invocation to `ModelCatalogueHandler/refresh_catalogue` via the Restate ingress HTTP API (same pattern as `TriggerHandler`). Returns immediately with an ack. The frontend polls or refetches after a short delay.

### Frontend Changes

**New hook:** `useAiModels(provider, capability)` — calls `ListAiModels` RPC, returns typed model list. Cached by React Query with a reasonable stale time (5 minutes).

**New hook:** `useRefreshModelCatalogue()` — mutation that calls `RefreshModelCatalogue`, invalidates the `aiModels` query key on success.

**Task Routing UI update** (in `ai-settings-tab.tsx`):

Replace the free-text `<Input>` for model selection with a **Combobox** (searchable select):
- Uses shadcn `<Popover>` + `<Command>` (cmdk) for searchable dropdown
- Filters models by the currently selected provider AND the task's required capability
- Shows model display name + context length + pricing as secondary info
- Falls back to allowing free-text entry if the catalogue is empty (provider key not set yet, or first use before refresh)
- "Refresh models" button next to the provider credentials section (with loading state)

**Model list display in dropdown:**
```
gemini-2.5-flash
  1M context · $0.15/M in · $0.60/M out

gemini-2.5-pro
  1M context · $1.25/M in · $10.00/M out
```

**Auto-refresh trigger:** When a provider secret is saved successfully (`useSetProviderSecret` `onSuccess`), automatically trigger a catalogue refresh for that provider.

### Cost Tracking Integration (Bonus)

The `model_pricing()` function in `ps-reasoning/src/cost.rs` currently uses hardcoded pricing. With the catalogue storing `input_price` and `output_price` from provider APIs, a future enhancement could read pricing from the catalogue instead. Out of scope for this plan but worth noting as a natural follow-on.

## Implementation Steps

### Phase 1: Backend (database + handler + API)
1. **Migration**: Create `config.ai_models` table
2. **Domain type**: Add `AiModel` struct to `ps-core/src/models/`
3. **Repository**: Add `upsert_ai_models`, `list_ai_models`, `delete_ai_models_for_provider` to `ConfigRepo`
4. **Provider fetchers**: Module in `ps-reasoning` with functions to call Google/OpenRouter model list APIs and parse responses into `Vec<AiModel>`
5. **Restate handler**: `ModelCatalogueHandler` in `ps-workers` — wires up fetch + store
6. **Proto**: Add `AiModel`, `ListAiModels`, `RefreshModelCatalogue` messages and RPCs
7. **Server RPCs**: Implement `list_ai_models` and `refresh_model_catalogue` in `ReasoningServiceImpl`
8. **Auto-trigger**: Fire catalogue refresh when `SetProviderSecret` succeeds
9. **sqlx prepare** + test

### Phase 2: Frontend
10. **Hooks**: `useAiModels`, `useRefreshModelCatalogue`
11. **Combobox component**: Searchable model selector (shadcn Command + Popover)
12. **Wire into Task Routing**: Replace `<Input>` with model combobox, filtered by provider + capability
13. **Refresh button**: Add to Provider Credentials section
14. **Auto-refresh on key save**: Trigger refresh in `onSuccess` of `useSetProviderSecret`

## Decisions (Resolved)

1. **Full replace** per provider (delete + insert in txn) — handles deprecated/removed models cleanly.
2. **Async refresh** — fire-and-forget to Restate, frontend refetches after. Consistent with other handlers.
3. **Provider fetchers live in `ps-reasoning`** — alongside routing, since it already owns provider knowledge.
4. **No periodic refresh** — auto-refresh on key save is sufficient. Model lists rarely change.
