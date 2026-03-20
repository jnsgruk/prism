CREATE TABLE reasoning.embeddings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id UUID NOT NULL REFERENCES activity.contributions(id) ON DELETE CASCADE,
    embedding vector(1024),
    model_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (contribution_id, model_name)
);

CREATE INDEX idx_embeddings_vector ON reasoning.embeddings
    USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);
