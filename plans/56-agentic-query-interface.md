# Plan 56: Agentic Query Interface (Phase 3 — W3)

## Context

This plan details the implementation of **W3: Agentic Query Interface** from [Phase 3](./14-phase3-intelligence.md). W0 (provider foundation), W1 (enrichment pipeline), and W2 (embeddings & similarity) are complete. The infrastructure they provide — `TaskRouter` with Rig clients, `CostTracker`, enrichment data in `reasoning.enrichments`, vector embeddings in `reasoning.embeddings`, and insight snapshots in `reasoning.insight_snapshots` — forms the foundation for the agentic layer.

**Goal:** Users can ask natural-language questions about their engineering data and receive sourced, auditable answers with full reasoning traces. The agent runs in an isolated Ubuntu container with access to both Prism data tools and real system tools (git, rg, grep, tokei, etc.), enabling deep repository analysis alongside metrics queries. Every claim cites its source. The UI streams the agent's thinking process in real time.

**Dependencies:**
- [14-phase3-intelligence.md](./14-phase3-intelligence.md) — parent plan (W3 section)
- [06-ai-reasoning.md](./06-ai-reasoning.md) — tool design, agentic architecture, container analysis
- [40-adopt-rig-framework.md](./40-adopt-rig-framework.md) — Rig for enrichment/embeddings (W1/W2); agent layer now uses Claude Agent SDK instead

**Key technology:**
- **Claude Agent SDK** (`@anthropic-ai/claude-agent-sdk`, TypeScript) — agent orchestration with built-in tools (Read, Write, Edit, Bash, Glob, Grep) and custom MCP tool registration
- **Ephemeral K8s Pods** — one container per chat session, reaped after idle timeout
- **gRPC server streaming** — `AskQuestion` RPC returns `stream AgentEvent`
- **Connect streaming** — frontend consumes server-streaming RPC via `@connectrpc/connect`

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
│         │           └─────────────────────────────────────────────────┘  │
└─────────┼───────────────────────────────────────────────────────────────┘
          │ gRPC server streaming (AskQuestion)
          ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ ps-server: ReasoningService                                              │
│  ┌────────────────────────┐  ┌──────────────────────────────────────┐   │
│  │ AskQuestion handler     │  │ ContainerManager                     │   │
│  │  1. Find or create Pod  │  │  - create_pod(session_id)           │   │
│  │  2. Connect WebSocket   │  │  - get_pod(session_id)              │   │
│  │  3. Send question        │  │  - reap_idle_pods()                │   │
│  │  4. Relay stream events │  │  - list_active_sessions()           │   │
│  │  5. Store conversation   │  │  Uses kube-rs (K8s API)            │   │
│  └────────────────────────┘  └──────────────────────────────────────┘   │
└─────────┬───────────────────────────────────────────────────────────────┘
          │ WebSocket (bidirectional streaming)
          ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ Agent Container (K8s Pod, 1 per chat session)                            │
│                                                                          │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │ Agent Service (TypeScript, port 8080)                            │    │
│  │  - WebSocket server: receives questions, streams events back     │    │
│  │  - Claude Agent SDK: runs agent with tools + MCP servers         │    │
│  │  - Session management: resume sessions within container lifetime │    │
│  └───────────┬────────────────────┬────────────────────────────────┘    │
│              │                    │                                      │
│  ┌───────────▼──────────┐  ┌─────▼──────────────────────────────┐      │
│  │ Built-in Tools        │  │ Prism MCP Server (in-process)      │      │
│  │  Bash (git, rg, grep, │  │  query_team_metrics()              │      │
│  │    tokei, etc.)        │  │  query_contributions()             │      │
│  │  Read, Write, Edit     │  │  compare_teams()                   │      │
│  │  Glob, Grep            │  │  get_person_profile()              │      │
│  │  WebSearch, WebFetch   │  │  search_similar()                  │      │
│  └───────────────────────┘  │  search_by_text()                   │      │
│                              │  query_enrichments()                │      │
│  ┌───────────────────────┐  │  list_teams()                       │      │
│  │ /workspace/            │  │  list_people()                      │      │
│  │  (cloned repos,        │  │                                     │      │
│  │   analysis outputs)    │  │  → calls ps-server gRPC internally │      │
│  └───────────────────────┘  └─────────────────────────────────────┘      │
│                                                                          │
│  Ubuntu 24.04 (Chisel-slimmed) + git, rg, grep, tokei, Node.js 22      │
└──────────────────────────────────────────────────────────────────────────┘
```

### Why a Separate Container?

1. **System tool access** — the agent can `git clone` repos, run `rg` to search code, use `tokei` for language stats, run `grep` for pattern matching — all in a sandboxed environment with real filesystem access. This enables the "How many repos use tox vs uv?" class of questions from [06-ai-reasoning.md](./06-ai-reasoning.md#repository-analysis-via-containers).

2. **Isolation** — each session gets its own filesystem, process space, and resource limits. A runaway agent can't affect other sessions or the main server.

3. **Claude Agent SDK** — the SDK bundles Claude Code's full toolset (Read, Write, Edit, Bash, Glob, Grep, WebSearch). We get battle-tested file navigation, code search, and command execution for free. Custom Prism tools are registered as MCP servers.

4. **Resource control** — K8s resource limits (CPU, memory, ephemeral storage) prevent abuse. Network policies restrict egress to ps-server + GitHub/Jira APIs only.

---

## Navigation & Page Placement

The agentic interface appears as a **top-level navigation item** in the sidebar, positioned after "Ingestion". It uses the `Sparkles` icon from Lucide.

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

**Route:** `/ask` (new session), `/ask/:conversationId` (resume a conversation)

The "Ask" page is a **full-page chat interface**. Agent answers often contain tables, multi-paragraph analysis, code snippets from repo scans, and inline citations — they need room.

---

## UI Mockups

### Empty State (`/ask`, no conversations)

```
┌─ PageHeader ──────────────────────────────────────────────────┐
│ ☰ │ Ask Prism                                                 │
│     Ask questions about your engineering data          [History]│
└───────────────────────────────────────────────────────────────┘
┌───────────────────────────────────────────────────────────────┐
│                                                               │
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
│   │ Suggested questions:                  │                  │
│   │                                       │                  │
│   │  ┌─────────────────────────────────┐  │                  │
│   │  │ How has Team X's review quality │  │                  │
│   │  │ changed this quarter?           │  │                  │
│   │  └─────────────────────────────────┘  │                  │
│   │  ┌─────────────────────────────────┐  │                  │
│   │  │ Who are the most thorough       │  │                  │
│   │  │ reviewers across the org?       │  │                  │
│   │  └─────────────────────────────────┘  │                  │
│   │  ┌─────────────────────────────────┐  │                  │
│   │  │ How many repos have migrated    │  │                  │
│   │  │ from tox to uv?                 │  │                  │
│   │  └─────────────────────────────────┘  │                  │
│   │  ┌─────────────────────────────────┐  │                  │
│   │  │ Compare throughput between      │  │                  │
│   │  │ Team A and Team B this month    │  │                  │
│   │  └─────────────────────────────────┘  │                  │
│   └───────────────────────────────────────┘                  │
│                                                               │
├───────────────────────────────────────────────────────────────┤
│ ┌──────────────────────────────────────────────────────┐ [▶] │
│ │ Ask a question...                                    │      │
│ └──────────────────────────────────────────────────────┘      │
└───────────────────────────────────────────────────────────────┘
```

### Active Conversation (streaming response)

```
┌─ PageHeader ──────────────────────────────────────────────────┐
│ ☰ │ Ask Prism                                                 │
│     Ask questions about your engineering data          [History]│
└───────────────────────────────────────────────────────────────┘
┌───────────────────────────────────────────────────────────────┐
│                                                               │
│  ┌─ You ─────────────────────────────────────────────────┐   │
│  │ How has Team Kernel's review quality changed this      │   │
│  │ quarter?                                               │   │
│  └────────────────────────────────────────────────────────┘   │
│                                                               │
│  ┌─ Prism ───────────────────────────────────────────────┐   │
│  │                                                        │   │
│  │  ┌─ Thinking ──────────────────────────────────── ▾ ┐ │   │
│  │  │  ✓ mcp: list_teams("Kernel")                     │ │   │
│  │  │    → Found "Kernel" (team_abc)                    │ │   │
│  │  │  ✓ mcp: query_team_metrics(team_abc, Q1 2026)    │ │   │
│  │  │    → 142 PRs merged, avg depth 3.1               │ │   │
│  │  │  ✓ mcp: query_team_metrics(team_abc, Q4 2025)    │ │   │
│  │  │    → 128 PRs merged, avg depth 2.4               │ │   │
│  │  │  ⟳ mcp: query_contributions(team_abc, reviews)   │ │   │
│  │  │    Fetching review data...                        │ │   │
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

