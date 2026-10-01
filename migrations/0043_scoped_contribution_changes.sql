-- Write provenance survives ordinary upserts of contribution metadata.
CREATE TABLE activity.contribution_changes (
    id UUID PRIMARY KEY,
    pipeline_id UUID NOT NULL REFERENCES activity.pipelines(id) ON DELETE CASCADE,
    run_id UUID REFERENCES activity.ingestion_runs(id) ON DELETE SET NULL,
    source_id UUID NOT NULL,
    contribution_id UUID NOT NULL REFERENCES activity.contributions(id) ON DELETE CASCADE,
    previous_person_id UUID,
    current_person_id UUID NOT NULL,
    previous_created_at TIMESTAMPTZ,
    current_created_at TIMESTAMPTZ NOT NULL,
    previous_input JSONB,
    current_input JSONB NOT NULL,
    affected_periods JSONB NOT NULL,
    input_hash TEXT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (pipeline_id, contribution_id, input_hash)
);

CREATE INDEX contribution_changes_pipeline ON activity.contribution_changes(pipeline_id);
