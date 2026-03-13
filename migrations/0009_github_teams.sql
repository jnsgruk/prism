-- Discovered GitHub teams from the GitHub API (separate from org.teams which are Prism teams)
CREATE TABLE org.github_teams (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id       UUID NOT NULL REFERENCES config.source_configs(id) ON DELETE CASCADE,
    github_org      TEXT NOT NULL,
    github_team_id  BIGINT NOT NULL,
    slug            TEXT NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT,
    last_synced_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_id, github_org, slug)
);

-- Members of discovered GitHub teams (by GitHub username)
CREATE TABLE org.github_team_members (
    github_team_id  UUID NOT NULL REFERENCES org.github_teams(id) ON DELETE CASCADE,
    github_username TEXT NOT NULL,
    last_synced_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (github_team_id, github_username)
);

-- Repos associated with discovered GitHub teams
CREATE TABLE org.github_team_repos (
    github_team_id  UUID NOT NULL REFERENCES org.github_teams(id) ON DELETE CASCADE,
    github_org      TEXT NOT NULL,
    github_repo     TEXT NOT NULL,
    last_synced_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (github_team_id, github_org, github_repo)
);

-- Many-to-many mapping: which Prism teams map to which GitHub teams
CREATE TABLE org.team_github_team_mappings (
    team_id         UUID NOT NULL REFERENCES org.teams(id) ON DELETE CASCADE,
    github_team_id  UUID NOT NULL REFERENCES org.github_teams(id) ON DELETE CASCADE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (team_id, github_team_id)
);

-- Track dismissed auto-suggestions so they don't resurface
CREATE TABLE org.dismissed_github_team_suggestions (
    team_id         UUID NOT NULL REFERENCES org.teams(id) ON DELETE CASCADE,
    github_team_id  UUID NOT NULL REFERENCES org.github_teams(id) ON DELETE CASCADE,
    dismissed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (team_id, github_team_id)
);

-- Migrate any existing github_team_slug values into the mapping table.
-- This requires a matching github_teams row to exist, which won't happen
-- until the first team sync runs. We drop the column now; users with
-- existing slugs will need to re-map after their first team sync.
ALTER TABLE org.teams DROP COLUMN IF EXISTS github_team_slug;
