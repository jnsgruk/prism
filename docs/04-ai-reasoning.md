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

Queue processing captures the queue ID and source content hash alongside its
input. Bulk and individual retry writes acquire ingestion's natural-key locks,
then contribution and queue locks in consistent order, and save only results whose captured queue ID and content hash still match.
`reasoning.enrichments.source_content_hash` records this provenance separately
from the formatted prompt's `input_hash`. Selection and cleanup require matching
source hashes for every applicable type. Cleanup acquires the same locks and
rechecks hashes and PR size eligibility after waiting for concurrent ingestion.
Queue upserts also order contribution IDs to avoid reversed batch lock order.
A response arriving after an input
change is discarded and does not count as committed; the replacement remains
queued and keeps historical insight invalidations pending. Legacy results with
unknown source hashes cannot satisfy newly queued work.

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
2. **Restate** runs `prepare_query` in `AgenticQueryHandler` — this handles durable pod lifecycle only:
   - Claims the conversation atomically via CAS update
   - Creates an ephemeral K8s pod running OpenCode with ps-mcp as the MCP server
   - The pod mounts the shared `prism-workspaces` PVC at `/workspace` via `subPath: {conversation_id}`
   - Waits for a pod IP and OpenCode application health within a shared 60-second wait budget
3. **ps-server** streams SSE events directly from the OpenCode pod to the gRPC client — this avoids Restate's journal/timeout issues with long-running non-journaled work
4. **QueryWatchdogHandler** (Restate, singleton key) runs every 60s to reset stuck conversations

The server subscribes to raw OpenCode SSE frames because the Rust SDK's typed
stream does not recognise `message.part.delta`. Prism reconstructs cumulative
text and reasoning snapshots from these deltas and forwards them immediately.
Part IDs provide stable ordering across messages; completed snapshots replace
the accumulated content without duplicating it. User message parts are excluded
from assistant output.

### Startup readiness and session safety

