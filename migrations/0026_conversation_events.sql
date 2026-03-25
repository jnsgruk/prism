-- Ephemeral event log for streaming agentic query events (Plan 57).
-- Events are written by the Restate handler and polled by ps-server.
-- Rows are deleted after the query completes (or after a TTL).

CREATE TABLE reasoning.conversation_events (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    conversation_id UUID NOT NULL REFERENCES reasoning.conversations(id) ON DELETE CASCADE,
    event_type  TEXT NOT NULL,  -- 'container_status', 'tool_call_started', 'tool_call_completed',
                               -- 'partial_answer', 'thinking', 'artifact_uploaded', 'final_answer', 'error'
    payload     JSONB NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conv_events_poll ON reasoning.conversation_events (conversation_id, id);

-- Query lifecycle status for the poll-and-stream adapter.
ALTER TABLE reasoning.conversations
    ADD COLUMN query_status TEXT NOT NULL DEFAULT 'idle';
    -- Values: 'idle', 'pending', 'running', 'completed', 'failed', 'cancelled'
