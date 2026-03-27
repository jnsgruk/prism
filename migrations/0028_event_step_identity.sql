-- Add server-assigned step identity and display ordering to conversation events.
-- step_id: stable identity for the logical step (e.g. "think-0-0", "tool-abc123")
-- step_seq: monotonically increasing display order within a conversation
ALTER TABLE reasoning.conversation_events
  ADD COLUMN step_id TEXT,
  ADD COLUMN step_seq INT;

-- Optimise resume queries that filter by conversation + sort by step_seq.
CREATE INDEX idx_conversation_events_step_seq
  ON reasoning.conversation_events (conversation_id, step_seq)
  WHERE step_seq IS NOT NULL;
