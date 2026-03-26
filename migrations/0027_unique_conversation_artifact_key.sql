-- Prevent duplicate artifacts within the same conversation (same S3 key).
CREATE UNIQUE INDEX IF NOT EXISTS uq_conv_artifacts_key
    ON reasoning.conversation_artifacts(conversation_id, artifact_key);
