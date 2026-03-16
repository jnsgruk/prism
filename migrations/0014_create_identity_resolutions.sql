-- Identity resolution tracking: records whether each known person has been
-- resolved on each external platform (Discourse, Jira, etc.).
--
-- Resolution links directory-imported people to their platform accounts.
-- Statuses: pending (not yet attempted), resolved (matched), unresolved
-- (attempted but no match), manual (admin override).

CREATE TABLE org.identity_resolutions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    person_id    UUID NOT NULL REFERENCES org.people(id) ON DELETE CASCADE,
    platform     TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending',
    resolved_at  TIMESTAMPTZ,
    attempted_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (person_id, platform)
);

CREATE INDEX idx_identity_resolutions_pending
    ON org.identity_resolutions (platform)
    WHERE status = 'pending';
