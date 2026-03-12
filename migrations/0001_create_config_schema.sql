CREATE SCHEMA IF NOT EXISTS config;

CREATE TABLE config.source_configs (
    id UUID PRIMARY KEY,
    source_type TEXT NOT NULL,
    name TEXT NOT NULL UNIQUE,
    enabled BOOLEAN NOT NULL DEFAULT true,
    settings JSONB NOT NULL DEFAULT '{}',
    schedule_cron TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE config.secrets (
    id UUID PRIMARY KEY,
    source_id UUID NOT NULL REFERENCES config.source_configs(id) ON DELETE CASCADE,
    secret_key TEXT NOT NULL,
    encrypted_value BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_id, secret_key)
);

CREATE TABLE config.global_settings (
    key TEXT PRIMARY KEY,
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
