-- Each change carries independently retryable raw and post-enrichment work.
CREATE TABLE activity.snapshot_invalidations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    change_id UUID NOT NULL REFERENCES activity.contribution_changes(id) ON DELETE CASCADE,
    period_type TEXT NOT NULL CHECK (period_type IN ('week', 'month', 'quarter')),
    period_start DATE NOT NULL,
    metrics_refreshed_at TIMESTAMPTZ,
    insights_refreshed_at TIMESTAMPTZ,
    UNIQUE (change_id, period_type, period_start)
);

CREATE INDEX snapshot_invalidations_pending_metrics
    ON activity.snapshot_invalidations(period_start, period_type)
    WHERE metrics_refreshed_at IS NULL;
CREATE INDEX snapshot_invalidations_pending_insights
    ON activity.snapshot_invalidations(period_start, period_type)
    WHERE insights_refreshed_at IS NULL;

-- Recover changes committed before this migration as well.
INSERT INTO activity.snapshot_invalidations (change_id, period_type, period_start)
SELECT c.id, period->>'period_type', (period->>'period_start')::date
FROM activity.contribution_changes c
CROSS JOIN LATERAL jsonb_array_elements(c.affected_periods) period;
