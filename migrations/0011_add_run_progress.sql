-- Add structured progress tracking to ingestion runs.
-- Stores JSON with phase, repo progress, counters, rate limit info etc.
-- Updated after every batch during the fetch-store loop.
ALTER TABLE activity.ingestion_runs
    ADD COLUMN IF NOT EXISTS progress JSONB;
