# Plan 56: Agentic Query Interface (Phase 3 — W3)

> **Implementation note:** The plan refers to the MCP crate as `ps-agent-mcp` throughout, but it was implemented as **`ps-mcp`** (shorter name). The binary is `ps-mcp`, the crate lives at `crates/ps-mcp/`, and the Dockerfile references `/usr/local/bin/ps-mcp`. The agent lifecycle crate is `ps-agent` at `crates/ps-agent/` (container manager, event mapper, pod spec). The agent container files live at `crates/ps-agent/agent-container/`.

## Context

This plan details the implementation of **W3: Agentic Query Interface** from [Phase 3](./14-phase3-intelligence.md). W0 (provider foundation), W1 (enrichment pipeline), and W2 (embeddings & similarity) are complete. The infrastructure they provide — `TaskRouter` with Rig clients, `CostTracker`, enrichment data in `reasoning.enrichments`, vector embeddings in `reasoning.embeddings`, and insight snapshots in `reasoning.insight_snapshots` — forms the foundation for the agentic layer.

**Goal:** Users can ask natural-language questions about their engineering data and receive sourced, auditable answers with full reasoning traces. The agent runs in an isolated Ubuntu container powered by [OpenCode](https://opencode.ai) with access to both Prism data tools (via MCP) and real system tools (git, rg, grep, tokei, uv/python, etc.), enabling deep repository analysis alongside metrics queries. Every claim cites its source. The UI streams the agent's thinking process in real time. Generated files (reports, analysis outputs) are stored as conversation artifacts in S3/RustFS.

**Dependencies:**
- [14-phase3-intelligence.md](./14-phase3-intelligence.md) — parent plan (W3 section)
- [06-ai-reasoning.md](./06-ai-reasoning.md) — tool design, agentic architecture, container analysis
- [40-adopt-rig-framework.md](./40-adopt-rig-framework.md) — Rig for enrichment/embeddings (W1/W2); agent layer uses OpenCode instead
- [01-architecture-overview.md](./01-architecture-overview.md) — object storage strategy (RustFS/S3)

**Key technology:**
- **OpenCode** ([anomalyco/opencode](https://github.com/anomalyco/opencode)) — open-source AI coding agent with built-in tools (read, write, edit, bash, grep, glob, webfetch), MCP server support, plugin hooks, multi-provider support (75+ providers), and session management
- **`opencode-sdk`** (Rust crate, [docs.rs/opencode-sdk](https://docs.rs/opencode-sdk)) — native Rust client for OpenCode's HTTP + SSE API, used by ps-server to control agent containers
- **`ps-agent-mcp`** (new Rust crate) — MCP stdio server binary providing Prism data tools + S3 artifact tools, spawned by OpenCode inside the container
- **Ephemeral K8s Pods** — one container per chat session, reaped after idle timeout
- **gRPC server streaming** — `AskQuestion` RPC returns `stream AgentEvent`
- **S3/RustFS** — artifact storage for generated files, reports, and analysis outputs

---

## Architecture Overview

```
┌──────────────────────────────────────────────────────────────────────────┐
│ Frontend: /ask                                                           │
│  ┌──────────────┐  ┌─────────────────────────────────────────────────┐  │
│  │ QueryInput    │  │ ConversationThread                              │  │
│  │ (textarea +   │  │  UserMessage → AgentResponse (streaming)       │  │
│  │  send btn)    │  │  ThinkingSteps (tool calls, bash, file reads)  │  │
│  └──────┬────────┘  │  AnswerContent (markdown + citations)          │  │
│         │           │  Artifacts (downloadable files)                 │  │
│         │           └─────────────────────────────────────────────────┘  │
└─────────┼───────────────────────────────────────────────────────────────┘
          │ gRPC server streaming (AskQuestion)
          ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ ps-server: ReasoningService                                              │
│  ┌────────────────────────┐  ┌──────────────────────────────────────┐   │
│  │ AskQuestion handler     │  │ ContainerManager                     │   │
│  │  1. Find or create Pod  │  │  - create_pod(session_id)           │   │
│  │  2. Connect via SDK     │  │  - get_pod(session_id)              │   │
│  │  3. Send prompt          │  │  - reap_idle_pods()                │   │
│  │  4. Stream events back  │  │  Uses kube-rs (K8s API)            │   │
│  │  5. Store conversation   │  └──────────────────────────────────────┘   │
│  │  6. Upload artifacts     │                                            │
│  └────────────────────────┘                                              │
└─────────┬───────────────────────────────────────────────────────────────┘
          │ opencode-sdk (Rust crate, HTTP + SSE to OpenCode server)
          ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ Agent Container (K8s Pod, 1 per chat session)                            │
│                                                                          │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │ OpenCode Server (port 4096)                                      │    │
│  │  - Built-in agent with custom system prompt                      │    │
│  │  - Session management (multi-turn within container lifetime)     │    │
│  │  - Event streaming via SDK                                       │    │
│  └───────────┬────────────────────┬────────────────────────────────┘    │
│              │                    │                                      │
│  ┌───────────▼──────────┐  ┌─────▼──────────────────────────────┐      │
│  │ Built-in Tools        │  │ ps-agent-mcp (Rust binary, stdio)  │      │
│  │  bash (git, rg, grep, │  │  query_team_metrics()              │      │
│  │    tokei, uv, python)  │  │  query_contributions()             │      │
│  │  read, write, edit     │  │  compare_teams()                   │      │
│  │  glob, grep            │  │  get_person_profile()              │      │
│  │  webfetch              │  │  search_similar()                  │      │
│  │  patch                 │  │  search_by_text()                  │      │
│  └───────────────────────┘  │  query_enrichments()                │      │
│                              │  list_teams(), list_people()        │      │
│  ┌───────────────────────┐  │  upload_artifact()                  │      │
│  │ /workspace/            │  │  list_artifacts()                   │      │
│  │  (cloned repos,        │  │                                     │      │
│  │   analysis outputs,    │  │  Uses ps-proto types, calls        │      │
│  │   generated reports)   │  │  ps-server gRPC via tonic client   │      │
│  └───────────────────────┘  └─────────────────────────────────────┘      │
│                                                                          │
│  Ubuntu 24.04 (Chisel) + git + rg + tokei + uv + OpenCode              │
└──────────────────────────────────────────────────────────────────────────┘
```

### Why OpenCode in a Container?

1. **Built-in tools for free** — OpenCode provides battle-tested file operations (read, write, edit, patch), code search (grep, glob), command execution (bash), and web access (webfetch). We don't build these ourselves.

2. **Multi-provider support** — OpenCode supports 75+ LLM providers via the AI SDK. We configure it to use Anthropic by default but any provider works — matching Prism's provider-agnostic philosophy.

3. **MCP integration** — custom Prism tools (query metrics, search contributions, etc.) are provided by `ps-agent-mcp`, a Rust binary implementing the MCP stdio transport. OpenCode spawns it as a local MCP server and exposes its tools to the LLM alongside built-in tools. Because it's Rust, it shares `ps-proto` types with the rest of the backend — no type duplication.

4. **Plugin hooks** — `tool.execute.before` and `tool.execute.after` hooks let us capture reasoning traces, guard against dangerous commands, and log tool usage without modifying OpenCode itself.

5. **Session management** — OpenCode maintains conversation state within the server process. Multi-turn follow-up questions work naturally within a container's lifetime.

6. **System tool access** — the agent can `git clone` repos, run `rg` to search code, use `tokei` for language stats, manage Python environments with `uv`, and execute arbitrary analysis scripts — all sandboxed in the container.

7. **S3 artifact integration** — generated files (CSVs, reports, charts, analysis outputs) are uploaded to RustFS via the `upload_artifact` MCP tool and linked to the conversation.

---

## Navigation & Page Placement

The agentic interface appears as a **top-level navigation item** in the sidebar, positioned after "Ingestion". Uses the `Sparkles` icon from Lucide.

```
┌──────────────────────┐
│ 🔷 Prism             │
│   Engineering Insights│
├──────────────────────┤
│ Platform             │
│  📊 Dashboard        │
│  👥 Teams            │
│  👤 People           │
│  📈 Ingestion        │
│  ✨ Ask              │  ← NEW
├──────────────────────┤
│ 👤 Jon Seager    ▾   │
└──────────────────────┘
```

**Route:** `/ask` (new session), `/ask/:conversationId` (resume)

---

## UI Mockups

### Empty State (`/ask`)

```
┌─ PageHeader ──────────────────────────────────────────────────┐
│ ☰ │ Ask Prism                                                 │
│     Ask questions about your engineering data          [History]│
└───────────────────────────────────────────────────────────────┘
┌───────────────────────────────────────────────────────────────┐
│                                                               │
│              ✨ (size-10, muted-foreground)                   │
│                                                               │
│              Ask a question about your                        │
│              engineering data                                 │
│                                                               │
│              Prism can query metrics, search contributions,   │
│              compare teams, and analyse repository code       │
│              across all your sources.                         │
│                                                               │
│   ┌───────────────────────────────────────┐                  │
│   │  ┌─────────────────────────────────┐  │                  │
│   │  │ How has Team X's review quality │  │                  │
│   │  │ changed this quarter?           │  │                  │
│   │  └─────────────────────────────────┘  │                  │
│   │  ┌─────────────────────────────────┐  │                  │
│   │  │ How many repos have migrated    │  │                  │
│   │  │ from tox to uv?                 │  │                  │
│   │  └─────────────────────────────────┘  │                  │
│   │  ┌─────────────────────────────────┐  │                  │
│   │  │ Compare throughput between      │  │                  │
│   │  │ Team A and Team B this month    │  │                  │
│   │  └─────────────────────────────────┘  │                  │
│   │  ┌─────────────────────────────────┐  │                  │
│   │  │ Generate a review quality       │  │                  │
│   │  │ report for the Kernel team      │  │                  │
│   │  └─────────────────────────────────┘  │                  │
│   └───────────────────────────────────────┘                  │
│                                                               │
├───────────────────────────────────────────────────────────────┤
│ ┌──────────────────────────────────────────────────────┐ [▶] │
│ │ Ask a question...                                    │      │
│ └──────────────────────────────────────────────────────┘      │
└───────────────────────────────────────────────────────────────┘
```

### Active Conversation — Streaming with Repo Analysis

```
┌─ PageHeader ──────────────────────────────────────────────────┐
│ ☰ │ Ask Prism                                                 │
│     Ask questions about your engineering data          [History]│
└───────────────────────────────────────────────────────────────┘
┌───────────────────────────────────────────────────────────────┐
│                                                               │
│  ┌─ You ─────────────────────────────────────────────────┐   │
│  │ How many of our repos have migrated from tox to uv?    │   │
│  └────────────────────────────────────────────────────────┘   │
│                                                               │
│  ┌─ Prism ───────────────────────────────────────────────┐   │
│  │                                                        │   │
│  │  ┌─ Thinking ──────────────────────────────────── ▾ ┐ │   │
│  │  │  ✓ mcp: list_teams() → 8 teams, 47 repos        │ │   │
│  │  │  ✓ bash: git clone --depth 1 ubuntu/kernel-snaps │ │   │
│  │  │    → Cloned to /workspace/kernel-snaps            │ │   │
│  │  │  ✓ bash: rg -l "tox.ini" /workspace/kernel-snaps │ │   │
│  │  │    → 3 files found                                │ │   │
│  │  │  ✓ grep: "uv" in pyproject.toml files            │ │   │
│  │  │    → uv dependency in 2 files                     │ │   │
│  │  │  ✓ bash: git clone --depth 1 ubuntu/kernel-sru   │ │   │
│  │  │  ⟳ bash: rg -l "tox.ini" /workspace/kernel-sru   │ │   │
│  │  │    Running...                                     │ │   │
│  │  └──────────────────────────────────────────────────┘ │   │
│  │                                                        │   │
│  │  █ (cursor — streaming in progress)                   │   │
│  │                                                        │   │
│  └────────────────────────────────────────────────────────┘   │
│                                                               │
├───────────────────────────────────────────────────────────────┤
│ ┌──────────────────────────────────────────────────────┐ [■] │
│ │ Ask a follow-up...                                   │      │
│ └──────────────────────────────────────────────────────┘      │
└───────────────────────────────────────────────────────────────┘
```

### Completed Response with Citations and Artifacts

```
│  ┌─ Prism ───────────────────────────────────────────────┐   │
│  │                                                        │   │
│  │  ┌─ Thinking (12 steps) ────────────────────── ▸ ┐   │   │
│  │  └──────────────────────────────────────────────────┘ │   │
│  │                                                        │   │
│  │  ## Tox → UV Migration Status                          │   │
│  │                                                        │   │
│  │  | Team    | Repos | tox | uv  | both | neither |     │   │
│  │  |---------|-------|-----|-----|------|---------|      │   │
│  │  | Kernel  | 12    | 4   | 5   | 2    | 1       |     │   │
│  │  | Desktop | 8     | 6   | 1   | 0    | 1       |     │   │
│  │  | Server  | 15    | 3   | 10  | 1    | 1       |     │   │
│  │  | **All** | **47**| **18**|**21**|**4**|**4**   |     │   │
│  │                                                        │   │
│  │  **53% migrated** (21 uv-only + 4 both = 25/47)      │   │
│  │                                                        │   │
│  │  ─────────────────────────────────────────────────     │   │
│  │  📎 Artifacts                                          │   │
│  │  ┌──────────────────────────────────────────────┐     │   │
│  │  │ 📄 tox-uv-migration-report.csv    [Download] │     │   │
│  │  │ 📄 per-repo-analysis.json         [Download] │     │   │
│  │  └──────────────────────────────────────────────┘     │   │
│  │                                                        │   │
│  │  ┌───────────────────────────────────────────────┐    │   │
│  │  │ ⓘ Evidence & Reasoning                    ▸  │    │   │
│  │  └───────────────────────────────────────────────┘    │   │
│  │                                                        │   │
│  │  [💾 Save as Insight]  [📋 Copy]                      │   │
│  │                                                        │   │
│  └────────────────────────────────────────────────────────┘   │
```

### Evidence & Reasoning (expanded)

```
│  │  ┌─ Evidence & Reasoning ─────────────────────── ▾ ┐  │   │
│  │  │                                                   │  │   │
│  │  │  Model: anthropic/claude-sonnet-4-6              │  │   │
│  │  │  Tokens: 6,120 in / 2,340 out                   │  │   │
│  │  │  Duration: 24.1s (12 tool calls)                 │  │   │
│  │  │  Container: prism-agent-a7f3 (active)            │  │   │
│  │  │                                                   │  │   │
│  │  │  Step 1: mcp__prism__list_teams()                │  │   │
│  │  │    → 8 teams, 47 repos across 3 GitHub sources   │  │   │
│  │  │                                                   │  │   │
│  │  │  Step 2: bash — git clone --depth 1 ...          │  │   │
│  │  │    → kernel-snaps cloned (3.2 MB)                │  │   │
│  │  │                                                   │  │   │
│  │  │  Step 3: bash — rg -l "tox.ini" ...              │  │   │
│  │  │    → 3 files: tox.ini, tests/tox.ini, ci/tox.ini│  │   │
│  │  │                                                   │  │   │
│  │  │  ...                                              │  │   │
│  │  │                                                   │  │   │
│  │  │  Step 11: mcp__prism__upload_artifact(csv)       │  │   │
│  │  │    → tox-uv-migration-report.csv uploaded        │  │   │
│  │  │                                                   │  │   │
│  │  │  Step 12: mcp__prism__upload_artifact(json)      │  │   │
│  │  │    → per-repo-analysis.json uploaded              │  │   │
│  │  │                                                   │  │   │
│  │  └──────────────────────────────────────────────────┘ │   │
```

### Conversation History Panel (Sheet)

```
┌─ Conversation History ──────────────────── ✕ ┐
│                                               │
│  🔍 Search conversations...                   │
│                                               │
│  ┌──────────────────────────────────────────┐ │
│  │ How many repos use tox vs uv?            │ │
│  │ 12 tool calls · 2 artifacts · Mar 24     │ │
│  │ ● container active                       │ │
│  └──────────────────────────────────────────┘ │
│  ┌──────────────────────────────────────────┐ │
│  │ Team Kernel review quality this quarter  │ │
│  │ 5 tool calls · Mar 23                    │ │
│  │ ○ container reaped                       │ │
│  └──────────────────────────────────────────┘ │
│                                               │
└───────────────────────────────────────────────┘
```

---

## Container Lifecycle

### Creation

```
User sends first question (or resumes reaped session)
  │
  ▼
ps-server: ContainerManager.ensure_pod(session_id)
  │
  ├── Generate Pod spec:
  │     image: prism-agent:latest
  │     env: PRISM_API_URL, OPENCODE_MODEL, SERVICE_TOKEN,
  │           provider API keys (from K8s Secrets), SESSION_ID
  │     resources: { cpu: 1, memory: 2Gi, ephemeral: 10Gi }
  │     labels: { app: prism-agent, session: <id> }
  │
  ├── Create Pod via kube-rs
  │
  ├── Wait for ready (OpenCode server healthcheck on :4096)
  │
  └── Return Pod IP + port
```

### Communication

`ps-server` communicates with the OpenCode server inside the agent container using the [`opencode-sdk`](https://docs.rs/opencode-sdk) Rust crate — a native async client for OpenCode's HTTP + SSE API.

```rust
use opencode_sdk::{Client, ClientBuilder};

// Connect to the agent container's OpenCode server
let client = ClientBuilder::new()
    .base_url(format!("http://{}:4096", pod_ip))
    .timeout_secs(120)
    .build()?;

// Create a session
let session = client.create_session_with_title(&question).await?;

// Send a prompt and stream events in real time
client.send_text_async(&session.id, &question, None).await?;

let subscription = client.sse_subscriber()
    .subscribe_session(&session.id).await?;

// subscription yields typed Event variants:
//   Event::MessagePartUpdated — tool calls, text deltas
//   Event::SessionIdle — agent finished, collect final answer
//   Event::SessionError — error during processing
```

The crate provides:
- **40 typed event variants** matching OpenCode's server — `MessagePartUpdated`, `SessionIdle`, `SessionError`, etc.
- **SSE with automatic reconnection and backoff** — handles dropped connections transparently
- **`SessionEventRouter`** — multiplexes a single SSE stream into per-session subscriptions
- **`send_text_async` / `wait_for_idle_text`** — async prompt submission with optional blocking wait

### Idle Timeout & Reaping

The container includes an idle timer. When no prompts are received for **15 minutes**:

1. Container self-terminates (graceful shutdown)
2. `ContainerManager` detects Pod termination, marks session `container_reaped`
3. Conversation history remains in `reasoning.conversations` — ephemeral container, durable data

A **background reaper** in `ps-server` periodically deletes Pods running > **2 hours** max lifetime.

### Session Resume

When a user resumes a conversation whose container was reaped:

1. `ps-server` creates a new Pod via `ContainerManager`
2. Loads conversation history from `reasoning.conversations` + `reasoning.conversation_messages`
3. Connects to the new container's OpenCode server via `opencode-sdk`
4. Injects prior conversation as context without triggering a response:
   ```rust
   // Inject prior context — OpenCode ingests it but does not reply
   let context = format_conversation_history(&messages);
   client.send_text_async(&session.id, &context, None).await?;
   // wait for idle so context is fully ingested before sending the real question
   client.wait_for_idle_text(&session.id, Duration::from_secs(30)).await?;
   ```
5. Sends the new question as a normal prompt
6. The agent "remembers" the prior conversation. Any previously cloned repos must be re-cloned (workspace is ephemeral).

### Resource Limits & Network Policy

```yaml
resources:
  requests: { cpu: "250m", memory: "512Mi" }
  limits: { cpu: "1000m", memory: "2Gi", ephemeral-storage: "10Gi" }

# Network policy: restrict egress
egress:
  - to:
    - podSelector:
        matchLabels:
          app: ps-server          # gRPC callbacks for MCP tools
  - to:
    - podSelector:
        matchLabels:
          app: rustfs              # S3 artifact uploads
  - to:
    - ipBlock: { cidr: 0.0.0.0/0 } # GitHub/Jira/Discourse HTTPS
    ports: [{ port: 443 }]
```

---

## Agent Container — Detailed Design

### Container Image

```dockerfile
FROM ubuntu:24.04

# System tools for code analysis
RUN apt-get update && apt-get install -y --no-install-recommends \
    git curl ca-certificates xz-utils \
    && rm -rf /var/lib/apt/lists/*

# ripgrep
RUN curl -sL https://github.com/BurntSushi/ripgrep/releases/latest/download/ripgrep-*-x86_64-unknown-linux-musl.tar.gz \
    | tar xz --strip-components=1 -C /usr/local/bin --wildcards '*/rg'

# tokei (code statistics)
RUN curl -sL https://github.com/XAMPPRocky/tokei/releases/latest/download/tokei-x86_64-unknown-linux-gnu.tar.gz \
    | tar xz -C /usr/local/bin

# uv (Python version + environment manager)
RUN curl -LsSf https://astral.sh/uv/install.sh | sh
ENV PATH="/root/.local/bin:$PATH"

# OpenCode
RUN curl -fsSL https://opencode.ai/install | bash

# ps-agent-mcp binary (Rust, built in CI)
COPY ps-agent-mcp /usr/local/bin/ps-agent-mcp

# OpenCode configuration + agent prompt
COPY opencode.json /app/opencode.json
COPY .opencode/ /app/.opencode/

# Workspace for cloned repos and generated files
RUN mkdir /workspace
WORKDIR /workspace

EXPOSE 4096

# Start OpenCode server (reads config from /app/opencode.json)
CMD ["opencode", "--server", "--port", "4096", "--config", "/app/opencode.json"]
```

The MCP server (`ps-agent-mcp`) is a static Rust binary — no additional runtimes needed beyond OpenCode itself. The final image will be slimmed with Chisel per project convention.

### Model Selection — Wired to Prism AI Settings

The agent container's model is **not hardcoded**. It is driven by the existing `AiSettings.agentic` task config (provider + model) managed in the Admin UI → AI Settings tab. When `ps-server` creates a Pod, it reads the current agentic config and passes it as environment variables:

| Env Var | Source | Example |
|---------|--------|---------|
| `OPENCODE_MODEL` | `ai.tasks.agentic.provider/model` | `anthropic/claude-sonnet-4-6` |
| `OPENCODE_SMALL_MODEL` | `ai.tasks.enrichment.provider/model` | `anthropic/claude-haiku-4-5` |
| `ANTHROPIC_API_KEY` | K8s Secret (sourced from `config.secrets` via admin setup) | (if Anthropic provider configured) |
| `OPENROUTER_API_KEY` | K8s Secret (sourced from `config.secrets`) | (if OpenRouter provider configured) |
| `GOOGLE_GENERATIVE_AI_API_KEY` | K8s Secret (sourced from `config.secrets`) | (if Google provider configured) |

The `opencode.json` uses `{env:OPENCODE_MODEL}` variable substitution so the model is resolved at container startup:

### OpenCode Configuration (`opencode.json`)

```json
{
  "$schema": "https://opencode.ai/config.json",
  "model": "{env:OPENCODE_MODEL}",
  "small_model": "{env:OPENCODE_SMALL_MODEL}",
  "server": {
    "port": 4096,
    "hostname": "0.0.0.0"
  },
  "mcp": {
    "prism": {
      "type": "local",
      "command": ["/usr/local/bin/ps-agent-mcp"],
      "environment": {
        "PRISM_API_URL": "{env:PRISM_API_URL}",
        "SERVICE_TOKEN": "{env:SERVICE_TOKEN}"
      }
    }
  },
  "permission": {
    "edit": "allow",
    "write": "allow",
    "bash": "allow",
    "read": "allow",
    "glob": "allow",
    "grep": "allow",
    "webfetch": "allow",
    "patch": "allow",
    "mcp_prism_*": "allow"
  },
  "agent": {
    "prism": {
      "mode": "primary",
      "description": "Engineering insights assistant with access to Prism data and repository analysis tools",
      "temperature": 0.3,
      "max_steps": 20
    }
  }
}
```

This means changing the agentic model in the Admin UI (e.g. switching from Claude Sonnet to Gemini Flash via OpenRouter) takes effect on the **next container creation** — existing containers keep their configured model until reaped.

The `ContainerManager.ensure_pod()` method reads the current AI settings from `TaskRouter` and injects them into the Pod spec's env vars:

```rust
let ai_config = self.router.read().await.config();
let agentic = ai_config.tasks.agentic;
let model_id = format!("{}/{}", agentic.provider.as_str(), agentic.model);

pod_spec.env("OPENCODE_MODEL", &model_id);
pod_spec.env("OPENCODE_SMALL_MODEL", &small_model_id);
// Provider API keys from K8s Secrets (mounted, not inline)
```

### Agent System Prompt (`.opencode/agents/prism.md`)

```markdown
---
description: Engineering insights assistant with Prism data tools and repo analysis
mode: primary
temperature: 0.3
max_steps: 20
---

You are Prism, an engineering insights assistant deployed at Canonical.
You help users understand their engineering data across GitHub, Jira, Discourse, and other platforms.

## Available tools

### Prism data tools (MCP — prefixed `mcp_prism_`)
Use these to query pre-computed metrics, enrichments, and team/people data:
- `mcp_prism_query_team_metrics` — DORA, flow, review metrics for a team + period
- `mcp_prism_query_contributions` — search/filter contributions with flexible criteria
- `mcp_prism_compare_teams` — side-by-side metrics for 2+ teams
- `mcp_prism_get_person_profile` — individual activity summary across platforms
- `mcp_prism_search_similar` — find semantically similar contributions (embeddings)
- `mcp_prism_search_by_text` — semantic search over all contributions
- `mcp_prism_query_enrichments` — get AI enrichment scores for a contribution
- `mcp_prism_list_teams` — browse team hierarchy (resolves names → IDs)
- `mcp_prism_list_people` — browse people, optionally filtered by team
- `mcp_prism_upload_artifact` — upload a generated file to S3 as a conversation artifact
- `mcp_prism_list_artifacts` — list artifacts for the current conversation

### System tools
Use these for repository analysis and code inspection:
- `bash` — run commands: git clone, rg, grep, tokei, uv, python, etc.
- `read` — read files from cloned repos or generated outputs
- `write` — create analysis scripts, reports, output files
- `glob` — find files by pattern in /workspace
- `grep` — search file contents with regex
- `webfetch` — fetch web pages for reference

### Python (via uv)
When you need to run Python scripts (data analysis, chart generation, etc.):
1. Always use `uv` to manage Python versions and virtual environments
2. `uv run python script.py` for one-off scripts
3. `uv init && uv add pandas matplotlib` for projects with dependencies
4. Never use `pip install` directly — always go through `uv`

## Guidelines

1. ALWAYS use Prism MCP tools for metrics and data queries. Never guess numbers.
2. Use system tools when the question requires inspecting actual code.
3. For repo analysis: clone repos to /workspace/<repo-name> with `--depth 1` (shallow clone).
4. Cite every claim with footnote references [¹][²] linking to internal paths:
   - People: [Name](/people/{person_id})
   - Teams: [Team](/teams/{team_id})
   - Contributions: [Title](/contributions/{contribution_id})
5. Format answers in Markdown with tables for comparisons.
6. When generating reports or analysis outputs, use `mcp_prism_upload_artifact` to make them downloadable.
7. When context is injected from a prior session, acknowledge it and re-clone repos if needed.
8. If you cannot answer with available tools, say so clearly. Do not hallucinate.

## Current context
- Current date: {current_date}
- Workspace: /workspace (ephemeral, empty at session start)
```

### Prism MCP Server (`crates/ps-agent-mcp/`)

A Rust crate that implements an MCP stdio server. OpenCode spawns the compiled binary as a subprocess and communicates via JSON-RPC 2.0 over stdin/stdout. The crate depends on `ps-proto` for gRPC client types and `object_store` for S3 artifact access.

For MCP protocol handling, the crate uses **[`rmcp`](https://crates.io/crates/rmcp)** (Rust MCP SDK) — the official Rust implementation of the Model Context Protocol maintained by the MCP project. `rmcp` provides the stdio transport, JSON-RPC framing, tool schema generation via `#[tool]` proc macros, and server lifecycle management. This eliminates hand-rolled transport code and gives us protocol-compliant behaviour for free.

```rust
// Cargo.toml
[dependencies]
rmcp = { version = "1.2", features = ["server", "transport-io"] }
```

```rust
// Example tool registration with rmcp
use rmcp::{ServerHandler, tool, Tool};

#[derive(Clone)]
struct PrismTools {
    client: PrismClient,
    artifacts: ArtifactStore,
}

#[tool(tool_box)]
impl PrismTools {
    #[tool(description = "Get DORA, flow, and review metrics for a team over a period")]
    async fn query_team_metrics(
        &self,
        #[tool(param, description = "Team name")] team_name: String,
        #[tool(param, description = "Start date (YYYY-MM-DD)")] period_start: String,
        #[tool(param, description = "End date (YYYY-MM-DD)")] period_end: String,
    ) -> Result<String, ToolError> {
        // Calls ps-server gRPC via self.client
    }

    #[tool(description = "Upload a generated file as a conversation artifact")]
    async fn upload_artifact(
        &self,
        #[tool(param, description = "Path to file in /workspace")] file_path: String,
        #[tool(param, description = "Display name")] display_name: Option<String>,
    ) -> Result<String, ToolError> {
        // Uploads to S3 via self.artifacts
    }

    // ... all 11 tools
}
```

```
crates/ps-agent-mcp/
├── Cargo.toml
└── src/
    ├── main.rs              # Entry point: env config, rmcp server setup
    ├── prism_client.rs      # tonic gRPC client wrapping ps-proto types
    ├── artifact_store.rs    # S3 upload/download via object_store crate
    └── tools.rs             # All 11 tool implementations via #[tool] macros
```

Note: with `rmcp`'s proc macros, all 11 tools can live in a single `tools.rs` file as methods on `PrismTools`. No need for separate files per tool — the macro generates the JSON schema, dispatch, and parameter parsing.

#### Artifact Tools (S3 Integration)

**`upload_artifact`** — Reads a file from the container's `/workspace`, uploads to RustFS under `ps-artifacts/conversations/{conversation_id}/{filename}`, and returns a pre-signed download URL. Implemented as a `#[tool]` method on `PrismTools` using the `object_store` crate (same crate used by `ArtifactStore` in ps-core).

**`list_artifacts`** — Lists artifacts for the current conversation by listing objects under the `conversations/{session_id}/` prefix.

#### Prism gRPC Client (`prism_client.rs`)

The MCP tools call ps-server's existing gRPC APIs via tonic, using the proto-generated client types from `ps-proto`. The container authenticates using a **read-only service account token** (stored as K8s Secret, passed as `SERVICE_TOKEN` env var).

```rust
// prism_client.rs
use ps_proto::canonical::prism::v1::*;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;

pub struct PrismClient {
    metrics: MetricsServiceClient<Channel>,
    org: OrgServiceClient<Channel>,
    reasoning: ReasoningServiceClient<Channel>,
    insights: InsightsServiceClient<Channel>,
}

impl PrismClient {
    pub async fn connect(url: &str, token: &str) -> Result<Self> {
        let channel = Channel::from_shared(url.to_string())?.connect().await?;
        let token: MetadataValue<_> = format!("Bearer {token}").parse()?;

        // Each client clone shares the channel, auth token added per-request
        // via interceptor
        Ok(Self {
            metrics: MetricsServiceClient::with_interceptor(channel.clone(), auth(token.clone())),
            org: OrgServiceClient::with_interceptor(channel.clone(), auth(token.clone())),
            reasoning: ReasoningServiceClient::with_interceptor(channel.clone(), auth(token.clone())),
            insights: InsightsServiceClient::with_interceptor(channel, auth(token)),
        })
    }
}
```

#### Tool Details

Each MCP tool wraps calls to ps-server's existing gRPC API. Tools do **not** access the database directly.

| Tool | Input | Backing RPCs | Output |
|------|-------|-------------|--------|
| `query_team_metrics` | team_name, period_start, period_end | `InsightsService.GetTeamInsights` + `MetricsService.GetTeamMetrics` | DORA + flow + review quality metrics |
| `query_contributions` | team_name?, person_name?, platform?, type?, state?, date range, limit | `MetricsService.ListContributions` (resolves names first) | Contribution list with titles, authors, states, URLs |
| `compare_teams` | team_names[], period, metrics[] | `InsightsService.GetTeamInsights` × N | Side-by-side comparison table |
| `get_person_profile` | person_name, period? | `OrgService.GetPerson` + `InsightsService.GetPersonInsights` | Activity summary, review profile, PR impact |
| `search_similar` | contribution_id, limit?, platform? | `ReasoningService.FindSimilar` | Ranked similar contributions |
| `search_by_text` | query, limit?, platform? | `ReasoningService.SearchByText` | Semantically matching contributions |
| `query_enrichments` | contribution_id | `ReasoningService.GetEnrichments` | Enrichment scores, rationale, confidence |
| `list_teams` | search? | `OrgService.ListTeams` | Team names, IDs, member counts, hierarchy |
| `list_people` | team_name?, search? | `OrgService.ListPeople` | Names, IDs, team memberships |
| `upload_artifact` | file_path, display_name? | Direct S3 upload | Artifact key + presigned download URL |
| `list_artifacts` | — | S3 list objects | Conversation's uploaded artifacts |

---

## Implementation Steps

### Step 1: Database — Conversations, Messages, Artifacts

**Migration: `XXXX_create_conversations.sql`**

```sql
CREATE TABLE reasoning.conversations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES auth.users(id),
    title TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    model_name TEXT NOT NULL DEFAULT 'anthropic/claude-sonnet-4-6',
    -- Container lifecycle
    container_pod_name TEXT,
    container_status TEXT NOT NULL DEFAULT 'pending',
    opencode_session_id TEXT,
    -- Totals (updated after each turn)
    total_tool_calls INTEGER NOT NULL DEFAULT 0,
    total_prompt_tokens INTEGER NOT NULL DEFAULT 0,
    total_completion_tokens INTEGER NOT NULL DEFAULT 0,
    total_estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_activity_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conversations_user ON reasoning.conversations(user_id, created_at DESC);
CREATE INDEX idx_conversations_container ON reasoning.conversations(container_pod_name)
    WHERE container_status = 'active';

-- Individual turns within a conversation
CREATE TABLE reasoning.conversation_messages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES reasoning.conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL,          -- 'user' | 'assistant'
    content TEXT NOT NULL,
    reasoning_trace JSONB,      -- tool calls for assistant messages
    supporting_data JSONB,      -- citations for assistant messages
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conv_messages ON reasoning.conversation_messages(conversation_id, created_at);

-- Artifacts generated during conversations (stored in S3)
CREATE TABLE reasoning.conversation_artifacts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES reasoning.conversations(id) ON DELETE CASCADE,
    message_id UUID REFERENCES reasoning.conversation_messages(id),
    artifact_key TEXT NOT NULL,       -- S3 key: conversations/{conv_id}/{filename}
    display_name TEXT NOT NULL,       -- Human-readable filename
    content_type TEXT,                -- MIME type (text/csv, application/json, etc.)
    size_bytes BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conv_artifacts ON reasoning.conversation_artifacts(conversation_id);

-- Link insights back to conversations
ALTER TABLE reasoning.insights
    ADD COLUMN conversation_id UUID REFERENCES reasoning.conversations(id);
```

**Reasoning trace JSONB schema (per-message):**

```json
{
  "steps": [
    {
      "index": 0,
      "tool_name": "mcp_prism_list_teams",
      "arguments": { "search": "Kernel" },
      "result_summary": "Found 'Kernel' (team_abc), 12 repos",
      "duration_ms": 45,
      "timestamp": "2026-03-24T14:30:01Z"
    },
    {
      "index": 3,
      "tool_name": "bash",
      "arguments": { "command": "git clone --depth 1 https://github.com/ubuntu/kernel-snaps /workspace/kernel-snaps" },
      "result_summary": "Cloned successfully (3.2 MB)",
      "duration_ms": 2100,
      "timestamp": "2026-03-24T14:30:05Z"
    }
  ],
  "total_duration_ms": 8300,
  "tool_call_count": 8
}
```

**Files created/changed:**

| File | Action |
|------|--------|
| `migrations/XXXX_create_conversations.sql` | **Create** |
| `crates/ps-core/src/repo/reasoning.rs` | **Modify** — add conversation CRUD, message CRUD, artifact CRUD, container status methods |

**Testing:**

| Level | Test |
|-------|------|
| Integration (repo) | `define_repo_test!` — conversation lifecycle: create, add messages, update totals, list by user |
| Integration (repo) | Multi-turn: 4 alternating user/assistant messages, retrieve in order |
| Integration (repo) | Container status transitions: pending → active → reaped → active (resume) |
| Integration (repo) | Artifacts: create artifact, list by conversation, verify S3 key format |
| Integration (repo) | `insights.conversation_id` FK round-trip |

---

### Step 2: Proto — Streaming RPC + Conversation Management

Add streaming `AskQuestion` RPC and conversation management RPCs to `reasoning.proto`.

**New RPCs added to `ReasoningService`:**

```protobuf
rpc AskQuestion(AskQuestionRequest) returns (stream AgentEvent);
rpc ListConversations(ListConversationsRequest) returns (ListConversationsResponse);
rpc GetConversation(GetConversationRequest) returns (GetConversationResponse);
rpc SaveInsightFromConversation(SaveInsightFromConversationRequest)
    returns (SaveInsightFromConversationResponse);
rpc GetArtifactDownloadUrl(GetArtifactDownloadUrlRequest)
    returns (GetArtifactDownloadUrlResponse);
```

**Key message types:**

```protobuf
message AskQuestionRequest {
  string question = 1;
  optional string conversation_id = 2;
}

message AgentEvent {
  oneof event {
    AgentToolCallStarted tool_call_started = 1;
    AgentToolCallCompleted tool_call_completed = 2;
    AgentPartialAnswer partial_answer = 3;
    AgentFinalAnswer final_answer = 4;
    AgentError error = 5;
    AgentThinking thinking = 6;
    AgentContainerStatus container_status = 7;
    AgentArtifactUploaded artifact_uploaded = 8;
  }
}

message AgentArtifactUploaded {
  string artifact_id = 1;
  string display_name = 2;
  string content_type = 3;
  int64 size_bytes = 4;
  string download_url = 5;
}

message AgentFinalAnswer {
  string answer = 1;
  string conversation_id = 2;
  string supporting_data_json = 3;
  int32 prompt_tokens = 4;
  int32 completion_tokens = 5;
  double estimated_cost_usd = 6;
  int32 tool_call_count = 7;
  int32 duration_ms = 8;
  repeated ArtifactInfo artifacts = 9;
}

message ArtifactInfo {
  string id = 1;
  string display_name = 2;
  string content_type = 3;
  int64 size_bytes = 4;
}

message GetArtifactDownloadUrlRequest {
  string artifact_id = 1;
}

message GetArtifactDownloadUrlResponse {
  string download_url = 1;
  int32 expires_in_seconds = 2;
}

// ConversationSummary, GetConversationResponse, etc. include artifact counts
// and container_status fields (see full proto in implementation)
```

**Files created/changed:**

| File | Action |
|------|--------|
| `proto/canonical/prism/v1/reasoning.proto` | **Modify** — add RPCs and messages |
| Auto-generated files (buf generate) | `reasoning_connect.ts`, `reasoning_pb.ts`, `canonical.prism.v1.rs` |

**Testing:**

| Level | Test |
|-------|------|
| Lint | `buf lint` passes |
| Build | `buf generate` produces valid Rust + TypeScript |

---

### Step 3: Container Manager — K8s Pod Lifecycle

New module in `ps-server` that manages agent container Pods using `kube-rs`.

**Operations:**

1. **`ensure_pod(session_id)`** — Find active Pod or create new one. Returns Pod IP + port when ready.
2. **`get_pod_status(session_id)`** — Check if Pod is running, pending, or reaped.
3. **`update_activity(session_id)`** — Update `last-activity` annotation on the Pod.
4. **`reap_idle_pods()`** — Background task (every 60s). Delete Pods idle > 15 min or running > 2 hours.
5. **`delete_pod(session_id)`** — Force-delete a Pod.

**OpenCode client** — Uses the `opencode-sdk` Rust crate ([docs.rs/opencode-sdk](https://docs.rs/opencode-sdk)), which provides a native async client for OpenCode's HTTP + SSE API:

```rust
use opencode_sdk::{Client, ClientBuilder};

// Create client pointing at the agent container
let client = ClientBuilder::new()
    .base_url(format!("http://{}:4096", pod_ip))
    .timeout_secs(120)
    .build()?;

// Create session
let session = client.create_session_with_title("User question...").await?;

// Inject prior conversation context (for resumed sessions)
client.send_text_async(&session.id, context, None).await?;

// Send the actual question and wait for completion
let answer = client.send_text_async_and_wait_for_idle(
    &session.id, &question, None, Duration::from_secs(120)
).await?;

// Or stream events in real time:
let subscription = client.sse_subscriber()
    .subscribe_session(&session.id).await?;
// subscription yields typed Event variants:
//   Event::MessagePartUpdated { properties: MessagePartEventProps }
//   Event::SessionIdle { properties: SessionIdleProps }
//   Event::SessionError { properties: SessionErrorProps }
```

The SDK provides:
- **40 typed event variants** — `MessagePartUpdated` (tool calls + text), `SessionIdle`, `SessionError`, etc.
- **SSE with reconnection/backoff** — handles dropped connections transparently
- **`SessionEventRouter`** — multiplexes a single SSE stream into per-session subscriptions (useful when multiple conversations share a container in future)
- **`send_text_async` with `noReply` equivalent** — inject context without triggering a response

**Files created/changed:**

| File | Action |
|------|--------|
| `crates/ps-server/src/container_manager/mod.rs` | **Create** — `ContainerManager`, Pod CRUD, idle reaper |
| `crates/ps-server/src/container_manager/pod_spec.rs` | **Create** — Pod spec builder (env vars from AI settings, resource limits, labels) |
| `crates/ps-server/src/container_manager/event_mapper.rs` | **Create** — Map `opencode_sdk::types::event::Event` variants to `AgentEvent` proto messages |
| `crates/ps-server/Cargo.toml` | **Modify** — add `kube`, `k8s-openapi`, `tokio-stream`, `opencode-sdk` |

Note: The `opencode-sdk` crate replaces what would have been a hand-rolled HTTP client. It provides typed session management, SSE streaming, and event routing out of the box.

**Testing:**

| Level | Test |
|-------|------|
| Unit | Pod spec builder — verify labels, env vars (including model from AI settings), resource limits |
| Unit | Event mapper — verify mapping from `opencode_sdk::Event::MessagePartUpdated` → `AgentEvent::tool_call_started`/`partial_answer`, `Event::SessionIdle` → `AgentEvent::final_answer`, `Event::SessionError` → `AgentEvent::error` |
| Integration | Pod lifecycle — create → ready → activity update → idle reap (requires K8s; skip in CI without) |

---

### Step 4: `ps-agent-mcp` Crate + Container Image

A new Rust crate that implements an MCP stdio server. OpenCode spawns it as a subprocess and communicates via JSON-RPC over stdin/stdout. The binary uses `ps-proto` types to call ps-server's gRPC API and `object_store` for S3 artifact management.

**Design rationale** (see [Appendix: Decision Log](#appendix-decision-log), decision #2):
- Shares `ps-proto` types — tool inputs/outputs use the same proto-generated types as the rest of the backend
- Static binary — no runtime needed in the container, smaller image
- Single toolchain — `cargo build` produces `ps-server`, `ps-agent-mcp`, and all other binaries
- Tool input schemas derived from Rust structs via `schemars::JsonSchema`, same pattern as Rig extractors in W1

**Crate structure:**

```
crates/ps-agent-mcp/
├── Cargo.toml
└── src/
    ├── main.rs              # Entry point: env config, rmcp server setup
    ├── prism_client.rs      # tonic gRPC client wrapping ps-proto types
    ├── artifact_store.rs    # S3 upload/download via object_store crate
    └── tools.rs             # All 11 tool implementations as #[tool] methods
```

With `rmcp`'s proc macros, all 11 tools live as methods on a single `PrismTools` struct. The macro generates JSON schemas, parameter parsing, and dispatch — no per-tool files or manual registry needed.

**Dependency flow:** `ps-agent-mcp → ps-proto` (for gRPC client types). Does **not** depend on `ps-core` (no direct DB access — all data goes through ps-server gRPC).

**Cargo.toml:**

```toml
[package]
name = "ps-agent-mcp"
version.workspace = true
edition.workspace = true

[dependencies]
ps-proto = { path = "../ps-proto" }
rmcp = { version = "1.2", features = ["server", "transport-io"] }
tonic.workspace = true
tokio.workspace = true
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
object_store = { version = "0.11", features = ["aws"] }
tracing.workspace = true
thiserror.workspace = true
```

**Entry point (`main.rs`):**

```rust
use rmcp::transport::io::stdio;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::init();

    let prism_url = std::env::var("PRISM_API_URL")?;
    let token = std::env::var("SERVICE_TOKEN")?;
    let session_id = std::env::var("SESSION_ID")?;
    let s3_endpoint = std::env::var("S3_ENDPOINT")?;

    let client = PrismClient::connect(&prism_url, &token).await?;
    let artifacts = ArtifactStore::new(&s3_endpoint, &session_id);
    let tools = PrismTools::new(client, artifacts);

    // rmcp handles MCP protocol negotiation, JSON-RPC framing, and dispatch
    let server = tools.serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
```

**Files created:**

| File | Action |
|------|--------|
| `crates/ps-agent-mcp/Cargo.toml` | **Create** |
| `crates/ps-agent-mcp/src/main.rs` | **Create** — entry point, rmcp server setup |
| `crates/ps-agent-mcp/src/prism_client.rs` | **Create** — tonic gRPC client wrapping ps-proto |
| `crates/ps-agent-mcp/src/artifact_store.rs` | **Create** — S3 upload/download via `object_store` |
| `crates/ps-agent-mcp/src/tools.rs` | **Create** — `PrismTools` struct with 11 `#[tool]` methods |
| `agent-container/Dockerfile` | **Create** — Ubuntu 24.04 + git + rg + tokei + uv + OpenCode + `ps-agent-mcp` binary |
| `agent-container/opencode.json` | **Create** — OpenCode config with `ps-agent-mcp` as local MCP server |
| `agent-container/.opencode/agents/prism.md` | **Create** — Agent system prompt |

**Testing:**

| Level | Test |
|-------|------|
| Unit | Each tool method — mock `PrismClient`, verify correct RPC called with correct args, verify JSON output |
| Unit | `upload_artifact` — mock `ArtifactStore`, verify S3 key format `conversations/{id}/{filename}` |
| Unit | `list_artifacts` — mock S3 list, verify response |
| Integration | Build binary, spawn as subprocess, send MCP `initialize` + `tools/list`, verify 11 tools registered with correct schemas |
| Integration | Send MCP `tools/call` for `query_team_metrics`, verify JSON-RPC response |
| Integration | Docker build — verify image builds, OpenCode starts, health endpoint responds |

---

### Step 5: gRPC Service — AskQuestion Streaming Handler

Wire container manager + OpenCode client into `ReasoningService`.

**Flow:**

```
1. Validate question (non-empty, <4000 chars), rate limit (10/min/user)
2. Create/resume conversation in DB
3. ContainerManager.ensure_pod() → Pod IP
4. Stream AgentContainerStatus events ("creating" / "ready")
5. opencode_sdk::Client → create_session_with_title() or resume
6. If resuming: inject prior conversation via send_text_async()
7. client.send_text_async(question)
8. client.sse_subscriber().subscribe_session() → SSE event stream
9. For each opencode_sdk::Event:
   a. event_mapper → AgentEvent proto
   b. Capture in reasoning trace (JSONB)
   c. Send on gRPC stream
   d. If artifact MCP tool completed: record in DB, send AgentArtifactUploaded
10. On Event::SessionIdle: store message + trace in DB, update conversation totals
11. Log cost to reasoning.api_usage via CostTracker
```

**Files created/changed:**

| File | Action |
|------|--------|
| `crates/ps-server/src/services/reasoning.rs` | **Modify** — add `ask_question()`, `list_conversations()`, `get_conversation()`, `save_insight_from_conversation()`, `get_artifact_download_url()` |

**Testing:**

| Level | Test |
|-------|------|
| Integration (API) | `define_api_test!` — mock container manager + OpenCode client, verify event stream sequence |
| Integration (API) | `ListConversations` — create 3 conversations, verify ordering, artifact counts |
| Integration (API) | `GetConversation` — verify messages + trace + artifacts returned |
| Integration (API) | `SaveInsightFromConversation` — verify insight with `conversation_id` FK |
| Integration (API) | `GetArtifactDownloadUrl` — verify presigned URL generation |
| Integration (API) | Auth + validation — unauthenticated rejection, empty question, too-long question |
| Integration (API) | Resume — create conversation with messages, resume, verify context injection |

---

### Step 6: Frontend — Ask Page & Streaming UI

#### 6a: Route & Navigation

| File | Action |
|------|--------|
| `frontend/app.tsx` | **Modify** — add `AskPage` lazy import, routes `/ask` and `/ask/:conversationId` |
| `frontend/components/app-sidebar.tsx` | **Modify** — add `Sparkles` import, `{ title: "Ask", href: "/ask", icon: Sparkles }` |

#### 6b: Hooks

| File | Action |
|------|--------|
| `frontend/views/ask/hooks/use-ask-question.ts` | **Create** — streaming hook, consumes server-streaming RPC, manages `AgentState` |
| `frontend/views/ask/hooks/use-conversations.ts` | **Create** — React Query hooks for list/get/save conversations |
| `frontend/views/ask/hooks/use-artifacts.ts` | **Create** — hook for `GetArtifactDownloadUrl`, artifact download |

**`AgentState` type:**

```typescript
type AgentState =
  | { status: "idle" }
  | { status: "container_starting"; message: string }
  | { status: "streaming"; steps: ToolCallStep[]; partialAnswer: string;
      thinkingText: string; artifacts: ArtifactInfo[] }
  | { status: "completed"; steps: ToolCallStep[]; answer: string;
      conversationId: string; supportingData: Citation[];
      tokenUsage: TokenUsage; durationMs: number; artifacts: ArtifactInfo[] }
  | { status: "error"; message: string; retryable: boolean };
```

#### 6c: Components

```
frontend/views/ask/
├── pages/
│   └── ask-page.tsx
├── components/
│   ├── query-input.tsx           # Textarea + send/cancel button
│   ├── conversation-thread.tsx   # Scrollable message list
│   ├── user-message.tsx          # User question
│   ├── agent-response.tsx        # Thinking + answer + citations + artifacts + actions
│   ├── thinking-steps.tsx        # Collapsible tool-call progress feed
│   ├── thinking-step.tsx         # Single step (MCP/bash/read/grep — distinct icons)
│   ├── answer-content.tsx        # Markdown renderer + citation links
│   ├── evidence-panel.tsx        # Expandable reasoning trace
│   ├── artifact-list.tsx         # Download links for generated files
│   ├── container-status.tsx      # "Starting agent container..." badge
│   ├── suggested-questions.tsx   # Empty state suggestions
│   ├── conversation-history.tsx  # Sheet with conversation list
│   └── save-insight-dialog.tsx   # Save answer as insight
└── hooks/
    ├── use-ask-question.ts
    ├── use-conversations.ts
    └── use-artifacts.ts
```

**Component highlights:**

**`thinking-step.tsx`** — Context-appropriate icons:
- MCP tools (`mcp_prism_*`): `Database` icon
- Bash: `Terminal` icon, shows command in monospace
- Read/Glob/Grep: `FileText`/`FolderSearch`/`Search` icons
- Upload artifact: `Upload` icon
- Running: `Loader2 animate-spin`, Completed: `Check` (green)

**`artifact-list.tsx`** — Shows uploaded artifacts with download buttons:
```
📎 Artifacts
┌────────────────────────────────────────┐
│ 📄 tox-uv-migration-report.csv  [↓]  │
│ 📄 per-repo-analysis.json       [↓]  │
└────────────────────────────────────────┘
```
Download button calls `GetArtifactDownloadUrl` → opens presigned URL.

**`answer-content.tsx`** — Renders via `react-markdown` + `remark-gfm`. Internal paths become `<Link>`. Code blocks syntax-highlighted.

**Dependencies to add:**

| Package | Purpose |
|---------|---------|
| `react-markdown` | Render agent Markdown answers |
| `remark-gfm` | GitHub-flavoured Markdown (tables) |

**Files created:**

| File | Action |
|------|--------|
| All files listed in component tree above | **Create** |

**Testing:**

| Level | Test |
|-------|------|
| UI (vitest) | `use-ask-question` — mock stream, verify state transitions: idle → container_starting → streaming → completed |
| UI (vitest) | `use-ask-question` — verify artifact events accumulate in state |
| UI (vitest) | `use-ask-question` — cancel mid-stream, verify abort and reset |
| UI (vitest) | `ThinkingStep` — render MCP (database icon), Bash (terminal icon), artifact upload (upload icon) |
| UI (vitest) | `ContainerStatus` — render creating/ready states |
| UI (vitest) | `QueryInput` — send/stop button toggle based on streaming state |
| UI (vitest) | `ArtifactList` — render 2 artifacts with download buttons |
| UI (vitest) | `ConversationHistory` — active vs reaped status dots, artifact counts |
| UI (vitest) | `SaveInsightDialog` — fill title, submit, verify mutation |
| UI (vitest) | `EvidencePanel` — render trace with MCP + Bash steps |
| UI (vitest) | `AnswerContent` — Markdown with table, code, citations |

---

### Step 7: psctl — `psctl ask` Command

```
$ psctl ask "How many repos have migrated from tox to uv?"

⏳ Starting agent container...
✅ Agent ready

🔧 mcp: list_teams() → 8 teams, 47 repos
🔧 bash: git clone --depth 1 ubuntu/kernel-snaps
🔧 bash: rg -l "tox.ini" /workspace/kernel-snaps → 3 files
...
📎 tox-uv-migration-report.csv uploaded

## Tox → UV Migration Status
...

---
Model: anthropic/claude-sonnet-4-6 | 12 tool calls | 2 artifacts | 24.1s
```

`--json` flag outputs structured JSON. Artifact download URLs included in output.

**Files created/changed:**

| File | Action |
|------|--------|
| `crates/psctl/src/commands/ask.rs` | **Create** |
| `crates/psctl/src/commands/mod.rs` | **Modify** — add `pub mod ask` |
| `crates/psctl/src/main.rs` | **Modify** — add `ask` to CLI enum |

**Testing:**

| Level | Test |
|-------|------|
| Unit | Terminal formatting — tool icons, artifact display, `--json` valid JSON |
| Integration | Call `psctl ask` with mock server, verify streamed output |

---

### Step 8: K8s Deployment — Network Policy, Service Account, Tiltfile

**Service account:** Create a read-only API token for agent containers during setup. Stored as K8s Secret, injected as `SERVICE_TOKEN` env var.

**Tiltfile strategy:** Agent containers are created on-demand by `ContainerManager`, not deployed as a standing K8s workload. However, Tilt needs to know about the image so it builds and pushes it to the local registry. We add a **dummy K8s Job** that references the image and runs `echo Done` — this ensures the image is built during `tilt up` but doesn't keep a container running:

```yaml
# k8s/agent-image-builder.yaml
apiVersion: batch/v1
kind: Job
metadata:
  name: prism-agent-image-builder
  labels:
    app: prism-agent-builder
spec:
  ttlSecondsAfterFinished: 60
  template:
    spec:
      containers:
        - name: agent
          image: prism-agent:latest
          command: ["echo", "Done"]
      restartPolicy: Never
```

```python
# Tiltfile addition
docker_build('prism-agent', './agent-container',
    build_args={'PS_AGENT_MCP_BIN': '../target/release/ps-agent-mcp'})
k8s_yaml('k8s/agent-image-builder.yaml')
k8s_yaml('k8s/agent-network-policy.yaml')
k8s_yaml('k8s/agent-service-account-secret.yaml')

# Group the agent resources together
k8s_resource('prism-agent-image-builder', labels=['agent'])
```

**Files created/changed:**

| File | Action |
|------|--------|
| `k8s/agent-image-builder.yaml` | **Create** — dummy Job to trigger image build in Tilt |
| `k8s/agent-network-policy.yaml` | **Create** — egress to ps-server (gRPC), RustFS (S3), and HTTPS (443) only |
| `k8s/agent-service-account-secret.yaml` | **Create** (template) |
| `Tiltfile` | **Modify** — add `docker_build` for agent-container, add K8s resources |

**Testing:**

| Level | Test |
|-------|------|
| Integration | `tilt up` builds agent image and pushes to local registry |
| Integration | Verify agent Pod can reach ps-server, RustFS, GitHub HTTPS. Verify blocked egress. |

---

### Step 9: Backup/Restore Extension

**Files changed:**

| File | Action |
|------|--------|
| `crates/ps-core/src/backup.rs` | **Modify** — add conversations, messages, artifacts to export/import |

**Note:** Artifact *files* in S3 are NOT included in the backup bundle (too large). Only the metadata rows are backed up. The `artifact_key` references remain; if the S3 data is lost, the metadata indicates what was generated but downloads will fail.

**Testing:**

| Level | Test |
|-------|------|
| Integration | Create conversations with messages + artifacts, export, restore, verify round-trip of metadata |

---

## Streaming Protocol — End-to-End

```
User types question
  │
  ▼ gRPC server streaming (AskQuestion)
ps-server (Rust)
  │
  ├── ContainerManager.ensure_pod() → K8s Pod running OpenCode
  │
  ├── opencode-sdk Client → Pod:4096
  │     ├── client.create_session_with_title()
  │     ├── client.send_text_async() (prompt)
  │     └── client.sse_subscriber().subscribe_session() (SSE stream)
  │
  ├── For each OpenCode event:
  │     ├── tool_use → AgentToolCallStarted
  │     ├── tool_result → AgentToolCallCompleted
  │     ├── text → AgentPartialAnswer
  │     ├── result → AgentFinalAnswer
  │     └── (artifact MCP tool) → AgentArtifactUploaded
  │
  ▼ AgentEvent proto messages (gRPC HTTP/2 frames)
Envoy / Caddy proxy
  │
  ▼ Connect async iterator
Frontend (React)
  │
  ▼ useState updates → re-render
UI: thinking steps + streamed answer + artifacts
```

**Latency budget:**
- Container cold start: 5-15s (image pull cached)
- Container warm start: 1-3s
- MCP tool call: 5-50ms (gRPC to ps-server → DB → response)
- Bash tool (git clone): 1-10s
- LLM first token: ~500ms
- LLM throughput: ~80 tokens/s

---

## Safety & Limits

| Constraint | Value | Enforcement |
|-----------|-------|-------------|
| Max tool calls per turn | 20 (`max_steps` in agent config) | OpenCode agent config |
| Wall-clock timeout per turn | 120 seconds | `tokio::time::timeout` in ps-server |
| Max question length | 4,000 chars | gRPC handler validation |
| Container idle timeout | 15 minutes | Background reaper in ps-server |
| Container max lifetime | 2 hours | Background reaper |
| Rate limit | 10 queries/min per user | `DashMap<UserId, RateBucket>` |
| Resource limits | 1 CPU, 2Gi RAM, 10Gi ephemeral | K8s Pod spec |
| Network egress | ps-server + RustFS + HTTPS(443) | K8s NetworkPolicy |
| Bash safety | Block rm -rf /, docker, kubectl | OpenCode permission hooks + plugin |
| Max concurrent containers | 20 | ContainerManager pool limit |
| Max artifact size | 50 MB per file | MCP tool validation |
| Max artifacts per conversation | 20 | MCP tool validation |

---

## Cost Estimation

| Component | Tokens | Cost (Claude Sonnet 4.6) |
|-----------|--------|--------------------------|
| System prompt + tool schemas | ~2,000 | $0.006 |
| Tool results (avg 8 calls × 300 tokens) | ~2,400 | $0.0072 |
| User question + context | ~500 | $0.0015 |
| Answer generation | ~1,500 output | $0.0225 |
| **Per-query total** | ~4,900 in / 1,500 out | **~$0.037** |

At 20 queries/day: **~$0.74/day** ($22/month). For cost-sensitive deployments, switch to a cheaper model (e.g. Haiku) via the Admin UI → AI Settings tab — the change takes effect on the next container creation.

S3 storage cost is negligible — artifacts are small files (CSVs, JSON, markdown reports).

---

## Files Summary

### New Files (~30)

| File | Step |
|------|------|
| `migrations/XXXX_create_conversations.sql` | 1 |
| `crates/ps-server/src/container_manager/mod.rs` | 3 |
| `crates/ps-server/src/container_manager/pod_spec.rs` | 3 |
| `crates/ps-server/src/container_manager/event_mapper.rs` | 3 |
| `crates/ps-agent-mcp/` (Rust crate, 5 files) | 4 |
| `agent-container/Dockerfile` | 4 |
| `agent-container/opencode.json` | 4 |
| `agent-container/.opencode/agents/prism.md` | 4 |
| `frontend/views/ask/` (15 component + hook files) | 6 |
| `crates/psctl/src/commands/ask.rs` | 7 |
| `k8s/agent-image-builder.yaml` | 8 |
| `k8s/agent-network-policy.yaml` | 8 |
| `k8s/agent-service-account-secret.yaml` | 8 |

### Modified Files (12)

| File | Step |
|------|------|
| `crates/ps-core/src/repo/reasoning.rs` | 1 |
| `proto/canonical/prism/v1/reasoning.proto` | 2 |
| `crates/ps-server/src/services/reasoning.rs` | 5 |
| `crates/ps-server/Cargo.toml` | 3 |
| `frontend/app.tsx` | 6 |
| `frontend/components/app-sidebar.tsx` | 6 |
| `crates/psctl/src/commands/mod.rs` | 7 |
| `crates/psctl/src/main.rs` | 7 |
| `crates/ps-core/src/backup.rs` | 9 |
| `Tiltfile` | 8 |

---

## Test Matrix

| Category | Unit | Integration | UI |
|----------|------|-------------|-----|
| Conversations repo (CRUD, messages, artifacts) | — | 6 | — |
| Proto (lint, generate) | 2 | — | — |
| ContainerManager (Pod CRUD, OpenCode client) | 3 | 2 | — |
| ps-agent-mcp tools (11 tools) | 11 | 3 | — |
| ps-agent-mcp artifacts | 2 | — | — |
| gRPC handlers (stream, CRUD, resume, auth) | — | 7 | — |
| Streaming hook (state transitions, artifacts, cancel) | — | — | 3 |
| UI components (thinking, artifacts, input, history, evidence, markdown) | — | — | 11 |
| psctl ask | 1 | 1 | — |
| Backup/restore conversations | — | 1 | — |
| K8s deployment | — | 1 | — |
| **Total** | **21** | **21** | **14** |

---

## Implementation Order

```
Week 1: Foundation
  ├─ Step 1: Database migration (conversations, messages, artifacts)
  ├─ Step 2: Proto definitions + buf generate
  └─ Step 4: ps-agent-mcp crate + agent container Dockerfile + OpenCode config

Week 2: Backend
  ├─ Step 3: ContainerManager (kube-rs, OpenCode HTTP client)
  ├─ Step 5: gRPC service handlers (AskQuestion streaming, CRUD, artifacts)
  └─ Step 8: K8s resources (network policy, service account, Tiltfile)

Week 3: Frontend
  ├─ Step 6a: Route + navigation
  ├─ Step 6b: Hooks (streaming, conversations, artifacts)
  └─ Step 6c: Components (ask page, thread, thinking, answer, artifacts)

Week 4: Polish & CLI
  ├─ Step 7: psctl ask command
  ├─ Step 9: Backup/restore extension
  ├─ End-to-end testing (question → container → tools → S3 → answer)
  └─ Traceability audit (every output links to source data)
```

---

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Container cold start too slow (>10s) | Medium | Poor first-query UX | Pre-pull image on nodes. Show "Starting agent..." status. Warm pool of 1-2 idle containers. |
| OpenCode server API changes | Low | Client breaks | Pin OpenCode version in Dockerfile. `opencode-sdk` crate absorbs API changes; pin crate version too. |
| Agent exhausts ephemeral storage (large repos) | Low | Container evicted | 10Gi limit. Shallow clones only. System prompt instructs size-aware behaviour. |
| OpenCode event stream format undocumented | Medium | Event mapping breaks | Test against running OpenCode server. Fall back to polling session result. |
| Bash safety bypass | Low | Security incident | OpenCode permission config + plugin hooks. NetworkPolicy. Non-root user. Resource limits. |
| S3 artifact storage fills up | Low | Upload failures | Max 50MB per file, 20 per conversation. Background cleanup of old artifacts. |
| Session resume loses nuance | Medium | Agent seems forgetful | Store full message history. Inject complete context on resume. System prompt acknowledges. |

---

## Exit Criteria

- [x] `reasoning.conversations`, `conversation_messages`, `conversation_artifacts` tables created
- [x] Agent container image builds: Ubuntu + git + rg + tokei + uv + OpenCode + `ps-mcp` binary
- [x] 11 MCP tools implemented (9 data tools + 2 artifact tools), each calling ps-server gRPC — `crates/ps-mcp/`
- [x] ContainerManager creates, connects, and reaps K8s Pods — `crates/ps-agent/`
- [x] `AskQuestion` streaming RPC works end-to-end
- [x] Frontend `/ask` page shows container status, real-time tool-call progress, streamed answer
- [x] Both MCP tools and system tools (bash, read, grep) visible in thinking panel
- [x] Artifacts uploaded to S3, downloadable from UI and psctl
- [x] Reasoning trace stored and viewable in "Evidence & Reasoning" panel
- [x] Citations in answers link to source data (people, teams, contributions)
- [x] Multi-turn conversations within a container session
- [x] Session resume after container reap (new container + context injection)
- [x] Conversation history browsable with artifact counts
- [ ] "Save as Insight" saves to `reasoning.insights` with `conversation_id` FK — RPC stubbed, pending insights repo integration
- [x] `psctl ask` streams output with `--json` support
- [x] Idle containers reaped (30 min), max lifetime (2 hours)
- [x] Network policy restricts container egress
- [x] uv available in container, system prompt enforces its use for Python
- [x] Backup/restore includes conversation metadata
- [x] `prek run -av` passes with zero warnings

---

## Appendix: Decision Log

Decisions made during the design of this plan, recorded for future context.

| # | Decision | Options considered | Chosen | Rationale |
|---|----------|--------------------|--------|-----------|
| 1 | **Agent runtime** | (a) In-process Rig agent in ps-server, (b) Claude Agent SDK in container, (c) OpenCode in container | OpenCode in container | OpenCode is open-source, multi-provider (75+ via AI SDK), has built-in tools (bash, read, write, grep, glob, webfetch), MCP support, plugin hooks, and session management. Not tied to a single LLM provider. Container isolation gives system tool access (git, rg, tokei, uv/python) without affecting the main server. |
| 2 | **MCP server language** | (a) TypeScript/Bun MCP server using `@modelcontextprotocol/sdk`, (b) Rust crate using `rmcp` | Rust crate (`ps-agent-mcp`) | Shares `ps-proto` types with the backend — no type duplication or separate codegen. Produces a static binary — no Bun/Node.js runtime needed in the container (smaller image, fewer moving parts). Consistent build via `cargo build`. |
| 3 | **ps-server ↔ OpenCode communication** | (a) TypeScript relay wrapping `@opencode-ai/sdk`, (b) Hand-rolled `reqwest` HTTP client, (c) `opencode-sdk` Rust crate | `opencode-sdk` Rust crate | Native async Rust client with typed event handling (40 variants), SSE streaming with reconnection/backoff, session management, and `SessionEventRouter`. Eliminates both a relay process and hand-rolled HTTP code. |
| 4 | **Agent model selection** | (a) Hardcoded in `opencode.json`, (b) Driven by AI Settings config | Driven by AI Settings | Model passed as `OPENCODE_MODEL` env var when Pod is created. `opencode.json` uses `{env:OPENCODE_MODEL}` substitution. Changing the model in Admin UI takes effect on next container creation. |
| 5 | **Container image build in Tilt** | (a) Standing Deployment (always running), (b) Dummy K8s Job referencing the image | Dummy K8s Job | Agent containers are created on-demand, not always running. A Job running `echo Done` ensures Tilt builds and pushes the image without keeping a container alive. |
| 6 | **MCP protocol framework** | (a) Hand-rolled JSON-RPC stdin/stdout, (b) `rmcp` crate (official Rust MCP SDK) | `rmcp` v1.2 | Official MCP SDK with `#[tool]` proc macros for declarative tool definitions, automatic JSON schema generation, and built-in stdio transport. Eliminates ~400 lines of protocol boilerplate. |