### Active Conversation — Repo Analysis (agent using system tools)

When the agent needs to analyse actual repository code, it uses built-in Bash/Glob/Grep tools. The thinking panel shows these distinctly:

```
│  ┌─ Thinking ──────────────────────────────────────── ▾ ┐    │
│  │  ✓ mcp: list_teams("Kernel")                        │    │
│  │    → Found "Kernel" (team_abc), 12 repos             │    │
│  │  ✓ bash: git clone --depth 1 ubuntu/kernel-snaps     │    │
│  │    → Cloned to /workspace/kernel-snaps               │    │
│  │  ✓ bash: rg -l "tox.ini" /workspace/kernel-snaps    │    │
│  │    → 3 files found                                   │    │
│  │  ✓ bash: rg -l "pyproject.toml" /workspace/kernel-…│    │
│  │    → 5 files found                                   │    │
│  │  ✓ grep: "uv" in /workspace/kernel-snaps/pyproject…│    │
│  │    → Found uv dependency in 2 pyproject.toml files   │    │
│  │  ✓ bash: git clone --depth 1 ubuntu/kernel-sru      │    │
│  │    → Cloned to /workspace/kernel-sru                 │    │
│  │  ⟳ bash: rg -l "tox.ini" /workspace/kernel-sru      │    │
│  │    Running...                                        │    │
│  └──────────────────────────────────────────────────────┘    │
```

### Completed Response (with citations)

```
│  ┌─ Prism ───────────────────────────────────────────────┐   │
│  │                                                        │   │
│  │  ┌─ Thinking (8 steps) ─────────────────────── ▸ ┐   │   │
│  │  └──────────────────────────────────────────────────┘ │   │
│  │                                                        │   │
│  │  ## Team Kernel — Review Quality, Q1 2026              │   │
│  │                                                        │   │
│  │  Review quality has **improved significantly** this     │   │
│  │  quarter compared to Q4 2025:                          │   │
│  │                                                        │   │
│  │  | Metric             | Q4 2025 | Q1 2026 | Change   | │   │
│  │  |--------------------|---------|---------|----------| │   │
│  │  | Avg review depth   | 2.4     | 3.1     | +0.7 ▲  | │   │
│  │  | Rubber-stamp %     | 34%     | 18%     | −16% ▲  | │   │
│  │  | Deep reviews (4+)  | 12%     | 28%     | +16% ▲  | │   │
│  │  | Reviews given       | 89      | 142     | +60%    | │   │
│  │                                                        │   │
│  │  The biggest driver appears to be **@alice** and       │   │
│  │  **@bob**, whose average depth scores rose from 2.1    │   │
│  │  to 3.8 [¹] and 1.9 to 3.5 [²] respectively.         │   │
│  │                                                        │   │
│  │  ─────────────────────────────────────────────────     │   │
│  │  [¹] Alice's review profile · /people/alice_id         │   │
│  │  [²] Bob's review profile · /people/bob_id             │   │
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
│  │  │  Model: claude-sonnet-4-6                        │  │   │
│  │  │  Tokens: 4,231 in / 1,847 out                   │  │   │
│  │  │  Duration: 8.3s (8 tool calls)                   │  │   │
│  │  │  Container: prism-agent-a7f3c (active)           │  │   │
│  │  │                                                   │  │   │
│  │  │  Step 1: mcp__prism__list_teams("Kernel")        │  │   │
│  │  │    → Resolved "Kernel" to team_abc               │  │   │
│  │  │                                                   │  │   │
│  │  │  Step 2: mcp__prism__query_team_metrics(...)     │  │   │
│  │  │    → Q1 2026: avg_depth 3.1, reviews 142        │  │   │
│  │  │                                                   │  │   │
│  │  │  Step 3: mcp__prism__query_team_metrics(...)     │  │   │
│  │  │    → Q4 2025: avg_depth 2.4, reviews 89         │  │   │
│  │  │                                                   │  │   │
│  │  │  Step 4: mcp__prism__query_contributions(...)    │  │   │
│  │  │    → 142 reviews, top: alice (38), bob (29)     │  │   │
│  │  │                                                   │  │   │
│  │  │  Step 5: mcp__prism__get_person_profile(alice)   │  │   │
│  │  │    → avg_depth 3.8, constructive 32/38          │  │   │
│  │  │                                                   │  │   │
│  │  └──────────────────────────────────────────────────┘ │   │
```

### Conversation History Panel (Sheet)

Clicking **[History]** in the page header opens a right-side sheet:

```
┌─ Conversation History ──────────────────── ✕ ┐
│                                               │
│  🔍 Search conversations...                   │
│                                               │
│  ┌──────────────────────────────────────────┐ │
│  │ How has Team Kernel's review quality...  │ │
│  │ 8 tool calls · Mar 23, 14:30            │ │
│  │ ● container active                       │ │
│  └──────────────────────────────────────────┘ │
│  ┌──────────────────────────────────────────┐ │
│  │ How many repos use tox vs uv?            │ │
│  │ 14 tool calls · Mar 22, 09:15           │ │
│  │ ○ container reaped                       │ │
│  └──────────────────────────────────────────┘ │
│                                               │
└───────────────────────────────────────────────┘
```

Conversations with active containers show a green dot. Reaped containers show a grey dot — clicking resumes with a new container but the agent receives the prior conversation context.

---

## Container Lifecycle

### Creation

When a user starts a new chat (or resumes a reaped session), `ps-server` creates a K8s Pod:

```
User sends first question
  │
  ▼
ps-server: ContainerManager.create_pod(session_id)
  │
  ├── Generate Pod spec:
  │     image: prism-agent:latest
  │     env: PRISM_API_URL, ANTHROPIC_API_KEY, SESSION_ID
  │     resources: { cpu: "500m", memory: "1Gi", ephemeral: "5Gi" }
  │     labels: { app: prism-agent, session: <id> }
  │
  ├── Create Pod via kube-rs
  │
  ├── Wait for Pod ready (readiness probe on :8080/health)
  │
  └── Return Pod IP + port
```

### Communication

`ps-server` connects to the agent container via WebSocket (`ws://<pod-ip>:8080/ws`). Messages are JSON-encoded:

**Server → Agent:**
```json
{
  "type": "question",
  "question": "How has Team Kernel's review quality changed?",
  "conversation_history": [
    { "role": "user", "content": "..." },
    { "role": "assistant", "content": "..." }
  ]
}
```

**Agent → Server (streamed events):**
```json
{ "type": "tool_call_started", "tool_name": "mcp__prism__list_teams", "arguments": "{...}" }
{ "type": "tool_call_completed", "tool_name": "mcp__prism__list_teams", "result_summary": "..." }
{ "type": "text_delta", "text": "Review quality has **improved**..." }
{ "type": "thinking", "text": "I should compare Q1 to Q4..." }
{ "type": "result", "text": "full answer...", "session_id": "sdk-session-xyz" }
```

### Idle Timeout & Reaping

Each agent container has an **idle timer** (default: 15 minutes). The timer resets on each question received. When the timer fires:

1. Agent container sends a `{ "type": "idle_shutdown" }` event (if ps-server is connected)
2. Container exits gracefully
3. `ContainerManager` detects Pod termination, marks session as `container_reaped`
4. Conversation history remains in `reasoning.conversations` — the container is ephemeral, the data is not

A **background reaper** in `ps-server` periodically checks for Pods that have been running longer than a max lifetime (e.g. 2 hours) and deletes them, regardless of activity. This prevents forgotten sessions from consuming cluster resources.

### Session Resume

When a user resumes a conversation whose container was reaped:

1. `ps-server` creates a new Pod (same as initial creation)
2. Loads full conversation history from `reasoning.conversations`
3. Sends history to agent container as `conversation_history` in the first question message
4. Agent SDK receives this as context — the agent "remembers" the prior conversation
5. Any previously cloned repos must be re-cloned (the workspace is ephemeral)

The agent's system prompt includes instructions to acknowledge when context comes from a prior session and to re-clone repos if needed for follow-up questions about code.

### Resource Limits & Network Policy

