ALTER TABLE reasoning.conversation_messages
  ADD COLUMN attached_files TEXT[] NOT NULL DEFAULT '{}';
