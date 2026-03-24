-- Conversation tables for the agentic query interface (Plan 56).
-- Each conversation maps to one ephemeral agent container (K8s Pod).

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

CREATE INDEX idx_conversations_user
    ON reasoning.conversations(user_id, created_at DESC);

CREATE INDEX idx_conversations_container
    ON reasoning.conversations(container_pod_name)
    WHERE container_status = 'active';

-- Individual turns within a conversation.
CREATE TABLE reasoning.conversation_messages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES reasoning.conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL,             -- 'user' | 'assistant'
    content TEXT NOT NULL,
    reasoning_trace JSONB,         -- tool calls for assistant messages
    supporting_data JSONB,         -- citations for assistant messages
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conv_messages
    ON reasoning.conversation_messages(conversation_id, created_at);

-- Artifacts generated during conversations (stored in S3/RustFS).
CREATE TABLE reasoning.conversation_artifacts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES reasoning.conversations(id) ON DELETE CASCADE,
    message_id UUID REFERENCES reasoning.conversation_messages(id),
    artifact_key TEXT NOT NULL,        -- S3 key: conversations/{conv_id}/{filename}
    display_name TEXT NOT NULL,        -- Human-readable filename
    content_type TEXT,                 -- MIME type (text/csv, application/json, etc.)
    size_bytes BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conv_artifacts
    ON reasoning.conversation_artifacts(conversation_id);

-- Link insight snapshots back to conversations that produced them.
ALTER TABLE reasoning.insight_snapshots
    ADD COLUMN conversation_id UUID REFERENCES reasoning.conversations(id);
