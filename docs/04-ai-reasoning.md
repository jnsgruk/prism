# AI and Reasoning

Prism uses AI to enrich engineering data, compute embeddings for similarity search, generate insights, and provide a natural-language query interface. Every AI-generated output must be auditable back to source data.

## Rig Framework

LLM integration is built on the Rig framework (`rig-core`), which provides `CompletionModel` and `EmbeddingModel` traits, structured extraction via derive macros, and agent orchestration. This replaced ~1,100 lines of hand-rolled provider abstraction.

**TaskRouter** in ps-reasoning routes requests to the appropriate model based on task type (enrichment, embedding, insight generation). Currently Google Gemini is the only supported provider.

The model catalogue (`ModelCatalogueHandler`, a Restate service) fetches available models from the provider API and caches them in the `reasoning` schema. The frontend presents these as a dropdown rather than free-text input.

## Enrichment Pipeline

1. **Capture** — during ingestion, rich content (PR descriptions, review comments, issue bodies) is queued in `reasoning.enrichment_queue`
2. **Process** — `EnrichmentHandler` (Restate service) picks batches from the queue, runs structured extraction via Rig extractors (sentiment, complexity, key themes, summary)
3. **Store** — results saved to `reasoning.enrichments` with model name, input hash, confidence score, and full prompt for auditability
4. **Cost tracking** — API usage logged in `reasoning.api_usage` (tokens in/out, model, cost)

Enrichments are fire-and-forget from ingestion — triggered as downstream handlers after successful data ingestion.

## Embeddings and Similarity Search

Embeddings are computed by `EmbeddingHandler` (Restate service) using Rig's `EmbeddingModel` trait. Vectors are stored in the `reasoning` schema using pgvector with IVFFlat indexes for approximate nearest-neighbour queries.

Key APIs:
- **FindSimilar** — given a contribution ID, find semantically similar contributions
- **SearchByText** — given a text query, find relevant contributions via embedding similarity

The embedding queue works similarly to the enrichment queue — items are queued during ingestion and processed asynchronously.

## Agentic Query

Natural-language questions about engineering data are handled by an agentic architecture:

### Architecture

1. **ps-server** receives a question via the `AskQuestion` gRPC streaming RPC
2. **Restate** runs `prepare_query` in `AgenticQueryHandler` — this handles durable pod lifecycle only (~90s):
   - Claims the conversation atomically via CAS update
   - Creates an ephemeral K8s pod running OpenCode with ps-mcp as the MCP server
   - The pod mounts the shared `prism-workspaces` PVC at `/workspace` via `subPath: {conversation_id}`
   - Waits for pod readiness
3. **ps-server** streams SSE events directly from the OpenCode pod to the gRPC client — this avoids Restate's journal/timeout issues with long-running non-journaled work
4. **QueryWatchdogHandler** (Restate, singleton key) runs every 60s to reset stuck conversations

### Why OpenCode in Pods?

- Battle-tested agent orchestration (tool-call -> execute -> reprompt loop)
- MCP stdio transport lets Prism provide data tools without modifying the agent framework
- Container isolation — agents can safely run code analysis tools (git, rg, tokei) without risk to the main system
- Session management within container lifetime; conversation history persisted in DB for multi-turn + resume

### Workspace Storage

Each conversation gets an isolated directory on the shared `prism-workspaces` PVC (ReadWriteMany, 50Gi). Agent pods mount it at `/workspace` with `subPath: {conversation_id}`, so each agent sees only its own files. ps-server mounts the same PVC read-only at `/workspaces` and serves file listings via `ListWorkspaceFiles` and streamed content via `DownloadWorkspaceFile` (64KB chunked gRPC stream). Files appear in the workspace sidebar as soon as the agent writes them.

When a user deletes a conversation, the `cleanup_storage` Restate handler deletes both the agent pod and the workspace directory from the PVC. Pod expiry (idle/max lifetime) does **not** delete workspace files — users can browse completed conversations. ps-workers mounts the PVC read-write for this cleanup.

### ps-mcp — Data Tools

The MCP server running inside agent containers provides:
- **Data query tools** — query team metrics, search contributions, find people, explore trends
- **Image generation** — generate images via AI models, saved to `/workspace`

### Why SSE Streaming Lives in ps-server

Initially, SSE streaming ran inside Restate handlers. Restate's 5-minute `ABORT_TIMEOUT` caused races: streams suspended mid-way, replay logic deleted recovery data, handlers retried forever. Moving streaming to ps-server eliminated these issues — ps-server already holds gRPC streams open for the duration, and Restate handles only the fast pod lifecycle.

## Traceability

Every metric, insight, or AI-generated output must be auditable back to source data:
- Static metrics link to contributing data points
- AI enrichments store model name, input, prompt, and confidence
- The UI provides a "show how this was calculated" affordance
- Cost tracking records token usage and model for every API call
