-- A durable submission intent is reserved before any Restate request is sent.
ALTER TABLE activity.pipelines
    ADD COLUMN request_snapshot JSONB NOT NULL DEFAULT '{}',
    ADD COLUMN requested_by UUID,
    ADD COLUMN requested_by_username TEXT,
    ADD COLUMN cancellation_requested BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN dispatch_acknowledged BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN dispatch_after TIMESTAMPTZ NOT NULL DEFAULT now();

-- Existing running workflows remain admitted. Multiple existing active rows must
-- be repaired explicitly before this migration can succeed.
CREATE UNIQUE INDEX pipelines_one_active
    ON activity.pipelines ((true))
    WHERE status IN ('pending', 'running', 'cancelling');

CREATE TABLE activity.pipeline_invocations (
    pipeline_id UUID NOT NULL REFERENCES activity.pipelines(id) ON DELETE CASCADE,
    invocation_id TEXT NOT NULL,
    parent_invocation_id TEXT,
    kind TEXT NOT NULL,
    run_id UUID REFERENCES activity.ingestion_runs(id),
    registered_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (pipeline_id, invocation_id)
);
