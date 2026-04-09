ALTER TABLE reasoning.conversation_messages
  ADD COLUMN mentions JSONB NOT NULL DEFAULT '[]';
