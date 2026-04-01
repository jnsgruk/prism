-- Pipeline orchestration records
CREATE TABLE activity.pipelines (
    id                    UUID PRIMARY KEY,
    status                TEXT NOT NULL DEFAULT 'running',
    current_stage         TEXT,
    started_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at          TIMESTAMPTZ,
    stages                JSONB NOT NULL DEFAULT '{}',
    current_invocation_id TEXT,
    error                 TEXT
);

CREATE INDEX idx_pipelines_status ON activity.pipelines (status, started_at DESC);
