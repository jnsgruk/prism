-- Per-person period snapshots for the individual profile view.
-- Stores aggregated activity summaries and peer comparison data
-- as JSONB, keyed by person + period.
CREATE TABLE metrics.individual_profiles (
    id UUID PRIMARY KEY,
    person_id UUID NOT NULL REFERENCES org.people(id) ON DELETE CASCADE,
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    period_type TEXT NOT NULL,
    -- Aggregated activity by platform: {"github": {...}, "jira": {...}, ...}
    activity_summary JSONB NOT NULL DEFAULT '{}',
    -- Peer comparison context: percentiles relative to same-level peers
    peer_comparison JSONB NOT NULL DEFAULT '{}',
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (person_id, period_start, period_type)
);

CREATE INDEX idx_individual_profiles_person
    ON metrics.individual_profiles (person_id, period_start DESC);
