-- Link ingestion runs to the pipeline that triggered them.
ALTER TABLE activity.ingestion_runs
    ADD COLUMN pipeline_id UUID REFERENCES activity.pipelines(id);

CREATE INDEX idx_ingestion_runs_pipeline ON activity.ingestion_runs (pipeline_id)
    WHERE pipeline_id IS NOT NULL;

-- Backfill: match existing runs to pipelines by time overlap.
-- A run belongs to a pipeline if it started between the pipeline's start and
-- (completed_at or now for running pipelines).
UPDATE activity.ingestion_runs r
SET pipeline_id = p.id
FROM activity.pipelines p
WHERE r.pipeline_id IS NULL
  AND r.started_at >= p.started_at
  AND r.started_at <= COALESCE(p.completed_at, now());
