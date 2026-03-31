# Plan 60 — Image Generation Model Support

## Problem

When a user selects an image generation model (e.g. "Nano Banana 2" via OpenRouter) in the Ask interface, the agentic query fails. The current architecture assumes all models are text completion models — it passes the model through to OpenCode which attempts to use it as a chat model, then the agent tries workarounds like downloading images with curl, which also fail.

Image generation models have fundamentally different APIs: they accept a text prompt and return binary image data, not a text completion stream. The OpenCode agent runtime has no concept of this — it expects a conversational model that can reason and call tools.

### Root Cause

1. **No modality awareness in the model catalogue** — `AiModel.capabilities` tracks `["completion", "tool_use", "embeddings"]` but never `"image_generation"`. The catalogue fetcher doesn't detect image generation models from OpenRouter's `architecture.modality` field or Google's `supportedGenerationMethods`.
2. **Model selector shows image models alongside chat models** — filtered only by `tool_use` capability, but since OpenRouter models all get `["completion", "tool_use"]` unconditionally, image-only models slip through.
3. **No image generation pathway** — the only way to produce images today is via Python scripts (matplotlib/plotly) inside the agent container. There's no tool that calls image generation APIs.
4. **No inline image rendering in chat** — `AnswerContent` renders Markdown only. Artifacts show as a download/preview list, not inline in the conversation flow.

## Desired State

1. The model selector shows both chat models and image generation models in a single grouped dropdown. Selecting an image model signals the intent to generate an image.
2. When an image model is selected, the query still flows through the agent (using the default chat model for reasoning), but the agent is instructed to call the `generate_image` MCP tool with the selected image model.
3. When a chat model is selected, the agent can still generate images on request (using the admin-configured default image model) — the user doesn't have to switch models to get an image.
4. Generated images are saved to S3 as conversation artifacts and rendered inline in the chat thread with a download button.
5. The agent retains full conversational context across both text and image queries — "make it more blue" or "now do one for Team B" work naturally.

## Design

### Approach: MCP Tool for Image Generation

Rather than splitting inference into two paths (direct API for images vs. agent for chat), keep **all requests flowing through the agent**. The agent's chat model retains conversational context, decides when image generation is appropriate, and delegates the actual API call to a new `generate_image` MCP tool in `ps-mcp`.

