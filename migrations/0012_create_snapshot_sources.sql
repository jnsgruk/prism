-- Traceability link table: maps metric snapshots to the contributions
-- that fed into their computation.  Every computed metric can be audited
-- back to its source data via this table.
CREATE TABLE metrics.snapshot_sources (
    snapshot_id UUID NOT NULL REFERENCES metrics.team_snapshots(id) ON DELETE CASCADE,
    contribution_id UUID NOT NULL REFERENCES activity.contributions(id) ON DELETE CASCADE,
    PRIMARY KEY (snapshot_id, contribution_id)
);

CREATE INDEX idx_snapshot_sources_contribution
    ON metrics.snapshot_sources (contribution_id);
