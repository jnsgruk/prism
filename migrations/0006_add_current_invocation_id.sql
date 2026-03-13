-- Stores the Restate invocation ID for the currently running ingestion,
-- allowing cancellation via the Restate admin API.
ALTER TABLE activity.ingestion_watermarks
ADD COLUMN current_invocation_id TEXT;