```yaml
# Pod resource limits
resources:
  requests:
    cpu: "250m"
    memory: "512Mi"
  limits:
    cpu: "1000m"
    memory: "2Gi"
    ephemeral-storage: "10Gi"

# Network policy: restrict egress
egress:
  - to:
    - podSelector:
        matchLabels:
          app: ps-server          # gRPC callbacks for MCP tools
    ports:
      - port: 50051
  - to:
    - ipBlock:
        cidr: 0.0.0.0/0          # GitHub, Jira, Discourse APIs for repo cloning
    ports:
      - port: 443
```

---

## Agent Container — Detailed Design

### Container Image

```dockerfile
FROM ubuntu:24.04

# System tools for code analysis
RUN apt-get update && apt-get install -y \
    git curl ca-certificates \
    ripgrep \
    && rm -rf /var/lib/apt/lists/*

# tokei (code statistics)
RUN curl -sL https://github.com/XAMPPRocky/tokei/releases/latest/download/tokei-x86_64-unknown-linux-gnu.tar.gz \
    | tar xz -C /usr/local/bin

# Node.js 22 LTS
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && \
    apt-get install -y nodejs && \
    rm -rf /var/lib/apt/lists/*

# Agent service
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --production
COPY dist/ ./dist/

# Workspace for cloned repos
RUN mkdir /workspace

ENV NODE_ENV=production
EXPOSE 8080

CMD ["node", "dist/server.js"]
```

The final image will be slimmed with Chisel per [feedback_containers.md].

### Agent Service (`agent-service/`)

A lightweight TypeScript service that:
1. Runs an HTTP + WebSocket server on port 8080
2. Receives questions from ps-server via WebSocket
3. Runs the Claude Agent SDK with registered tools
4. Streams events back to ps-server

```
agent-service/
├── package.json
├── tsconfig.json
├── src/
│   ├── server.ts              # HTTP + WebSocket server (health, /ws endpoint)
│   ├── agent.ts               # Claude Agent SDK wrapper: build agent, run query
│   ├── tools/                 # Prism MCP tool definitions
│   │   ├── index.ts           # create_sdk_mcp_server with all tools
│   │   ├── query-metrics.ts   # query_team_metrics tool
│   │   ├── query-contribs.ts  # query_contributions tool
│   │   ├── compare-teams.ts   # compare_teams tool
│   │   ├── person-profile.ts  # get_person_profile tool
│   │   ├── search-similar.ts  # search_similar tool
│   │   ├── search-text.ts     # search_by_text tool
│   │   ├── query-enrich.ts    # query_enrichments tool
│   │   ├── list-teams.ts      # list_teams tool
│   │   └── list-people.ts     # list_people tool
│   ├── prism-client.ts        # gRPC client to ps-server (for MCP tool data)
│   ├── event-mapper.ts        # Map Agent SDK messages → WebSocket events
│   └── types.ts               # Shared types
└── Dockerfile
```

### Agent SDK Configuration

```typescript
// agent.ts
import { ClaudeSDKClient, ClaudeAgentOptions } from "@anthropic-ai/claude-agent-sdk";
import { createPrismMcpServer } from "./tools/index.js";

const buildAgentOptions = (prismApiUrl: string): ClaudeAgentOptions => ({
  systemPrompt: SYSTEM_PROMPT,
  cwd: "/workspace",
  maxTurns: 15,
  permissionMode: "acceptEdits",    // auto-approve all tool use in container
  allowedTools: [
    "Read", "Write", "Edit", "Bash", "Glob", "Grep",
    "WebFetch",
    // Prism MCP tools (auto-discovered from server name)
    "mcp__prism__query_team_metrics",
    "mcp__prism__query_contributions",
    "mcp__prism__compare_teams",
    "mcp__prism__get_person_profile",
    "mcp__prism__search_similar",
    "mcp__prism__search_by_text",
    "mcp__prism__query_enrichments",
    "mcp__prism__list_teams",
    "mcp__prism__list_people",
  ],
  disallowedTools: [
    "Agent",           // No sub-agents (cost control)
    "WebSearch",       // Disable web search (not needed, costs money)
  ],
  mcpServers: {
    prism: createPrismMcpServer(prismApiUrl),
  },
  hooks: {
    PreToolUse: [
      {
        matcher: "Bash",
        hooks: [guardBashCommands],   // Block dangerous commands
      },
    ],
  },
});
```

### System Prompt

```typescript
const SYSTEM_PROMPT = `You are Prism, an engineering insights assistant deployed at Canonical.
You help users understand their engineering data across GitHub, Jira, Discourse, and other platforms.

## Available tools

You have two categories of tools:

### Prism data tools (MCP)
Use these to query pre-computed metrics, enrichments, and team/people data:
- mcp__prism__query_team_metrics — DORA, flow, review metrics for a team + period
- mcp__prism__query_contributions — search/filter contributions with flexible criteria
- mcp__prism__compare_teams — side-by-side metrics for 2+ teams
- mcp__prism__get_person_profile — individual activity summary across platforms
- mcp__prism__search_similar — find semantically similar contributions (embeddings)
- mcp__prism__search_by_text — semantic search over all contributions
- mcp__prism__query_enrichments — get AI enrichment scores for a contribution
- mcp__prism__list_teams — browse team hierarchy (also resolves names to IDs)
- mcp__prism__list_people — browse people, filtered by team

### System tools
Use these for repository analysis and code inspection:
- Bash — run commands: git clone, rg, grep, tokei, wc, find, etc.
- Read — read files from cloned repos
- Glob — find files by pattern in /workspace
- Grep — search file contents with regex in /workspace

## Guidelines

1. ALWAYS use Prism MCP tools for metrics and data queries. Never guess numbers.
2. Use system tools when the question requires inspecting actual code (e.g. "which repos use tox?")
3. For repo analysis: clone repos to /workspace/<repo-name> with --depth 1 (shallow clone)
4. Cite every claim: use footnote references [¹][²] linking to /people/, /teams/, /contributions/ paths
5. Format answers in Markdown with tables for comparisons
6. When context comes from a prior session, acknowledge it and re-clone repos if needed
7. If you cannot answer with available tools, say so clearly. Do not hallucinate.

## Current deployment context
- Current date: ${new Date().toISOString().split("T")[0]}
- Workspace: /workspace (ephemeral, empty at session start)
`;
```

### Bash Safety Hook

```typescript
const guardBashCommands = async (inputData: any) => {
  const command = inputData?.tool_input?.command ?? "";

  // Block destructive or escape-attempt commands
  const blocked = [
    /rm\s+-rf\s+\/(?!workspace)/,     // rm -rf outside /workspace
    /curl.*\|\s*(?:bash|sh)/,          // curl | bash (arbitrary execution)
    /nc\s+-l/,                         // netcat listen (reverse shell)
    /chmod\s+\+s/,                     // setuid
    /docker|kubectl|podman/,           // container escape attempts
  ];

  for (const pattern of blocked) {
    if (pattern.test(command)) {
      return {
        hookSpecificOutput: {
          hookEventName: "PreToolUse",
          permissionDecision: "deny",
          permissionDecisionReason: `Blocked: command matches restricted pattern`,
        },
      };
    }
  }
  return {};
};
```

### Prism MCP Server (Custom Tools)

Each tool is an in-process MCP tool that calls back to ps-server's gRPC API:

```typescript
// tools/index.ts
import { tool, createSdkMcpServer } from "@anthropic-ai/claude-agent-sdk";
import { createPrismClient } from "../prism-client.js";

export const createPrismMcpServer = (apiUrl: string) => {
  const client = createPrismClient(apiUrl);

  const queryTeamMetrics = tool(
    "query_team_metrics",
    "Get DORA, flow, and review metrics for a team over a time period",
    {
      team_name: { type: "string", description: "Team name (resolved via list_teams)" },
      period_start: { type: "string", description: "Start date (YYYY-MM-DD)" },
      period_end: { type: "string", description: "End date (YYYY-MM-DD)" },
    },
    async (args) => {
      const result = await client.queryTeamMetrics(args);
      return { content: [{ type: "text", text: JSON.stringify(result) }] };
    },
  );

  // ... (same pattern for all 9 tools)

  return createSdkMcpServer({
    name: "prism",
    version: "1.0.0",
    tools: [
      queryTeamMetrics,
      queryContributions,
      compareTeams,
      getPersonProfile,
      searchSimilar,
      searchByText,
      queryEnrichments,
      listTeams,
      listPeople,
    ],
  });
};
```

### Prism gRPC Client (Data Access)

The MCP tools don't access the database directly. They call ps-server's existing gRPC APIs:

