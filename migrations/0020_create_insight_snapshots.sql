-- Periodically-computed insight snapshots per team, aggregating enrichment data.
-- Follows the same pattern as metrics.team_snapshots — one row per team per
-- period, idempotently upserted by the InsightsHandler.

CREATE TABLE reasoning.insight_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL REFERENCES org.teams(id) ON DELETE CASCADE,
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    period_type TEXT NOT NULL,  -- 'week', 'month', 'quarter'

    -- Review quality
    avg_review_depth REAL,
    review_count INTEGER NOT NULL DEFAULT 0,
    rubber_stamp_pct REAL,
    deep_review_pct REAL,
    depth_distribution INTEGER[] NOT NULL DEFAULT '{}',  -- [score1_count, ..., score5_count]

    -- Sentiment
    constructive_count INTEGER NOT NULL DEFAULT 0,
    neutral_count INTEGER NOT NULL DEFAULT 0,
    critical_count INTEGER NOT NULL DEFAULT 0,
    hostile_count INTEGER NOT NULL DEFAULT 0,

    -- PR significance
    significant_count INTEGER NOT NULL DEFAULT 0,
    notable_count INTEGER NOT NULL DEFAULT 0,
    routine_count INTEGER NOT NULL DEFAULT 0,

    -- Depth × Significance cross-reference
    avg_depth_on_significant REAL,
    avg_depth_on_notable REAL,
    avg_depth_on_routine REAL,

    -- Coverage at snapshot time
    enrichment_coverage JSONB NOT NULL DEFAULT '{}',

    -- Overflow / extensible data (top reviewers, Discourse categories, etc.)
    raw_insights JSONB NOT NULL DEFAULT '{}',

    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (team_id, period_start, period_type)
);

-- Traceability: link each snapshot back to the enrichments that produced it.
CREATE TABLE reasoning.insight_snapshot_sources (
    snapshot_id UUID NOT NULL REFERENCES reasoning.insight_snapshots(id) ON DELETE CASCADE,
    enrichment_id UUID NOT NULL REFERENCES reasoning.enrichments(id) ON DELETE CASCADE,
    PRIMARY KEY (snapshot_id, enrichment_id)
);

-- Query snapshots by team for trend display.
CREATE INDEX idx_insight_snapshots_team
    ON reasoning.insight_snapshots(team_id, period_type, period_start DESC);
