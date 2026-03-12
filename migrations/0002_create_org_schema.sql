CREATE SCHEMA IF NOT EXISTS org;

CREATE TABLE org.people (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT,
    level TEXT,
    directory_id TEXT UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE org.teams (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    org_name TEXT NOT NULL,
    parent_team_id UUID REFERENCES org.teams(id),
    lead_id UUID REFERENCES org.people(id),
    github_team_slug TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_teams_parent ON org.teams (parent_team_id) WHERE parent_team_id IS NOT NULL;

CREATE TABLE org.team_memberships (
    id UUID PRIMARY KEY,
    person_id UUID NOT NULL REFERENCES org.people(id),
    team_id UUID NOT NULL REFERENCES org.teams(id),
    start_date DATE NOT NULL,
    end_date DATE,
    UNIQUE (person_id, team_id, start_date)
);

CREATE TABLE org.platform_identities (
    id UUID PRIMARY KEY,
    person_id UUID NOT NULL REFERENCES org.people(id),
    platform TEXT NOT NULL,
    platform_username TEXT NOT NULL,
    platform_user_id TEXT,
    UNIQUE (platform, platform_username)
);

CREATE INDEX idx_identity_lookup ON org.platform_identities (platform, platform_username);

CREATE TABLE org.repositories (
    id UUID PRIMARY KEY,
    github_org TEXT NOT NULL,
    github_repo TEXT NOT NULL,
    default_branch TEXT DEFAULT 'main',
    primary_language TEXT,
    team_id UUID REFERENCES org.teams(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (github_org, github_repo)
);