```typescript
// prism-client.ts
import { createClient } from "@connectrpc/connect";
import { createGrpcTransport } from "@connectrpc/connect-node";
import { MetricsService } from "./gen/canonical/prism/v1/metrics_connect.js";
import { OrgService } from "./gen/canonical/prism/v1/org_connect.js";
import { ReasoningService } from "./gen/canonical/prism/v1/reasoning_connect.js";
import { InsightsService } from "./gen/canonical/prism/v1/insights_connect.js";

export const createPrismClient = (apiUrl: string) => {
  const transport = createGrpcTransport({ baseUrl: apiUrl });

  return {
    metrics: createClient(MetricsService, transport),
    org: createClient(OrgService, transport),
    reasoning: createClient(ReasoningService, transport),
    insights: createClient(InsightsService, transport),
  };
};
```

The agent container authenticates to ps-server using a **service account API token** passed as an environment variable. This token has read-only access to all data (no mutations). The token is created during setup and stored as a K8s Secret.

### Event Mapping (Agent SDK → WebSocket)

```typescript
// event-mapper.ts
import type { AssistantMessage, ToolUseBlock, TextBlock } from "@anthropic-ai/claude-agent-sdk";

export const mapSdkMessage = (message: any): WsEvent[] => {
  const events: WsEvent[] = [];

  if (message.type === "assistant" && message.content) {
    for (const block of message.content) {
      if (block.type === "tool_use") {
        events.push({
          type: "tool_call_started",
          toolName: block.name,
          argumentsJson: JSON.stringify(block.input),
        });
      } else if (block.type === "text") {
        events.push({
          type: "text_delta",
          text: block.text,
        });
      }
    }
  }

  if (message.type === "tool_result") {
    events.push({
      type: "tool_call_completed",
      toolName: message.tool_name,
      resultSummary: truncate(extractText(message.content), 200),
    });
  }

  if (message.type === "result") {
    events.push({
      type: "result",
      text: message.result,
      sessionId: message.session_id,
    });
  }

  return events;
};
```

---

## Implementation Steps

### Step 1: Database — Conversations Table + Container Sessions

Add `reasoning.conversations` to store query history and container session state.

**Migration: `XXXX_create_conversations.sql`**

```sql
CREATE TABLE reasoning.conversations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES auth.users(id),
    question TEXT NOT NULL,
    answer TEXT,
    reasoning_trace JSONB,
    supporting_data JSONB,
    model_name TEXT NOT NULL DEFAULT 'claude-sonnet-4-6',
    status TEXT NOT NULL DEFAULT 'processing',
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
    -- Container lifecycle
    container_pod_name TEXT,
    container_status TEXT NOT NULL DEFAULT 'pending',
    sdk_session_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX idx_conversations_user
    ON reasoning.conversations(user_id, created_at DESC);

CREATE INDEX idx_conversations_container
    ON reasoning.conversations(container_pod_name)
    WHERE container_status = 'active';

-- Multi-turn: messages within a conversation
CREATE TABLE reasoning.conversation_messages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES reasoning.conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL,          -- 'user' or 'assistant'
    content TEXT NOT NULL,
    tool_calls JSONB,           -- tool call details for assistant messages
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conversation_messages_conv
    ON reasoning.conversation_messages(conversation_id, created_at);

-- Link insights back to conversations
ALTER TABLE reasoning.insights
    ADD COLUMN conversation_id UUID REFERENCES reasoning.conversations(id);
```

**Reasoning trace JSONB schema:**

```json
{
  "steps": [
    {
      "index": 0,
      "tool_name": "mcp__prism__list_teams",
      "arguments": { "search": "Kernel" },
      "result_summary": "Found 'Kernel' (team_abc), 12 repos",
      "duration_ms": 45,
      "timestamp": "2026-03-23T14:30:01Z"
    },
    {
      "index": 3,
      "tool_name": "Bash",
      "arguments": { "command": "git clone --depth 1 https://github.com/ubuntu/kernel-snaps /workspace/kernel-snaps" },
      "result_summary": "Cloned successfully (3.2 MB)",
      "duration_ms": 2100,
      "timestamp": "2026-03-23T14:30:05Z"
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
| `crates/ps-core/src/repo/reasoning.rs` | **Modify** — add `create_conversation()`, `update_conversation()`, `get_conversation()`, `list_conversations()`, `add_message()`, `get_messages()`, `update_container_status()` |

**Testing:**

| Level | Test |
|-------|------|
| Integration (repo) | `define_repo_test!` — create conversation, add messages, update with answer + trace, list by user, verify JSONB round-trip |
| Integration (repo) | Multi-turn: create conversation, add 4 messages (user/assistant alternating), retrieve in order |
| Integration (repo) | Container status transitions: pending → active → reaped → active (resume) |
| Integration (repo) | `insights.conversation_id` FK — create conversation, save insight linked to it, query back |

---

### Step 2: Proto — Streaming RPC + Conversation Management

Add the streaming `AskQuestion` RPC and conversation management RPCs to `reasoning.proto`.

**Proto additions:**

```protobuf
// --- Add to ReasoningService ---

// AskQuestion submits a natural-language question and streams back agent
// events (thinking steps, tool calls, partial answer, final answer).
// Spins up an agent container if one isn't already active for this session.
rpc AskQuestion(AskQuestionRequest) returns (stream AgentEvent);

// ListConversations returns recent conversations for the current user.
rpc ListConversations(ListConversationsRequest) returns (ListConversationsResponse);

// GetConversation returns a conversation with its full reasoning trace.
rpc GetConversation(GetConversationRequest) returns (GetConversationResponse);

// SaveInsightFromConversation saves an agent's answer as a named insight.
rpc SaveInsightFromConversation(SaveInsightFromConversationRequest)
    returns (SaveInsightFromConversationResponse);

// --- New message types ---

message AskQuestionRequest {
  string question = 1;
  // Resume an existing conversation (multi-turn or after container reap).
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
  }
}

message AgentToolCallStarted {
  int32 step_index = 1;
  // Tool name (e.g. "mcp__prism__list_teams", "Bash", "Grep").
  string tool_name = 2;
  string arguments_json = 3;
}

message AgentToolCallCompleted {
  int32 step_index = 1;
  string tool_name = 2;
  string result_summary = 3;
  int32 duration_ms = 4;
}

message AgentThinking {
  string text = 1;
}

message AgentPartialAnswer {
  string text = 1;
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
}

message AgentError {
  string message = 1;
  bool retryable = 2;
}

// Emitted when the container is being prepared (creation, readiness).
message AgentContainerStatus {
  // "creating", "ready", "reconnecting" (after reap)
  string status = 1;
  string message = 2;
}

message ListConversationsRequest {
  int32 limit = 1;
  int32 offset = 2;
}

message ListConversationsResponse {
  repeated ConversationSummary conversations = 1;
  int32 total_count = 2;
}

message ConversationSummary {
  string id = 1;
  string question = 2;
  string answer_preview = 3;
  string status = 4;
  string model_name = 5;
  int32 tool_call_count = 6;
  string container_status = 7;
  google.protobuf.Timestamp created_at = 8;
}

message GetConversationRequest {
  string id = 1;
}

message GetConversationResponse {
  string id = 1;
  string question = 2;
  string answer = 3;
  string reasoning_trace_json = 4;
  string supporting_data_json = 5;
  string model_name = 6;
  string status = 7;
  int32 prompt_tokens = 8;
  int32 completion_tokens = 9;
  double estimated_cost_usd = 10;
  string container_status = 11;
  repeated ConversationMessage messages = 12;
  google.protobuf.Timestamp created_at = 13;
  optional google.protobuf.Timestamp completed_at = 14;
}

message ConversationMessage {
  string role = 1;
  string content = 2;
  string tool_calls_json = 3;
  google.protobuf.Timestamp created_at = 4;
}

message SaveInsightFromConversationRequest {
  string conversation_id = 1;
  string title = 2;
}

