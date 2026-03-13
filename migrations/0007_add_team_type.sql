-- Add team_type enum to distinguish hierarchy levels: org, group, team, squad
CREATE TYPE org.team_type AS ENUM ('org', 'group', 'team', 'squad');

ALTER TABLE org.teams ADD COLUMN team_type org.team_type NOT NULL DEFAULT 'team';

-- Backfill existing top-level teams (no parent) as 'group' type,
-- since the current directory import creates flat group-level entries.
UPDATE org.teams SET team_type = 'group' WHERE parent_team_id IS NULL;
