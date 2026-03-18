-- Enrichment queue: transient content captured during ingestion for AI enrichment.
-- Content is deleted after all applicable enrichment types are satisfied.

CREATE TABLE reasoning.enrichment_queue (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id UUID NOT NULL UNIQUE
                    REFERENCES activity.contributions(id) ON DELETE CASCADE,
    content         JSONB NOT NULL,
    content_hash    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- FK lookups on contribution delete
CREATE INDEX idx_eq_contribution_id
    ON reasoning.enrichment_queue(contribution_id);