message SaveInsightFromConversationResponse {
  string insight_id = 1;
}
```

**Files created/changed:**

| File | Action |
|------|--------|
| `proto/canonical/prism/v1/reasoning.proto` | **Modify** — add RPCs and message types |
| `frontend/lib/api/gen/canonical/prism/v1/reasoning_connect.ts` | **Auto-generated** by `buf generate` |
| `frontend/lib/api/gen/canonical/prism/v1/reasoning_pb.ts` | **Auto-generated** by `buf generate` |
| `crates/ps-proto/src/gen/canonical.prism.v1.rs` | **Auto-generated** by `buf generate` |

**Testing:**

| Level | Test |
|-------|------|
| Lint | `buf lint` passes |
| Build | `buf generate` produces valid Rust + TypeScript |

---

### Step 3: Container Manager — K8s Pod Lifecycle

A new module in `ps-server` that manages agent container pods using `kube-rs`.

**Key operations:**

1. **`create_pod(session_id, service_token)`** — Create a K8s Pod running `prism-agent:latest`. Set environment variables: `PRISM_API_URL` (internal service URL), `ANTHROPIC_API_KEY` (from K8s Secret), `SESSION_ID`, `SERVICE_TOKEN`. Apply resource limits and network policy. Wait for readiness.

2. **`get_pod(session_id)`** — Look up active Pod by session label. Return Pod IP + port if running, `None` if reaped.

3. **`connect(pod_ip)`** — Establish WebSocket connection to agent container. Return bidirectional stream.

4. **`reap_idle_pods()`** — Background task running every 60s. List Pods with `app=prism-agent` label. Check last-activity annotation. Delete Pods idle > 15 min or running > 2 hours.

5. **`delete_pod(session_id)`** — Force-delete a Pod (used on conversation delete or user request).

**Implementation notes:**
- Uses `kube::Client` (created once, shared via `Arc`)
- Pods are created in a dedicated namespace (`prism-agents`) or the same namespace with distinct labels
- Pod names are deterministic: `prism-agent-{session_id_short}`
- The `last-activity` annotation is updated by ps-server on each question relay

**Files created/changed:**

| File | Action |
|------|--------|
| `crates/ps-server/src/container_manager.rs` | **Create** — `ContainerManager` struct, K8s Pod CRUD, WebSocket client, idle reaper |
| `crates/ps-server/src/container_manager/pod_spec.rs` | **Create** — Pod spec builder (resource limits, env vars, labels, network policy) |
| `crates/ps-server/Cargo.toml` | **Modify** — add `kube` (with `runtime`, `client` features), `k8s-openapi` (with v1_31), `tokio-tungstenite` (WebSocket client) |

**Testing:**

| Level | Test |
|-------|------|
| Unit | Pod spec builder — verify labels, env vars, resource limits, container image |
| Unit | Pod name generation — deterministic from session ID, valid K8s name |
| Integration | Create Pod → wait ready → connect WebSocket → send ping → receive pong → delete Pod (requires K8s API access; skip in CI without K8s) |
| Integration | Idle reaper — create Pod with old last-activity annotation, run reaper, verify Pod deleted |

---

### Step 4: Agent Service — TypeScript Container Application

The TypeScript application that runs inside the agent container.

**Directory structure:**

```
agent-service/
├── package.json
├── tsconfig.json
├── Dockerfile
├── src/
│   ├── server.ts                    # HTTP + WebSocket server
│   ├── agent.ts                     # Claude Agent SDK wrapper
│   ├── event-mapper.ts              # SDK messages → WebSocket events
│   ├── idle-timer.ts                # Idle timeout (self-terminate)
│   ├── bash-guard.ts                # PreToolUse hook for Bash safety
│   ├── types.ts                     # Shared types
│   ├── prism-client.ts              # gRPC client to ps-server
│   └── tools/
│       ├── index.ts                 # createPrismMcpServer()
│       ├── query-metrics.ts         # query_team_metrics
│       ├── query-contribs.ts        # query_contributions
│       ├── compare-teams.ts         # compare_teams
│       ├── person-profile.ts        # get_person_profile
│       ├── search-similar.ts        # search_similar
│       ├── search-text.ts           # search_by_text
│       ├── query-enrich.ts          # query_enrichments
│       ├── list-teams.ts            # list_teams
│       └── list-people.ts           # list_people
├── tests/
│   ├── event-mapper.test.ts         # Event mapping unit tests
│   ├── bash-guard.test.ts           # Bash safety hook tests
│   ├── idle-timer.test.ts           # Idle timeout tests
│   └── tools/
│       └── query-metrics.test.ts    # Tool integration tests (mock gRPC)
└── vitest.config.ts
```

#### Tool Details

Each MCP tool wraps a call to ps-server's existing gRPC API. The tools do not access the database directly.

**Tool 1: `query_team_metrics`**
- Input: `{ team_name, period_start, period_end }`
- Calls: `InsightsService.GetTeamInsights` + `MetricsService.GetTeamMetrics`
- Returns: DORA metrics, flow metrics, review quality summary, enrichment-based insights

**Tool 2: `query_contributions`**
- Input: `{ team_name?, person_name?, platform?, contribution_type?, state?, date_from?, date_to?, limit?, sort_by? }`
- Calls: `MetricsService.ListContributions` (resolving names to IDs first via OrgService)
- Returns: contribution list with titles, authors, states, dates, URLs

**Tool 3: `compare_teams`**
- Input: `{ team_names[], period_start, period_end, metrics[] }`
- Calls: `InsightsService.GetTeamInsights` for each team, `MetricsService.GetTeamMetrics` for each
- Returns: side-by-side comparison table as JSON

**Tool 4: `get_person_profile`**
- Input: `{ person_name, period_start?, period_end? }`
- Calls: `OrgService.GetPerson`, `InsightsService.GetPersonInsights`, `MetricsService.GetPersonMetrics`
- Returns: activity summary, review profile, PR impact, platform breakdown

**Tool 5: `search_similar`**
- Input: `{ contribution_id, limit?, platform_filter? }`
- Calls: `ReasoningService.FindSimilar`
- Returns: ranked similar contributions with distance scores

**Tool 6: `search_by_text`**
- Input: `{ query, limit?, platform_filter? }`
- Calls: `ReasoningService.SearchByText`
- Returns: semantically matching contributions

**Tool 7: `query_enrichments`**
- Input: `{ contribution_id }`
- Calls: `ReasoningService.GetEnrichments`
- Returns: all enrichment types with scores, rationale, confidence

**Tool 8: `list_teams`**
- Input: `{ search? }`
- Calls: `OrgService.ListTeams`
- Returns: team names, IDs, member counts, hierarchy

**Tool 9: `list_people`**
- Input: `{ team_name?, search? }`
- Calls: `OrgService.ListPeople`
- Returns: names, IDs, team memberships

**Files created:**

| File | Action |
|------|--------|
| All files in `agent-service/` above | **Create** (new top-level directory alongside `frontend/` and `crates/`) |

**Testing:**

| Level | Test |
|-------|------|
| Unit | `event-mapper.ts` — map each SDK message type to correct WebSocket event |
| Unit | `bash-guard.ts` — verify blocked patterns (rm -rf /, curl\|bash, docker, kubectl) and allowed patterns (git clone, rg, tokei) |
| Unit | `idle-timer.ts` — verify timer fires after inactivity, verify reset on activity |
| Unit | Each tool in `tools/` — mock gRPC client, verify correct RPC called with correct args, verify output format |
| Integration | Full agent flow with mock SDK — send question, verify event sequence through WebSocket |

---

### Step 5: gRPC Service — AskQuestion Streaming Handler

Wire the container manager into `ReasoningService` as a server-streaming RPC. The handler orchestrates container lifecycle and relays events.

```rust
async fn ask_question(
    &self,
    request: Request<AskQuestionRequest>,
) -> Result<Response<Streaming<AgentEvent>>, Status> {
    let user = require_auth(&request)?;
    let req = request.into_inner();

    // 1. Validate question (non-empty, <4000 chars)
    // 2. Rate limit check (10 queries/min/user)

    let (tx, rx) = mpsc::channel(64);

    // Determine conversation context
    let (conversation_id, history) = if let Some(id) = req.conversation_id {
        // Resuming existing conversation
        let conv = self.repos.reasoning().get_conversation(&id).await?;
        let msgs = self.repos.reasoning().get_messages(&id).await?;
        (conv.id, msgs)
    } else {
        // New conversation
        let id = self.repos.reasoning()
            .create_conversation(user.id, &req.question, "claude-sonnet-4-6")
            .await?;
        (id, vec![])
    };

    // Add user message
    self.repos.reasoning()
        .add_message(conversation_id, "user", &req.question, None)
        .await?;

    let container_mgr = self.container_manager.clone();
    let repos = self.repos.clone();
    let question = req.question.clone();
    let conv_id = conversation_id;

    tokio::spawn(async move {
        // 1. Find or create container
        tx.send(AgentEvent::container_status("creating", "Starting agent container...")).await.ok();

        let pod = match container_mgr.ensure_pod(conv_id.to_string()).await {
            Ok(p) => p,
            Err(e) => {
                tx.send(AgentEvent::error(&format!("Container error: {e}"), true)).await.ok();
                return;
            }
        };

        tx.send(AgentEvent::container_status("ready", "Agent ready")).await.ok();
        repos.reasoning().update_container_status(conv_id, "active", &pod.name).await.ok();

        // 2. Connect and send question
        let ws = match container_mgr.connect(&pod).await {
            Ok(ws) => ws,
            Err(e) => {
                tx.send(AgentEvent::error(&format!("Connection error: {e}"), true)).await.ok();
                return;
            }
        };

        ws.send_question(&question, &history).await;

        // 3. Relay events from agent container → gRPC stream
        let mut trace = ReasoningTrace::new();
        let mut full_answer = String::new();
        let mut step_index = 0;

        while let Some(event) = ws.next_event().await {
            match event {
                WsEvent::ToolCallStarted { tool_name, arguments_json } => {
                    trace.start_step(&tool_name, &arguments_json);
                    tx.send(AgentEvent::tool_call_started(step_index, &tool_name, &arguments_json)).await.ok();
                    step_index += 1;
                }
                WsEvent::ToolCallCompleted { tool_name, result_summary, duration_ms } => {
                    trace.complete_step(&tool_name, &result_summary, duration_ms);
                    tx.send(AgentEvent::tool_call_completed(step_index - 1, &tool_name, &result_summary, duration_ms)).await.ok();
                }
                WsEvent::TextDelta { text } => {
                    full_answer.push_str(&text);
                    tx.send(AgentEvent::partial_answer(&text)).await.ok();
                }
                WsEvent::Thinking { text } => {
                    tx.send(AgentEvent::thinking(&text)).await.ok();
                }
                WsEvent::Result { text, session_id } => {
                    full_answer = text.clone();
                    // Store assistant message
                    repos.reasoning().add_message(conv_id, "assistant", &text, Some(&trace)).await.ok();
                    // Update conversation
                    repos.reasoning().update_conversation(conv_id, &text, &trace, 0, 0).await.ok();
                    // Store SDK session ID for potential in-container resume
                    if let Some(sid) = session_id {
                        repos.reasoning().set_sdk_session_id(conv_id, &sid).await.ok();
                    }
                    // Send final answer
                    tx.send(AgentEvent::final_answer(&text, &conv_id.to_string(), /*..*/)).await.ok();
                }
                WsEvent::Error { message } => {
                    tx.send(AgentEvent::error(&message, false)).await.ok();
                }
            }
        }
    });

    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}
