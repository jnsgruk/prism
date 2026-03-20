CREATE TABLE reasoning.embedding_queue (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id UUID NOT NULL REFERENCES activity.contributions(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (contribution_id)
);

CREATE INDEX idx_embedding_queue_created ON reasoning.embedding_queue(created_at);