Kubernetes phase `Running` means the container has started, not that OpenCode
is reachable. Pods expose an HTTP readiness probe on the supported, inexpensive
[`GET /global/health`](https://opencode.ai/docs/server/#global) endpoint. The
worker polls the same endpoint after obtaining the pod IP; ps-server checks it
again from its own network path before resolving a session. Health requires a
successful HTTP response with `healthy: true`; it does not initialise the
project directory, MCP tools, or model providers.

Startup has one two-minute deadline in ps-server covering Restate preparation,
health, session resolution and SSE connection. Direct startup HTTP uses no
proxy, a one-second connection deadline, three-second read requests and
250 ms polling. Session POST has a 15-second request deadline. The entire SSE
handshake has a 15-second deadline. Normal SDK operations keep their 120-second
timeout and streaming retains the existing ten-minute query budget.

Session resolution uses `GET /session/{id}`, rather than fetching every message.
Only a definitive 404 permits replacement; transient failures retry reads within
the original deadline and preserve the stored ID. Before creating a session,
ps-server checks for its stable title (`Prism conversation {conversation_id}`)
and persists `pending:{pod_uid}` in the internal `opencode_session_id` column.
POST is sent once. An ambiguous timeout, invalid response, or failure to persist
the returned ID is recovered by listing sessions and matching that title.
Follow-up requests on the same Kubernetes UID only reconcile a pending creation;
a different UID after expiry permits fresh creation with a recap of prior turns.
No migration or SQL cache change is needed.

This deliberately favours avoiding duplicate sessions: a crash between saving
intent and sending POST, or a failed POST that created nothing, leaves that pod
in reconciliation-only mode until it expires or cancellation replaces it. The
UI receives a retryable startup error at the deadline. OpenCode has no supported
client-supplied session ID or idempotency key in the inspected API. Deploy
ps-workers before ps-server, since the prepare response now supplies `pod_uid`;
ps-server fails closed if it is absent. The worker's journaled step names,
sequence and result types remain unchanged, so existing Restate invocations
need no cancellation or journal reset.

Startup progress is persisted as container events for browser reconnection.
Cancellation interrupts preparation, health/session resolution, SSE handshake
and prompt submission. Logs report scheduling, application health attempts,
session resolution, SSE connection and cumulative startup durations. No session
or prompt work is moved into the Restate journal; SSE stays in ps-server.

An isolated development pod on 2026-10-01 reached `Running` at 5.55 s, returned
connection refused then a one-second HTTP timeout, and became healthy at 7.16 s.
The fixed client subsequently measured health 1 ms, initial session POST 204 ms,
session reuse 2 ms and SSE handshake 11 ms (244 ms total). A second check ran the actual startup client against a fresh diagnostic pod
with a deliberate five-second delay before `opencode serve`: health polling
recovered after 16 attempts in 6.787 s (including a three-second stalled health
request), creation took 287 ms, reuse 13 ms and SSE 10 ms, for 7.127 s total.
No model prompt or live conversation was used. These observations demonstrate a real readiness /
network reachability gap; they do not identify the packet-level cause of the
original 120-second failure. The historical server logs lack connection-phase
and TCP details. Validation used direct host-to-pod traffic and pod-local
requests, not a rollout of ps-server / ps-workers or a real model query. The
only live NetworkPolicy restricts agent egress and does not isolate ingress;
neither service deployment declares proxy environment variables. The image tested was OpenCode 1.18.34; the Dockerfile currently
installs the latest release despite declaring an unused version argument.

For a repeatable check, create an isolated pod using the development agent image
and run `PRISM_STARTUP_PROBE_URL=http://<diagnostic-pod-ip>:4096 cargo nextest run
-p ps-server -E 'test(live_readiness_create_reuse_and_sse)' --run-ignored only
--no-capture`. This creates a diagnostic session; never target a user's agent.
Remove only the diagnostic pod afterwards. Regression tests also cover delayed
health, transient refused connections, HTTP lookup failures, ambiguous creation,
shared-budget exhaustion, persistent pending creation, expiry recovery,
follow-up reuse, browser reconnect and startup cancellation against Wiremock
and isolated PostgreSQL.

### Why OpenCode in Pods?

- Battle-tested agent orchestration (tool-call -> execute -> reprompt loop)
- MCP stdio transport lets Prism provide data tools without modifying the agent framework
- Container isolation — agents can safely run code analysis tools (git, rg, tokei) without risk to the main system
- Session management within container lifetime; conversation history persisted in DB for multi-turn + resume

### Workspace Storage

Each conversation gets an isolated directory on the shared `prism-workspaces` PVC (ReadWriteMany, 50Gi). Agent pods mount it at `/workspace` with `subPath: {conversation_id}`, so each agent sees only its own files. ps-server mounts the same PVC read-only at `/workspaces` and serves file listings via `ListWorkspaceFiles` and streamed content via `DownloadWorkspaceFile` (64KB chunked gRPC stream). Files appear in the workspace sidebar as soon as the agent writes them.

When a user deletes a conversation, the `cleanup_storage` Restate handler deletes both the agent pod and the workspace directory from the PVC. Pod expiry (idle/max lifetime) does **not** delete workspace files — users can browse completed conversations. ps-workers mounts the PVC read-write for this cleanup.

### Generated-file references

Agents refer to files in their own workspace as `/workspace/<path>`. This is an
agent filesystem reference, not a public browser endpoint. The application
translates verified non-image references to
`/ask/<conversation-id>/files/<path encoded by segment>`. Do not persist blob or
data URLs, invent download hostnames, or expose `/workspace` as a static server.

`ResolveWorkspaceFiles` checks bounded batches of workspace-relative paths and
returns availability, MIME type and size without transferring the contents.
Resolution and download require an authenticated session and an existing
conversation. Prism's current shared-conversation read policy permits any
signed-in user to read an existing conversation; conversation listings remain
filtered to their owner. These URLs do not grant anonymous access.

The resolver checks a readable regular file confined to the canonical
conversation workspace. Missing files, directories, unsafe paths and storage
failures cannot become verified downloads. Download rechecks availability;
verification is a point-in-time observation, so deletion between verification
and download must produce an error. Streaming transfers include metadata for
empty files and propagate failures rather than certifying partial output.

Completed answers are validated before persistence and final-answer emission.
Only verified file links receive the browser URL; unavailable targets receive
an explanation. Images keep workspace image rendering. Live text remains
streamed while the frontend verifies references, and finalisation refreshes
availability. Existing Markdown is repaired at render time without rewriting
history or regenerating files. Resume/reload uses the same persisted answer.

API paths contain decoded workspace-relative filenames. URL paths encode each
segment separately and decode once at the reference/route boundary, never again
inside the filesystem API. Spaces, Unicode, literal `%`, `#` and `?` therefore
survive the round trip; a literal `%2F` filename uses `%252F` in a browser URL.
Traversal, empty segments, backslashes and encoded separators are rejected.
See `fixtures/workspace-paths.json`, consumed by Rust and TypeScript tests, for
exact encoding examples.

Files outlive idle/max-lifetime agent pod expiry because they live on the shared
PVC. Conversation deletion revokes access immediately and schedules storage
cleanup; a retained file on disk does not make a deleted conversation readable.

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

## Backfilled historical insights

Person backfills retain separate raw-metric and post-enrichment invalidations for
old/new contribution periods. Historical insights recompute only after the
changed contribution leaves the enrichment queue; unavailable AI work stays
pending and visible instead of being acknowledged. Terminal-pipeline recovery
retries that work after failed or cancelled ingestion. Snapshot source links are
replaced with the recalculated result, including clearing links after timestamp
or attribution corrections. Individual insights continue to query raw history.

The enrichment and embedding queues remain shared. Person ingestion enqueues
only eligible changed contributions, while the owned processing stages can
drain existing queue entries too. Pipeline ownership identifies cancellation
and progress; it does not imply a separate person-only AI queue.