```

**Files created/changed:**

| File | Action |
|------|--------|
| `crates/ps-server/src/services/reasoning.rs` | **Modify** — add `ask_question()`, `list_conversations()`, `get_conversation()`, `save_insight_from_conversation()` handlers |
| `crates/ps-server/Cargo.toml` | **Modify** — add `tokio-stream`, `tokio-tungstenite` |

**Testing:**

| Level | Test |
|-------|------|
| Integration (API) | `define_api_test!` — mock container manager, call `AskQuestion`, consume stream, verify event sequence |
| Integration (API) | `ListConversations` — create 3 conversations, list, verify ordering and pagination |
| Integration (API) | `GetConversation` — complete conversation, fetch, verify messages + trace |
| Integration (API) | `SaveInsightFromConversation` — save, verify insight with `conversation_id` FK |
| Integration (API) | Auth — verify `AskQuestion` rejects unauthenticated requests |
| Integration (API) | Resume — create conversation with messages, call `AskQuestion` with `conversation_id`, verify history sent to container |

---

### Step 6: Frontend — Ask Page & Streaming UI

#### 6a: Route & Navigation

**Files changed:**

| File | Action |
|------|--------|
| `frontend/app.tsx` | **Modify** — add `const AskPage = lazy(...)`, routes for `/ask` and `/ask/:conversationId` |
| `frontend/components/app-sidebar.tsx` | **Modify** — add `Sparkles` import, add `{ title: "Ask", href: "/ask", icon: Sparkles }` to `NAV_ITEMS` |

#### 6b: Streaming Hook

```typescript
// frontend/views/ask/hooks/use-ask-question.ts

type ToolCallStep = {
  index: number;
  toolName: string;
  argumentsJson: string;
  resultSummary?: string;
  durationMs?: number;
  status: "running" | "completed" | "failed";
};

type ContainerState = "creating" | "ready" | "reconnecting" | null;

type AgentState =
  | { status: "idle" }
  | { status: "container_starting"; containerState: ContainerState; message: string }
  | { status: "streaming"; steps: ToolCallStep[]; partialAnswer: string;
      thinkingText: string; containerState: ContainerState }
  | { status: "completed"; steps: ToolCallStep[]; answer: string;
      conversationId: string; supportingData: Citation[];
      tokenUsage: TokenUsage; durationMs: number }
  | { status: "error"; message: string; retryable: boolean };
```

The hook consumes the gRPC server-streaming RPC via Connect's async iterator, updating React state on each event. The `container_status` events are shown as a status badge above the thinking panel while the container starts.

#### 6c: Page Components

```
frontend/views/ask/
├── pages/
│   └── ask-page.tsx              # Main page: header + thread + input
├── components/
│   ├── query-input.tsx           # Textarea + send/cancel button
│   ├── conversation-thread.tsx   # Scrollable message list
│   ├── user-message.tsx          # User question bubble
│   ├── agent-response.tsx        # Thinking + answer + citations + actions
│   ├── thinking-steps.tsx        # Collapsible tool-call progress feed
│   ├── thinking-step.tsx         # Single step (MCP tools, Bash, Read, Grep)
│   ├── answer-content.tsx        # Markdown renderer with citation links
│   ├── evidence-panel.tsx        # Expandable evidence & reasoning section
│   ├── container-status.tsx      # "Starting agent container..." badge
│   ├── suggested-questions.tsx   # Empty state suggested question cards
│   ├── conversation-history.tsx  # Sheet with conversation list + container status dots
│   └── save-insight-dialog.tsx   # Dialog to save answer as named insight
└── hooks/
    ├── use-ask-question.ts       # Streaming hook
    └── use-conversations.ts      # CRUD hooks for conversations
```

**Component details:**

**`thinking-step.tsx`** — Shows tool calls with context-appropriate icons:
- MCP tools (`mcp__prism__*`): Database icon (`Database`, size-3.5)
- Bash commands: Terminal icon (`Terminal`, size-3.5), shows the command in monospace
- Read/Glob/Grep: File icons, shows file paths
- Running: `Loader2 animate-spin`
- Completed: `Check` (green)

This distinction helps users understand when the agent is querying Prism data vs. running system commands.

**`container-status.tsx`** — Shows while container is starting:
```
┌─────────────────────────────────────┐
│ ⟳ Starting agent container...       │
└─────────────────────────────────────┘
```
Uses `Badge variant="secondary"` with `Loader2 animate-spin` icon. Disappears when container is ready.

**`answer-content.tsx`** — Renders Markdown via `react-markdown` + `remark-gfm`. Citation links (`[¹]`) become footnotes. Internal paths (`/people/...`, `/teams/...`) become React Router `<Link>` components. Code blocks from repo analysis are syntax-highlighted.

**Dependencies to add:**

| Package | Purpose |
|---------|---------|
| `react-markdown` | Render agent Markdown answers safely |
| `remark-gfm` | GitHub-flavoured Markdown (tables) |

**Files created:**

| File | Action |
|------|--------|
| `frontend/views/ask/pages/ask-page.tsx` | **Create** |
| `frontend/views/ask/hooks/use-ask-question.ts` | **Create** |
| `frontend/views/ask/hooks/use-conversations.ts` | **Create** |
| `frontend/views/ask/components/query-input.tsx` | **Create** |
| `frontend/views/ask/components/conversation-thread.tsx` | **Create** |
| `frontend/views/ask/components/user-message.tsx` | **Create** |
| `frontend/views/ask/components/agent-response.tsx` | **Create** |
| `frontend/views/ask/components/thinking-steps.tsx` | **Create** |
| `frontend/views/ask/components/thinking-step.tsx` | **Create** |
| `frontend/views/ask/components/answer-content.tsx` | **Create** |
| `frontend/views/ask/components/evidence-panel.tsx` | **Create** |
| `frontend/views/ask/components/container-status.tsx` | **Create** |
| `frontend/views/ask/components/suggested-questions.tsx` | **Create** |
| `frontend/views/ask/components/conversation-history.tsx` | **Create** |
| `frontend/views/ask/components/save-insight-dialog.tsx` | **Create** |

**Testing:**

| Level | Test |
|-------|------|
| UI (vitest) | `use-ask-question` — mock streaming transport, verify state transitions: idle → container_starting → streaming → completed |
| UI (vitest) | `use-ask-question` — mock error event, verify error state |
| UI (vitest) | `use-ask-question` — cancel mid-stream, verify abort fires and state resets |
| UI (vitest) | `ThinkingStep` — render MCP tool (database icon), Bash tool (terminal icon), Read tool (file icon) |
| UI (vitest) | `ContainerStatus` — render creating/ready/reconnecting states |
| UI (vitest) | `QueryInput` — type text, verify send enabled; streaming, verify stop button appears |
| UI (vitest) | `SuggestedQuestions` — render with mock teams, verify question includes team names |
| UI (vitest) | `ConversationHistory` — render with active + reaped conversations, verify status dots |
| UI (vitest) | `SaveInsightDialog` — fill title, submit, verify mutation called |
| UI (vitest) | `EvidencePanel` — render trace with MCP + Bash steps, verify display |
| UI (vitest) | `AnswerContent` — render Markdown with table, code block, and citation links |

---

### Step 7: psctl — `psctl ask` Command

```
$ psctl ask "How many repos have migrated from tox to uv?"

