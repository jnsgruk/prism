-- Add handler tracking columns to ingestion_runs so we can track runs from
-- all Restate handlers (not just GithubIngestionHandler).
ALTER TABLE activity.ingestion_runs
    ADD COLUMN handler_name TEXT NOT NULL DEFAULT 'GithubIngestionHandler',
    ADD COLUMN handler_method TEXT NOT NULL DEFAULT 'run_ingestion';

-- Index for listing runs by handler
CREATE INDEX idx_ingestion_runs_handler ON activity.ingestion_runs (handler_name, started_at DESC);
