# Plan 65: Strip OpenRouter Support

## Context

Prism currently supports two AI providers: Google Gemini and OpenRouter. The dual-provider abstraction adds complexity across the stack — enum variants, separate API clients, catalogue fetching, cost tracking, image generation, and frontend UI — without delivering value. We're using Gemini exclusively and will continue to do so. This plan removes OpenRouter to simplify the codebase.

## Scope

Remove all OpenRouter-specific code paths while preserving the general provider abstraction only where it's zero-cost (e.g. the `provider` TEXT column in `config.ai_models` stays — it's harmless and avoids a migration). The goal is to delete code, not restructure.

## Changes by Area

### 1. Domain Enums & Types (`ps-core`)

**`crates/ps-core/src/models/enums.rs`**
- Remove `AiProvider::OpenRouter` variant and its `Display`/`FromStr` arms
- Remove `SecretKey::OpenRouterApiKey` variant and its string mapping
- If `AiProvider` becomes a single-variant enum, consider removing it entirely and hardcoding `"google"` where needed. However, keeping it as a single-variant enum is fine too — it's cheap and avoids a large blast radius.

### 2. Reasoning Crate (`ps-reasoning`)

**`crates/ps-reasoning/src/routing.rs`**
- Remove `openrouter_client: Option<openrouter::Client>` from `TaskRouter`
- Remove `set_openrouter()` method
- Remove OpenRouter entries from `provider_env_vars()` (drop `OPENROUTER_API_KEY`)
- Simplify `resolve_provider()` — no need to match on provider, just return Google client (or error if not configured)
- Remove `ResolvedProvider::OpenRouter` variant
- Remove `OPENROUTER_TEST_MODEL` constant
- Clean up `test_provider()` to only handle Google

**`crates/ps-reasoning/src/types.rs`**
- Default routing already points to Google — no change needed, but verify no OpenRouter references remain

**`crates/ps-reasoning/src/catalogue.rs`**
- Delete `fetch_openrouter_models()` and its response structs (`OpenRouterModel`, `OpenRouterModelsResponse`, `OpenRouterPricing`, `OpenRouterArchitecture`)
- Remove OpenRouter arm from `fetch_models()` dispatcher
- Delete OpenRouter capability-detection tests

**`crates/ps-reasoning/src/cost.rs`**
- Delete `openrouter_pricing()` function
- Remove OpenRouter arm from `model_pricing()` dispatcher

**`crates/ps-reasoning/src/features/enrichment/extract.rs`**
- Remove OpenRouter arm from `extract_enrichment()` — call Google client directly instead of dispatching

### 3. Image Generation (`ps-mcp`)

**`crates/ps-mcp/src/tools/generate_image.rs`**
- Remove `ImageProvider::OpenRouter` variant
- Delete `generate_openrouter()` and `extract_image_from_openrouter()` functions
- Simplify `resolve_model_and_provider()` — always use Google, remove "provider/" prefix parsing for OpenRouter
- Remove `OPENROUTER_API_KEY` env var read

### 4. Server (`ps-server`)

**`crates/ps-server/src/features/reasoning/ai_settings.rs`**
- Remove `"openrouter"` / `"openrouter_api_key"` entries from `build_ai_settings()` provider_secret_status map
- Remove OpenRouter arm from `provider_secret_key()`
- Remove `router.set_openrouter()` call from `load_providers_from_db_impl()`
- Simplify `set_provider_secret()` — only Google key path

**`crates/ps-server/src/common/conversions.rs`**
- Remove `AiProvider::OpenRouter` arms from `ai_provider_to_proto()` and `proto_to_ai_provider()`

### 5. Workers (`ps-workers`)

**`crates/ps-workers/src/features/reasoning/model_catalogue.rs`**
- Remove OpenRouter iteration from the provider loop in `refresh_catalogue()`
- Only fetch Google models

**`crates/ps-workers/src/main.rs`**
- Remove OpenRouter client setup from `setup_ai_router()`

### 6. Agent Pod Spec (`ps-agent`)

**`crates/ps-agent/src/pod_spec.rs`**
- No structural change needed — `provider_env_vars()` will simply return fewer entries after the routing.rs cleanup

### 7. Proto Definitions

**`proto/canonical/prism/v1/common.proto`**
- Remove `OPENROUTER = 2` from `AiProvider` enum
- Reserve field number 2: `reserved 2;`

**`proto/canonical/prism/v1/reasoning.proto`**
- No structural changes — messages are provider-agnostic. The `provider_secret_status` map will simply never contain an `"openrouter"` key.

After proto changes: `buf lint && buf generate`, rebuild both backend and frontend.

### 8. Frontend

**`frontend/views/admin/components/ai-settings-tab.tsx`**
- Remove OpenRouter entry from `PROVIDERS` array
- Remove OpenRouter key row from `ProviderCredentialsSection`
- Simplify task routing dropdowns — if only one provider, the provider select becomes unnecessary. Replace with a static "Google Gemini" label or remove the provider column entirely.

**`frontend/views/ask/components/model-selector.tsx`**
- Remove `OpenRouterIcon` component and its SVG
- Remove OpenRouter arm from `ProviderIcon` dispatcher
- Simplify model grouping if all models are now Google

**`frontend/lib/proto-display.ts`**
- Remove `AiProvider.OPENROUTER` display mapping

**Generated code** (`frontend/lib/api/gen/`) — regenerated automatically by `buf generate`.

### 9. Database

**No migration needed.** The `config.ai_models` table uses TEXT for `provider` — existing OpenRouter rows (if any) become inert. The model catalogue refresh will stop inserting new ones, and existing rows will age out on next refresh (the handler does `replace_ai_models` which deletes before inserting).

The `config.secrets` table may contain an encrypted `openrouter_api_key` row. It's harmless — no code will read it. Optionally add a migration to `DELETE FROM config.secrets WHERE secret_key = 'openrouter_api_key'` for cleanliness, but it's not required.

The `reasoning.api_usage` table may contain historical rows with `provider = 'openrouter'`. Keep these for audit history.

### 10. Integration Tests

**`tests/integration/src/api/reasoning.rs`**
- Remove assertions about `provider_secret_status["openrouter"]`
- Remove any test cases that configure OpenRouter

### 11. Plans & Documentation

- Update plan 40 (Rig framework) to reflect single-provider reality
- Update plan 41 (model catalogue) to remove OpenRouter fetch docs
- Update plan 60 (image generation) to remove OpenRouter provider path

### 12. Memory

- Update `feedback_ai_providers.md` memory file — the "must support OpenRouter + Gemini" constraint is superseded by this work.

## Implementation Order

1. **Proto** — remove enum variant, regenerate
2. **ps-core** — remove enum variants
3. **ps-reasoning** — strip OpenRouter from routing, catalogue, cost, enrichment
4. **ps-mcp** — strip OpenRouter from image generation
5. **ps-server** — strip from AI settings service and conversions
6. **ps-workers** — strip from catalogue handler and main setup
7. **Frontend** — strip from admin settings, model selector, display utils
8. **Tests** — update integration tests
9. **Docs/memory** — update plans and memory files
10. `prek run -av` — verify clean build, lint, and tests

## Risk

Low. OpenRouter is a leaf dependency — no other features depend on it. The main risk is missing a reference that causes a compile error, which is caught immediately. No data migration required.