This mirrors how the antigravity-image OpenCode plugin works (see [jkalasas/opencode-antigravity-image](https://github.com/jkalasas/opencode-antigravity-image)) — it registers a `generate_image` tool that the chat model calls. The key difference is we implement it as an MCP tool in our existing `ps-mcp` binary rather than as a standalone OpenCode plugin, since ps-mcp already has S3 access and provider API keys.

**Why MCP tool over OpenCode plugin:**
- ps-mcp is Rust, consistent with the rest of the codebase.
- ps-mcp already has S3 upload (`ArtifactStore`), session context, and artifact registration.
- Provider API keys are already passed to the container as env vars — the MCP server reads them directly.
- No additional npm dependencies or build steps in the agent container.
- The tool naturally integrates with the existing artifact upload/streaming pipeline.

**Why agent-mediated over direct server-side:**
- The chat model has full conversation context — it can interpret "make it more blue" or "now generate one for Team B" without re-explaining.
- The agent decides when to generate images vs. when to answer with text — no need for a separate `generation_mode` field or client-side routing logic.
- Multi-step workflows work naturally: "Query team metrics, generate a chart, AND create a banner image" all happen in one agent session.
- Consistent UX — all queries flow through the same streaming path, no special cases.

### Layer 1 — Model Catalogue: Detect Image Generation Models

**Files:** `crates/ps-reasoning/src/catalogue.rs`, `crates/ps-core/src/models/config.rs`

The catalogue needs to distinguish image-generation-only models from chat models so the model selector can filter them appropriately. Image-only models cannot be used as the agent's chat model — they can't reason, call tools, or hold conversations.

#### OpenRouter

The OpenRouter `/api/v1/models` response includes an `architecture` object:

```json
{
  "id": "openrouter/quasar-alpha",
  "architecture": {
    "modality": "text->text",
    "input_modalities": ["text"],
    "output_modalities": ["text"]
  }
}
```

Image generation models have `"modality": "text->image"` or `"output_modalities": ["image"]`. Some multimodal models may have `["text", "image"]` in output_modalities.

**Changes:**

1. Add `architecture` to the `OpenRouterModel` deserialisation:
   ```rust
   #[derive(serde::Deserialize)]
   struct OpenRouterArchitecture {
       #[serde(default)]
       modality: Option<String>,
       #[serde(default)]
       output_modalities: Vec<String>,
   }
   ```

2. Detect output modalities and assign capabilities accordingly:
   ```rust
   let (has_text_output, has_image_output) = match &m.architecture {
       Some(arch) => {
           let img = arch.output_modalities.iter().any(|m| m == "image")
               || arch.modality.as_deref().map_or(false, |m| m.contains("->image"));
           let txt = arch.output_modalities.iter().any(|m| m == "text")
               || arch.modality.as_deref().map_or(true, |m| m.contains("text"));
           (txt, img)
       }
       None => (true, false), // Default: assume text-only chat model
   };

   let mut capabilities = Vec::new();
   if has_text_output {
       capabilities.push("completion".into());
       capabilities.push("tool_use".into());
   }
   if has_image_output {
       capabilities.push("image_generation".into());
   }
   ```

3. Models with **only** image output should **not** get `"completion"` or `"tool_use"` — they can't be used as the agent's chat model.

#### Google Gemini

Google's model listing includes `supportedGenerationMethods`. Image-capable models (Imagen, Gemini 2.0+ with native image output) report `"generateImages"`.

**Changes:**
1. Check for `"generateImages"` in `supported_generation_methods` → add `"image_generation"` capability.
2. Gemini models that support both `"generateContent"` and `"generateImages"` get both `"completion"` and `"image_generation"` capabilities (they're multimodal).

#### Proto: expose `image_generation` as a capability string

No proto changes needed — capabilities are already `repeated string` in `AiModelInfo`. The frontend just needs to filter on the new `"image_generation"` value.

### Layer 2 — MCP Tool: `generate_image`

**Files:** `crates/ps-mcp/src/tools/generate_image.rs` (new), `crates/ps-mcp/src/tools/mod.rs`

The new MCP tool calls provider image generation APIs and returns the image as an S3 artifact — exactly like `upload_artifact` but with an API call prepended.

#### Tool Schema

```json
{
  "name": "generate_image",
  "description": "Generate an image using an AI image generation model. The image is saved as a conversation artifact and automatically displayed in the chat.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "prompt": {
        "type": "string",
        "description": "Detailed description of the image to generate"
      },
      "model": {
        "type": "string",
        "description": "Model ID to use (e.g. 'openai/dall-e-3', 'google/imagen-3'). Optional — uses the configured default image model if omitted."
      },
      "provider": {
        "type": "string",
        "enum": ["openrouter", "google"],
        "description": "Provider to use. Optional — inferred from model ID prefix if present."
      },
      "aspect_ratio": {
        "type": "string",
        "description": "Aspect ratio (e.g. '1:1', '16:9', '9:16'). Optional — defaults to '1:1'."
      }
    },
    "required": ["prompt"]
  }
}
```

#### Implementation Flow

1. **Resolve provider + model** — from explicit args, or fall back to a configured default image model in AI settings.
2. **Read API key** from environment (`OPENROUTER_API_KEY`, `GOOGLE_API_KEY` — already injected into the agent container by `pod_spec.rs`).
3. **Call the provider's image generation endpoint:**

   **OpenRouter** — standard `/api/v1/chat/completions` with the image model ID:
   ```json
   POST https://openrouter.ai/api/v1/chat/completions
   {
     "model": "stabilityai/nano-banana-2",
     "messages": [{"role": "user", "content": "a banana wearing sunglasses"}]
   }
   ```
   Response contains base64 image data in `choices[0].message.content` (either as `image_url.url` data URI or inline base64). Parse both formats.

   **Google Gemini** — `generateContent` with `responseModalities: ["IMAGE"]`:
   ```json
   POST https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent
   {
     "contents": [{"parts": [{"text": "a banana wearing sunglasses"}]}],
     "generationConfig": {"responseModalities": ["IMAGE"]}
   }
   ```
   Response contains `inlineData` parts with `mimeType` and base64 `data`.

4. **Decode image** — base64 decode the response payload, determine content type (from response headers, data URI prefix, or explicit `mimeType` field).
5. **Upload to S3** — via the existing `ArtifactStore::put()`, using key `conversations/{session_id}/{filename}.{ext}`.
6. **Register artifact in DB** — call the existing artifact registration endpoint (or directly insert via the MCP's gRPC connection to `ps-server`).
7. **Return tool result** — same format as `upload_artifact`:
   ```json
   {
     "status": "generated",
     "artifact_key": "conversations/{session_id}/generated-image-1743350400.png",
     "content_type": "image/png",
     "size_bytes": 245760,
     "display_name": "a banana wearing sunglasses"
   }
   ```

The existing event loop in `event_loop.rs` already intercepts `upload_artifact` tool completions and emits `artifact_uploaded` events. Extend this to also intercept `generate_image` completions — or better, have `generate_image` use the same output format so the existing interception logic works unchanged.

#### Error Handling

- **Rate limits (429)** — return a clear error message with retry-after time. The agent can relay this to the user.
- **Content blocked** — some providers reject prompts for safety. Return the block reason so the agent can suggest a modified prompt.
- **Model not found** — if the requested model isn't available, return available image models as a hint.

### Layer 3 — Agent Prompt: Image Generation Instructions

**Files:** `crates/ps-agent/agent-container/.opencode/agents/prism.md`

Add a section to the agent prompt:

```markdown
## Image generation

You can generate images using the `generate_image` MCP tool. Use it when the user asks
for an image, illustration, logo, diagram, or visual that isn't a data chart.

- For **data charts and graphs** — use Python (matplotlib/plotly) as before.
- For **creative images, illustrations, diagrams** — use `generate_image`.

The tool saves the image as a conversation artifact automatically. Do NOT include a
download link in your response — the UI shows the image inline with a download button.

When generating images, write a detailed prompt. Include style, composition, colours,
and mood. Example: instead of "a cat", write "a fluffy orange tabby cat sitting on a
windowsill, watercolour style, warm afternoon light, soft focus background".
```

### Layer 4 — Frontend: Unified Model Selector & Inline Images

**Files:** `frontend/views/ask/components/model-selector.tsx`, `frontend/views/ask/hooks/use-ask-question.ts`, `frontend/views/ask/components/query-input.tsx`, `frontend/views/ask/components/conversation-thread.tsx`, `frontend/views/ask/components/agent-response.tsx`, `frontend/views/ask/components/artifact-list.tsx`

#### Unified model selector with grouped models

The model selector becomes a single dropdown with two groups: chat models and image models. The user's selection determines the **intent** of the query — when an image model is selected, the frontend still sends the query through the agent (using the admin-configured chat model for reasoning), but includes the selected image model as metadata so the agent knows to call `generate_image` with that specific model.

**Selector layout:**
```
┌─────────────────────────────────┐
│ 🔍 Search models...             │
├─────────────────────────────────┤
│ ✦ Gemini 2.5 Flash (default)   │
│                                 │
│ Chat Models                     │
│ ✦ Gemini 2.5 Pro               │
│ ✦ Gemini 2.5 Flash             │
│ » Claude Sonnet 4.6            │
│ » Quasar Alpha                 │
│                                 │
│ Image Generation                │
│ 🖼 DALL-E 3                     │
│ 🖼 Stable Diffusion XL         │
│ 🖼 Nano Banana 2               │
│ 🖼 Imagen 3                    │
└─────────────────────────────────┘
```

**Changes to `model-selector.tsx`:**

1. Fetch models with **no capability filter** (or two parallel fetches: `tool_use` + `image_generation`), then group client-side.
2. Use `CommandGroup` with labels "Chat Models" and "Image Generation" to visually separate the two sections.
3. Image models show an `ImageIcon` (Lucide) instead of the provider icon.
4. The selected value encodes both the model ID and the intent:
   - Chat model selected → `onSelect("google/gemini-2.5-flash")` (existing behaviour)
   - Image model selected → `onSelect("image:openrouter/stabilityai/nano-banana-2")` — prefixed with `image:` to distinguish from chat model overrides.
5. The trigger button shows the model name. When an image model is active, the provider icon swaps to `ImageIcon`.

**Changes to `query-input.tsx`:**

1. When the selected model starts with `image:`, change the placeholder to "Describe an image..." instead of "Ask a question about your engineering data...".
2. Optionally show a small `ImageIcon` badge next to the input to reinforce the mode.

**Changes to `use-ask-question.ts` / backend flow:**

When the user submits a query with an image model selected:

1. The frontend sends `AskQuestionRequest` with the **chat model** as `model_override` (or no override, using the default) — the agent still needs a reasoning model.
2. The image model preference is passed as a new proto field:
   ```protobuf
   message AskQuestionRequest {
     string question = 1;
     optional string conversation_id = 2;
     optional string model_override = 3;
     // When set, the agent should use this model for image generation.
     // Format: "provider/model_id".
     optional string image_model = 4;
   }
   ```
3. The server passes `image_model` through to the Restate handler, which injects it into the agent's system prompt or as an env var (`OPENCODE_IMAGE_MODEL`).
4. The agent prompt instructs: "When `image_model` is set, the user expects an image. Call `generate_image` with this model and the user's prompt."

**Alternative: pass via system prompt injection**

Instead of a proto field, the server could prepend a system instruction to the user's question when an image model is selected:

```
[System: The user has selected image model "openrouter/stabilityai/nano-banana-2".
Generate an image using the generate_image tool with this model.]

User's actual prompt here
```

This avoids a proto change but is less clean. **Recommendation:** Use the proto field — it's explicit, type-safe, and the agent prompt can reference it as a known context variable. The server injects it as an env var that the MCP tool reads as its default model.

#### Inline image rendering in chat

When a message's artifacts include images (`content_type` starts with `image/`), render them inline in the conversation thread:

**New component: `InlineImage`**
```tsx
const InlineImage = ({ artifact }: { artifact: ArtifactDisplay }) => {
  // 1. Fetch presigned URL from GetArtifactDownloadUrl on mount
  // 2. Render <img> with loading skeleton
  // 3. Show download button with size/format metadata below
};
```

**Layout:**
```
┌──────────────────────────────────┐
│  ┌─────────────────────┐         │
│  │   Generated image   │         │
│  │   (max-w-lg)        │         │
│  └─────────────────────┘         │
│                                  │
│  ⬇ Download · 1.2 MB · PNG      │
│                                  │
│  "Here's your generated image."  │
└──────────────────────────────────┘
```

**Changes:**
1. In `AgentResponse` (or `ConversationThread`), separate image artifacts from non-image artifacts.
2. Image artifacts render with `<InlineImage>` above the answer text.
3. Non-image artifacts continue to use the existing `<ArtifactList>`.
4. `<InlineImage>` component:
   - Fetches the presigned URL from `GetArtifactDownloadUrl`.
   - Renders `<img>` with `max-w-lg rounded-lg shadow-sm`.
   - Shows a download button below with size and format info.
   - Loading state: `<Skeleton>` placeholder.

#### Download button

```tsx
<div className="flex items-center gap-2 text-sm text-muted-foreground">
  <Button variant="ghost" size="sm" onClick={() => download(artifact.id, artifact.displayName)}>
    <Download className="mr-1.5 size-3.5" />
    Download
  </Button>
  <span>{formatSize(artifact.sizeBytes)}</span>
  <span>·</span>
  <span>{artifact.contentType?.split("/")[1]?.toUpperCase()}</span>
</div>
```

### Layer 5 — AI Settings: Default Image Model Configuration

**Files:** `frontend/views/admin/`, `proto/canonical/prism/v1/reasoning.proto`, `crates/ps-server/src/services/reasoning/`

Add a `default_image_model` setting in the AI admin panel (alongside the existing `agentic.model` and `enrichment.model` settings). This is the fallback model the `generate_image` tool uses when the user hasn't explicitly selected an image model from the dropdown.

Proto change to `AiSettings` (or the agentic sub-config):
```protobuf
message AgenticConfig {
  string model = 1;
  AiProvider provider = 2;
  string small_model = 3;
  AiProvider small_provider = 4;
  // Default model for image generation via the generate_image tool.
  optional string image_model = 5;
  optional AiProvider image_provider = 6;
}
```

The MCP tool reads the image model from two sources, in priority order:
1. **Per-query override** — `OPENCODE_IMAGE_MODEL` env var, set by `pod_spec.rs` when the user selects an image model from the dropdown (passed via the `image_model` field on `AskQuestionRequest`).
2. **Admin default** — `DEFAULT_IMAGE_MODEL` env var, set from the `AgenticConfig.image_model` setting.

This means: when a user picks an image model in the selector, that exact model is used. When a chat model user asks "generate an image of X" without selecting an image model, the admin-configured default is used. If neither is set, the tool returns an error telling the agent no image model is configured.

## Implementation Phases

### Phase A — Catalogue: Modality Detection (0.5 day)

1. Parse `architecture` from OpenRouter API response in `catalogue.rs`.
2. Set `"image_generation"` capability on image-output models.
3. Stop assigning `"completion"` / `"tool_use"` to image-only models.
4. Check for `"generateImages"` in Google's `supported_generation_methods`.

### Phase B — MCP Tool: `generate_image` (2 days)

1. Create `crates/ps-mcp/src/tools/generate_image.rs`.
2. Implement OpenRouter image generation (standard chat completions endpoint with image models).
3. Implement Google Gemini image generation (`generateContent` with `responseModalities: ["IMAGE"]`).
4. Decode base64 response → upload to S3 → register artifact → return result.
5. Use the same output format as `upload_artifact` so the existing event loop interception works.
6. Register the tool in `ps-mcp/src/tools/mod.rs`.
7. Read model from `OPENCODE_IMAGE_MODEL` env var (per-query override) falling back to `DEFAULT_IMAGE_MODEL` (admin default).

### Phase C — Unified Model Selector (1 day)

1. Fetch models with no capability filter (or parallel `tool_use` + `image_generation` fetches).
2. Group into "Chat Models" and "Image Generation" sections using `CommandGroup`.
3. Image model selection encodes the `image:provider/model_id` prefix.
4. Parse the prefix in `use-ask-question.ts` — send `model_override` as the chat model (default) and `image_model` as the selected image model.
5. Update input placeholder to "Describe an image..." when an image model is selected.

### Phase D — Backend Plumbing (0.5 day)

1. Add `image_model` field to `AskQuestionRequest` proto.
2. Pass `image_model` through `agent_query.rs` → Restate trigger → `pod_spec.rs` as `OPENCODE_IMAGE_MODEL` env var.
3. Add `image_model` / `image_provider` to `AgenticConfig` proto for admin defaults.
4. Pass admin default as `DEFAULT_IMAGE_MODEL` env var.

### Phase E — Agent Prompt & Inline Rendering (1 day)

1. Add image generation section to `prism.md` agent prompt — instruct the agent to call `generate_image` when `OPENCODE_IMAGE_MODEL` is set or when the user asks for a creative image.
2. Create `InlineImage` component for the frontend.
3. Detect image artifacts in `AgentResponse` and render inline above the answer text.
4. Add download button with size/format metadata.
5. Keep `ArtifactList` for non-image artifacts.

### Phase F — Test Coverage (1.5 days)

Tests follow the existing patterns in the codebase: colocated `#[cfg(test)]` for Rust unit tests, `define_api_test!`/`define_repo_test!` for integration tests, vitest + RTL + `createRouterTransport` for frontend.

#### F.1 — MCP Tool Unit Tests (`crates/ps-mcp/src/tools/`)

Existing coverage in `mod.rs` is metadata-only (tool count, names, descriptions). Add execution-level tests for the new tool.

**`generate_image.rs` — `#[cfg(test)]` module:**

1. **Provider resolution** — model ID prefix `"google/"` resolves to Google provider; `"openrouter/"` or unprefixed resolves to OpenRouter. Missing model + no default → clear error message.
2. **OpenRouter response parsing** — parse base64 image from `choices[0].message.content` in both `image_url.url` data URI format and inline base64 format. Malformed JSON → descriptive error.
3. **Google Gemini response parsing** — parse `inlineData` parts with `mimeType` and base64 `data`. Missing parts or unexpected structure → descriptive error.
4. **Content type detection** — correctly identifies PNG, JPEG, WebP from response metadata / data URI prefix / explicit `mimeType` field.
5. **S3 key format** — generated key matches `conversations/{session_id}/generated-image-{timestamp}.{ext}`.
6. **Error mapping** — 429 → rate limit message with retry-after; 400 content-blocked → block reason surfaced; 404 model not found → hint with available models.
7. **Empty/missing prompt** — returns validation error, doesn't call provider API.

**`mod.rs` — update existing metadata test:**

8. **Tool count** — update `tool_router_registers_all_11_tools` → 12 tools (add `generate_image`).

Use `wiremock::MockServer` for HTTP assertions (consistent with ingestion source tests). Use `object_store::memory::InMemory` for S3 assertions (no real bucket needed).

#### F.2 — Worker Tests (`crates/ps-workers/src/features/reasoning/agentic_query/`)

The event loop and artifact handler currently have **zero test coverage**. Add targeted tests for the new interception path.

**`artifact.rs` — `#[cfg(test)]` module:**

1. **`prism_generate_image` tool completion registers artifact** — construct a synthetic SSE event with tool name `"prism_generate_image"` and the standard JSON output (`artifact_key`, `display_name`, `content_type`, `size_bytes`). Verify `repos.reasoning.create_artifact()` is called with the correct values.
2. **`prism_upload_artifact` still works** — same test shape as above but with existing tool name, confirming the filter extension didn't break the original path.
3. **Malformed JSON output** — tool result is not valid JSON or missing required fields (`artifact_key`, `display_name`). Verify graceful no-op (no panic, no DB write), consistent with existing `unwrap_or_default()` behaviour.
4. **Unrelated tool ignored** — tool completion for `"prism_query_team_metrics"` does not trigger artifact registration.
5. **Missing optional fields** — `content_type` and `size_bytes` absent from JSON. Verify artifact created with defaults/nulls.

These are `#[tokio::test]` tests using a real test database via `define_repo_test!` (artifact registration writes to the DB).

#### F.3 — Model Catalogue Tests (`crates/ps-workers/src/features/reasoning/`)

**`catalogue.rs` — `#[cfg(test)]` module (extend existing tests if any, or add new):**

1. **OpenRouter image-only model** — `architecture.modality: "text->image"` → capabilities = `["image_generation"]` only, no `"completion"` or `"tool_use"`.
2. **OpenRouter multimodal model** — `output_modalities: ["text", "image"]` → capabilities include both `"completion"`, `"tool_use"`, and `"image_generation"`.
3. **OpenRouter text-only model** — `modality: "text->text"` → capabilities = `["completion", "tool_use"]`, no `"image_generation"`. Unchanged from current behaviour.
4. **OpenRouter missing architecture** — no `architecture` field → default to text-only capabilities. Backwards compatible.
5. **Google `generateImages` method** — model with `supportedGenerationMethods: ["generateContent", "generateImages"]` → capabilities include `"image_generation"` alongside `"completion"`.
6. **Google image-only model** — `supportedGenerationMethods: ["generateImages"]` only → `"image_generation"` capability, no `"completion"`.

Use `wiremock` to mock the OpenRouter/Google model list endpoints, consistent with existing catalogue refresh integration tests.

#### F.4 — Integration Tests (`tests/integration/src/api/reasoning.rs`)

Extend the existing reasoning API test suite using `define_api_test!`.

1. **AI settings round-trip with image model** — set `image_model` and `image_provider` in `AgenticConfig` via `UpdateAiSettings`, read back via `GetAiSettings`, verify values persisted.
2. **Model catalogue includes image capability** — after catalogue refresh (mocked OpenRouter response with mixed model types), verify `ListAiModels` returns models with `"image_generation"` capability and that image-only models lack `"completion"`.
3. **AskQuestion with `image_model` field** — send `AskQuestionRequest` with `image_model` set, verify it flows through to the handler (this is a plumbing test — the agent container isn't running in integration tests, so verify the field is accepted and doesn't error at the API layer).
4. **Artifact DB registration for generated images** — use `define_repo_test!` to directly test `repos.reasoning.create_artifact()` with image content types (`image/png`, `image/webp`), then verify retrieval and listing includes correct metadata.

#### F.5 — Frontend Tests (`frontend/views/ask/`)

Follow existing patterns: vitest + RTL + `renderWithProviders()` + `createRouterTransport` mocks.

**`components/model-selector.test.tsx` (new file):**

1. **Groups models by type** — render with mixed chat + image models, verify two `CommandGroup` sections ("Chat Models", "Image Generation") are present.
2. **Image model selection emits prefixed value** — click an image model, verify `onSelect` called with `"image:provider/model_id"` prefix.
3. **Chat model selection emits plain value** — click a chat model, verify `onSelect` called with plain model ID (no prefix).
4. **Image models show ImageIcon** — verify image models render with the image icon, not the provider icon.
5. **Search filters both groups** — type in search, verify filtering applies across both chat and image sections.
6. **Empty image group hidden** — when no image models exist, the "Image Generation" group header is not rendered.

**`components/query-input.test.tsx` (extend existing):**

7. **Placeholder changes for image model** — when selected model starts with `"image:"`, placeholder text is "Describe an image...".
8. **Placeholder reverts for chat model** — when model is plain ID, placeholder is the default text.

**`components/inline-image.test.tsx` (new file):**

9. **Renders image with presigned URL** — mock `GetArtifactDownloadUrl` response, verify `<img>` element rendered with the URL and appropriate `max-w-lg rounded-lg` classes.
10. **Loading skeleton while fetching URL** — before URL resolves, verify `Skeleton` placeholder is shown.
11. **Download button shows size and format** — verify download button text includes formatted file size and content type (e.g. "1.2 MB · PNG").
12. **Error state** — when presigned URL fetch fails, verify fallback UI (not a blank space or crash).

**`components/agent-response.test.tsx` or `conversation-thread.test.tsx` (extend or new):**

13. **Image artifacts rendered inline, non-image in ArtifactList** — provide a message with both image and non-image artifacts. Verify image artifacts use `InlineImage` component and non-image artifacts use existing `ArtifactList`.
14. **Multiple image artifacts** — message with 2+ image artifacts renders all of them in order.
15. **No artifacts** — message with no artifacts renders neither `InlineImage` nor `ArtifactList`.

**`hooks/use-ask-question.test.ts` (extend or new):**

16. **Image model prefix parsed into separate fields** — when selected model is `"image:google/gemini-3.1-flash-image-preview"`, the hook sends `model_override` as the default chat model and `image_model` as `"google/gemini-3.1-flash-image-preview"`.
17. **Chat model sent as model_override only** — when selected model is `"google/gemini-2.5-flash"`, `image_model` is not set in the request.

## Decisions (formerly Open Questions)

1. **Default image model** — Google provider, model ID `gemini-3.1-flash-image-preview` (marketed as "Nano Banana 2"). Configurable in AI settings admin panel via `AgenticConfig.image_model` / `image_provider`.
2. **Image size/quality controls** — start with model defaults only. The `aspect_ratio` parameter is available in the tool schema but no explicit size/quality controls. The user can add details to their prompt (e.g. "make it 16:9", "high detail") and the agent will translate that into appropriate tool parameters or prompt phrasing. Consider exposing more controls later based on usage patterns.
3. **Multi-image generation** — start with `n = 1`. Revisit multi-image support later if there's demand.
4. **Image editing/inpainting** — out of scope. The MCP tool schema is extensible (add `input_image` parameter later).
5. **Rate limiting** — the existing daily budget cap in `TaskRouter` covers this. Per-image cost tracking via `reasoning.ai_cost_log` ensures visibility.
6. **Event loop interception** — `generate_image` needs its own tool name check in the event loop, but reuses the same pattern and artifact registration logic as `upload_artifact`. Investigation confirmed that interception in `event_loop.rs` is keyed on tool name (checks for `"prism_upload_artifact"` in `artifact.rs:28`), not on result shape. The fix is straightforward: extend `handle_artifact_upload()` to also match `"prism_generate_image"`, since both tools output the same JSON shape (`artifact_key`, `display_name`, `content_type`, `size_bytes`). No new handler function needed — just a second tool name in the existing filter.
7. **Selector UX when image model active** — placeholder text change to "Describe an image..." is sufficient for now. Iterate based on feedback.
