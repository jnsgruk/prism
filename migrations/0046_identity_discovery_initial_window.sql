-- A failed first traversal must retain its original lower bound across new runs.
ALTER TABLE activity.identity_discovery_coverage
    ADD COLUMN initial_since TIMESTAMPTZ;
UPDATE activity.identity_discovery_coverage SET initial_since = covered_through;
ALTER TABLE activity.identity_discovery_coverage
    ALTER COLUMN initial_since SET NOT NULL,
    ALTER COLUMN covered_through DROP NOT NULL;
