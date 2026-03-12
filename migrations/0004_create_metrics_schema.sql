CREATE SCHEMA IF NOT EXISTS metrics;

CREATE TABLE metrics.team_snapshots (
    id UUID PRIMARY KEY,
    team_id UUID NOT NULL REFERENCES org.teams(id),
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    period_type TEXT NOT NULL,
    throughput INTEGER,
    avg_review_turnaround_hours REAL,
    deployment_frequency REAL,
    lead_time_hours REAL,
    change_failure_rate REAL,
    mttr_hours REAL,
    avg_cycle_time_hours REAL,
    wip_avg REAL,
    flow_efficiency REAL,
    avg_review_depth REAL,
    raw_metrics JSONB DEFAULT '{}',
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (team_id, period_start, period_type)
);
