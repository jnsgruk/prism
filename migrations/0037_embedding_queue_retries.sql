-- Keep failed embedding work visible without hot-looping on the same records.
ALTER TABLE reasoning.embedding_queue
    ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN last_error TEXT,
    ADD COLUMN failed_at TIMESTAMPTZ;

CREATE INDEX idx_embedding_queue_ready
    ON reasoning.embedding_queue (next_attempt_at, created_at)
    WHERE failed_at IS NULL;
