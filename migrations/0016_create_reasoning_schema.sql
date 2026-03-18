-- Phase 3: Reasoning schema for AI capabilities.

CREATE SCHEMA IF NOT EXISTS reasoning;

-- API usage tracking for cost management.
CREATE TABLE reasoning.api_usage (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    task_type TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_api_usage_daily
    ON reasoning.api_usage(task_type, created_at DESC);

CREATE INDEX idx_api_usage_provider
    ON reasoning.api_usage(provider, created_at DESC);
