-- AI-generated enrichments (traceable back to source content).
-- Each enrichment ties to a single contribution and carries full provenance:
-- which model, what input hash, confidence, and structured result.
CREATE TABLE reasoning.enrichments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id UUID NOT NULL REFERENCES activity.contributions(id) ON DELETE CASCADE,
    enrichment_type TEXT NOT NULL,       -- 'review_depth', 'sentiment', 'significance', 'topic'
    value JSONB NOT NULL,               -- structured result (score, label, rationale)
    model_name TEXT NOT NULL,            -- e.g. 'gemini-3.1-flash-lite'
    confidence REAL,                     -- model's self-reported confidence 0.0–1.0
    input_hash TEXT,                     -- SHA-256 of input text for reproducibility
    input_preview TEXT,                  -- first ~500 chars of input for quick audit
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (contribution_id, enrichment_type)
);

-- Query enrichments by contribution (the primary access pattern).
CREATE INDEX idx_enrichments_contribution
    ON reasoning.enrichments(contribution_id);

-- Find un-enriched contributions efficiently: scheduler queries contributions
-- that lack a row in this table for a given enrichment_type.
CREATE INDEX idx_enrichments_type_created
    ON reasoning.enrichments(enrichment_type, created_at DESC);
