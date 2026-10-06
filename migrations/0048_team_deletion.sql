-- Team deletion removes derived snapshots and clears repository assignments.
-- Memberships stay restrictive; OrgRepo deletes only ended rows after validation.
ALTER TABLE metrics.team_snapshots
    DROP CONSTRAINT team_snapshots_team_id_fkey,
    ADD CONSTRAINT team_snapshots_team_id_fkey
        FOREIGN KEY (team_id) REFERENCES org.teams(id) ON DELETE CASCADE;

ALTER TABLE org.repositories
    DROP CONSTRAINT repositories_team_id_fkey,
    ADD CONSTRAINT repositories_team_id_fkey
        FOREIGN KEY (team_id) REFERENCES org.teams(id) ON DELETE SET NULL;
