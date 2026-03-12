CREATE SCHEMA IF NOT EXISTS activity;

CREATE TABLE activity.contributions (
    id UUID PRIMARY KEY,
    person_id UUID REFERENCES org.people(id),
    platform TEXT NOT NULL,
    contribution_type TEXT NOT NULL,
    platform_id TEXT NOT NULL,
    title TEXT,
    url TEXT,
    state TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ,
    closed_at TIMESTAMPTZ,
    metrics JSONB NOT NULL DEFAULT '{}',
    metadata JSONB NOT NULL DEFAULT '{}',
    content TEXT,
    state_history JSONB DEFAULT '[]',
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (platform, platform_id)
);

CREATE INDEX idx_contributions_person ON activity.contributions (person_id, created_at DESC);
CREATE INDEX idx_contributions_platform ON activity.contributions (platform, contribution_type, created_at DESC);
CREATE INDEX idx_contributions_state ON activity.contributions (state) WHERE state IN ('open', 'in_progress');
CREATE INDEX idx_contributions_created ON activity.contributions (created_at DESC);
CREATE INDEX idx_contributions_metrics ON activity.contributions USING GIN (metrics);

CREATE TABLE activity.ingestion_watermarks (
    source_name TEXT PRIMARY KEY,
    watermark_value TEXT NOT NULL,
    last_successful_run TIMESTAMPTZ,
    last_attempt TIMESTAMPTZ,
    last_error TEXT,
    items_collected_last_run INTEGER DEFAULT 0
);

CREATE TABLE activity.ingestion_runs (
    id UUID PRIMARY KEY,
    source_name TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'running',
    items_collected INTEGER DEFAULT 0,
    error_message TEXT,
    rate_limit_waits_seconds INTEGER DEFAULT 0,
    metadata JSONB DEFAULT '{}'
);

CREATE INDEX idx_ingestion_runs_source ON activity.ingestion_runs (source_name, started_at DESC);

CREATE TABLE activity.etag_cache (
    source_name TEXT NOT NULL,
    endpoint_url TEXT NOT NULL,
    etag TEXT NOT NULL,
    last_used TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (source_name, endpoint_url)
);