⏳ Starting agent container...
✅ Agent ready

🔧 mcp: list_teams() → 8 teams, 47 repos
🔧 bash: git clone --depth 1 ubuntu/kernel-snaps → Cloned (3.2 MB)
🔧 bash: rg -l "tox.ini" /workspace/kernel-snaps → 3 files
🔧 bash: rg "uv" /workspace/kernel-snaps/pyproject.toml → Found uv in 2 files
🔧 bash: git clone --depth 1 ubuntu/kernel-sru → Cloned (1.8 MB)
...

## Tox → UV Migration Status

| Team    | Repos | tox only | uv only | both | neither |
|---------|-------|----------|---------|------|---------|
| Kernel  | 12    | 4        | 5       | 2    | 1       |
| Desktop | 8     | 6        | 1       | 0    | 1       |
| ...     |       |          |         |      |         |

**Summary:** 14 of 47 repos (30%) have fully migrated to uv...

---
Model: claude-sonnet-4-6 | 12 tool calls | 18.4s
```

**Files created/changed:**

| File | Action |
|------|--------|
| `crates/psctl/src/commands/ask.rs` | **Create** — `AskCommand`, streaming consumer, terminal formatting |
| `crates/psctl/src/commands/mod.rs` | **Modify** — add `pub mod ask` |
| `crates/psctl/src/main.rs` | **Modify** — add `ask` to CLI enum |

**Testing:**

| Level | Test |
|-------|------|
| Unit | Terminal formatting — tool-call events render with icons, `--json` output is valid JSON |
| Integration | Start test server with mock container, call `psctl ask`, verify output |

---

### Step 8: K8s Deployment — Tiltfile, Service Account, Secrets

#### Service Account Token

The agent container needs a read-only API token to call ps-server. Create this during setup:

```sql
-- Bootstrap: create a service account for agent containers
INSERT INTO auth.users (username, display_name, role)
VALUES ('prism-agent-service', 'Prism Agent Service Account', 'readonly');

INSERT INTO auth.api_tokens (user_id, name, token_hash, ...)
VALUES (..., 'agent-container-token', ...);
```

The token is stored as a K8s Secret and mounted into agent Pods via environment variable.

#### K8s Resources

```yaml
# Agent container network policy
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: prism-agent-policy
spec:
  podSelector:
    matchLabels:
      app: prism-agent
  policyTypes: [Egress]
  egress:
    - to:
        - podSelector:
            matchLabels:
              app: ps-server
      ports:
        - port: 50051
    - to:
        - ipBlock:
            cidr: 0.0.0.0/0
      ports:
        - port: 443     # GitHub/Jira/Discourse HTTPS
```

#### Tiltfile

Add agent container build and deploy:

```python
# Agent service container
docker_build('prism-agent', './agent-service')

# K8s resources for agent infrastructure
k8s_yaml('k8s/agent-network-policy.yaml')
k8s_yaml('k8s/agent-service-account-secret.yaml')
```

**Files created/changed:**

| File | Action |
|------|--------|
| `k8s/agent-network-policy.yaml` | **Create** |
| `k8s/agent-service-account-secret.yaml` | **Create** (template) |
| `Tiltfile` | **Modify** — add agent container build |

**Testing:**

| Level | Test |
|-------|------|
| Integration | Deploy to dev K8s, verify agent Pod can reach ps-server gRPC, verify egress to GitHub works, verify egress to internal services blocked |

---

### Step 9: Backup/Restore Extension

**Files changed:**

| File | Action |
|------|--------|
| `crates/ps-core/src/backup.rs` | **Modify** — add conversations + messages to export/import |

**Testing:**

| Level | Test |
|-------|------|
| Integration | Create conversations with messages, export, restore to fresh DB, verify round-trip |

---

## Streaming Protocol — End-to-End Flow

```
User types question in /ask
  │
  ▼ gRPC server streaming (AskQuestion)
ps-server
  │
  ├── Create/find K8s Pod (ContainerManager)
  │     │
  │     ▼ WebSocket connect to pod-ip:8080/ws
  │   Agent Container
  │     │
  │     ├── Claude Agent SDK runs agent
  │     │     │
  │     │     ├── MCP tool call → gRPC to ps-server → DB query → result
  │     │     ├── Bash tool call → git clone / rg / grep → result
  │     │     ├── Read tool call → read file from /workspace → result
  │     │     └── Text generation → streamed tokens
  │     │
  │     ▼ WebSocket events (JSON)
  │   ps-server
  │     │
  │     ▼ maps to AgentEvent proto messages
  │   gRPC stream frames (HTTP/2)
  │     │
  │     ▼ Envoy proxy
  │   Frontend (Connect async iterator)
  │     │
  │     ▼ React state updates
  │   UI renders thinking steps + streamed answer
```

**Latency budget:**
- Container startup (cold): 5-15s (image pull cached after first run)
- Container startup (warm): 1-3s (Pod creation + readiness)
- MCP tool call (DB query): 5-50ms
- Bash tool call (git clone): 1-10s (depends on repo size)
- LLM first token: ~500ms
- LLM throughput: ~80 tokens/s

The `AgentContainerStatus` event lets the UI show "Starting agent container..." during the cold-start delay, setting user expectations.

---

## Safety & Limits

| Constraint | Value | Enforcement |
|-----------|-------|-------------|
| Max tool calls per question | 15 (via `maxTurns`) | Agent SDK config |
| Wall-clock timeout | 120 seconds | `tokio::time::timeout` in ps-server |
| Max question length | 4,000 chars | gRPC handler validation |
| Container idle timeout | 15 minutes | Agent service idle timer + K8s reaper |
| Container max lifetime | 2 hours | K8s reaper background task |
| Rate limit | 10 queries/min per user | In-memory `DashMap<UserId, RateBucket>` |
| Resource limits | 1 CPU, 2Gi RAM, 10Gi ephemeral | K8s Pod spec |
| Network egress | ps-server + HTTPS only | K8s NetworkPolicy |
| Bash safety | Block rm -rf /, docker, kubectl, etc. | PreToolUse hook |
| Max concurrent containers | 20 | ContainerManager pool limit |
| Ephemeral storage per container | 10Gi | K8s ephemeral-storage limit |

---

## Cost Estimation

The agent uses Claude (via Anthropic API key) instead of Gemini for the agentic task, since the Claude Agent SDK requires Claude. Enrichment and embeddings continue to use Gemini via Rig.

| Component | Tokens | Cost (Claude Sonnet 4.6) |
|-----------|--------|-----------------------|
| System prompt | ~800 | $0.0024 |
| Tool schemas (9 MCP + built-in) | ~1,200 | $0.0036 |
| Tool results (avg 8 calls × 300 tokens) | ~2,400 | $0.0072 |
| User question + context | ~500 | $0.0015 |
| Answer generation | ~1,500 output | $0.0225 |
| **Per-query total** | ~4,900 in / 1,500 out | **~$0.037** |

At 20 queries/day: **~$0.74/day** ($22/month). Higher than Gemini Flash but the Claude Agent SDK's built-in tooling and code analysis capabilities justify the difference. For cost-sensitive deployments, the model can be swapped to Haiku via the `ANTHROPIC_MODEL` env var on the container.

Container infrastructure cost is negligible — Pods run on existing K8s nodes and are reaped after 15 min idle.

---

## Files Summary — All Steps

### New Files (35+)

| File | Step |
|------|------|
| `migrations/XXXX_create_conversations.sql` | 1 |
| `crates/ps-server/src/container_manager.rs` | 3 |
| `crates/ps-server/src/container_manager/pod_spec.rs` | 3 |
| `agent-service/` (entire directory, ~20 files) | 4 |
| `frontend/views/ask/pages/ask-page.tsx` | 6 |
| `frontend/views/ask/hooks/use-ask-question.ts` | 6 |
| `frontend/views/ask/hooks/use-conversations.ts` | 6 |
| `frontend/views/ask/components/query-input.tsx` | 6 |
| `frontend/views/ask/components/conversation-thread.tsx` | 6 |
| `frontend/views/ask/components/user-message.tsx` | 6 |
| `frontend/views/ask/components/agent-response.tsx` | 6 |
| `frontend/views/ask/components/thinking-steps.tsx` | 6 |
| `frontend/views/ask/components/thinking-step.tsx` | 6 |
| `frontend/views/ask/components/answer-content.tsx` | 6 |
| `frontend/views/ask/components/evidence-panel.tsx` | 6 |
| `frontend/views/ask/components/container-status.tsx` | 6 |
| `frontend/views/ask/components/suggested-questions.tsx` | 6 |
| `frontend/views/ask/components/conversation-history.tsx` | 6 |
| `frontend/views/ask/components/save-insight-dialog.tsx` | 6 |
| `crates/psctl/src/commands/ask.rs` | 7 |
| `k8s/agent-network-policy.yaml` | 8 |
| `k8s/agent-service-account-secret.yaml` | 8 |

### Modified Files (12)

| File | Step |
|------|------|
| `crates/ps-core/src/repo/reasoning.rs` | 1 |
| `proto/canonical/prism/v1/reasoning.proto` | 2 |
| `crates/ps-server/src/services/reasoning.rs` | 5 |
| `crates/ps-server/Cargo.toml` | 3, 5 |
| `frontend/app.tsx` | 6 |
| `frontend/components/app-sidebar.tsx` | 6 |
| `crates/psctl/src/commands/mod.rs` | 7 |
| `crates/psctl/src/main.rs` | 7 |
| `crates/ps-core/src/backup.rs` | 9 |
| `Tiltfile` | 8 |

### Auto-generated

| File | Step |
|------|------|
| `frontend/lib/api/gen/canonical/prism/v1/reasoning_connect.ts` | 2 |
| `frontend/lib/api/gen/canonical/prism/v1/reasoning_pb.ts` | 2 |
| `crates/ps-proto/src/gen/canonical.prism.v1.rs` | 2 |

---

## Test Matrix Summary

| Category | Unit | Integration | UI |
|----------|------|-------------|-----|
| Conversations repo (CRUD, messages, container status) | — | 6 tests | — |
| Proto (lint, generate) | 2 checks | — | — |
| ContainerManager (Pod CRUD, WebSocket, reaper) | 2 tests | 2 tests | — |
| Agent service: event mapper | 4 tests | — | — |
| Agent service: bash guard | 4 tests | — | — |
| Agent service: idle timer | 2 tests | — | — |
| Agent service: MCP tools (9 tools) | 9 tests | — | — |
| Agent service: full flow | — | 1 test | — |
| gRPC AskQuestion (streaming + resume) | — | 3 tests | — |
| gRPC ListConversations | — | 1 test | — |
| gRPC GetConversation | — | 1 test | — |
| gRPC SaveInsight | — | 1 test | — |
| gRPC Auth/validation | — | 2 tests | — |
| Streaming hook (state transitions) | — | — | 3 tests |
| ThinkingStep (tool type icons) | — | — | 1 test |
| ContainerStatus component | — | — | 1 test |
| QueryInput | — | — | 2 tests |
| ConversationHistory | — | — | 1 test |
| SaveInsightDialog | — | — | 1 test |
| EvidencePanel | — | — | 1 test |
| AnswerContent (Markdown) | — | — | 1 test |
| psctl ask (formatting, JSON) | 1 test | 1 test | — |
| Backup/restore conversations | — | 1 test | — |
| K8s deployment (network policy, egress) | — | 1 test | — |
| **Total** | **24** | **20** | **11** |

---

## Implementation Order

```
Week 1: Foundation + Agent Service
  ├─ Step 1: Database migration (conversations + messages)
  ├─ Step 2: Proto definitions + buf generate
  └─ Step 4: Agent service (TypeScript container app + MCP tools)

Week 2: Container Management + Backend Wiring
  ├─ Step 3: ContainerManager (kube-rs Pod lifecycle)
  ├─ Step 5: gRPC service handlers (AskQuestion streaming, CRUD)
  └─ Step 8: K8s resources (network policy, service account, Tiltfile)

Week 3: Frontend
  ├─ Step 6a: Route + navigation
  ├─ Step 6b: Streaming hook
  └─ Step 6c: Page components (ask page, thread, thinking, answer)

Week 4: Polish & CLI
  ├─ Step 7: psctl ask command
  ├─ Step 9: Backup/restore extension
  ├─ End-to-end testing (question → container → tools → answer)
  └─ Traceability audit (every output links to source data)
```

Steps 1, 2, and 4 are independent and can proceed in parallel. Step 3 depends on having the agent container image (Step 4). Step 5 depends on Steps 1-3. Step 6 depends on Step 2 (proto types) but can scaffold immediately. Step 7 depends on Step 5.

---

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Cold container startup too slow (>10s) | Medium | Poor first-query UX | Pre-pull image on nodes. Show "Starting agent..." status. Consider warm pool of 1-2 idle containers. |
| Claude Agent SDK TypeScript not yet released or unstable | Medium | Blocked on SDK | Fall back to Python SDK. Or use the Claude API directly with manual tool loop (more code, same architecture). |
| Agent clones large repos, exhausts ephemeral storage | Low | Container OOM-killed | 10Gi ephemeral limit. Shallow clones only (`--depth 1`). System prompt instructs size-aware behaviour. |
| WebSocket connection drops mid-stream | Medium | Lost events | Reconnect logic in ContainerManager. Conversation history in DB means no data loss — just UX interruption. |
| Agent runs dangerous Bash commands | Low | Security incident | PreToolUse hook blocks patterns. NetworkPolicy restricts egress. Container runs as non-root. Resource limits cap CPU/memory. |
| High concurrent users exhaust container quota | Low | Users queued | Max 20 concurrent containers. Queue with timeout for excess. Show "Agent busy, please wait..." |
| Anthropic API key costs higher than expected | Medium | Budget overrun | Track via CostTracker (existing). Daily budget cap applies to agentic task type. Switch to Haiku for lower cost. |
| Session resume loses context nuance | Medium | Agent seems forgetful | Store full message history (not just summaries). Include all tool call results in resume context. System prompt acknowledges resumed sessions. |

---

## Decision Record

| Attribute | Value |
|-----------|-------|
| Decision | Use Claude Agent SDK in ephemeral K8s containers for the agentic query interface |
| Date | 2026-03-23 |
| Status | Proposed |
| Drivers | System tool access (git, rg, grep), sandboxed execution, Claude Agent SDK's built-in tooling and code analysis, container-per-session isolation |
| Alternatives considered | In-process Rig agent (simpler but no system tools), long-lived agent pool (more complex lifecycle), Docker-in-Docker for repo analysis (security concerns) |
| Risks | Container cold-start latency (mitigated by pre-pulling + status UX), SDK stability (mitigated by pinned version + Python fallback) |

---

## Exit Criteria

- [ ] `reasoning.conversations` + `conversation_messages` tables created
- [ ] Agent container image builds and runs (Ubuntu + git + rg + tokei + Node.js + Agent SDK)
- [ ] 9 MCP tools implemented, each calling ps-server gRPC APIs
- [ ] ContainerManager creates, connects, and reaps K8s Pods
- [ ] `AskQuestion` streaming RPC works end-to-end (question → container → tools → answer)
- [ ] Frontend `/ask` page shows container status, real-time tool-call progress, and streamed answer
- [ ] Both MCP tool calls and system tool calls (Bash, Read, Grep) visible in thinking panel
- [ ] Reasoning trace stored and viewable in "Evidence & Reasoning" panel
- [ ] Citations in answers link to real source data (people, teams, contributions)
- [ ] Multi-turn conversations work (follow-up questions within same container)
- [ ] Session resume after container reap works (new container + history context)
- [ ] Conversation history persisted and browsable
- [ ] "Save as Insight" saves answer to `reasoning.insights` with `conversation_id` FK
- [ ] `psctl ask` streams output to terminal with `--json` support
- [ ] Idle containers reaped after 15 min, max lifetime 2 hours
- [ ] Bash safety hook blocks dangerous commands
- [ ] Network policy restricts container egress
- [ ] Backup/restore includes conversations
- [ ] `prek run -av` passes with zero warnings
