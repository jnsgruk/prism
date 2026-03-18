-- AI model catalogue: cached list of models available from each provider.
CREATE TABLE config.ai_models (
    id              TEXT            NOT NULL,
    provider        TEXT            NOT NULL,
    display_name    TEXT            NOT NULL,
    description     TEXT,
    context_length  INTEGER,
    input_price     DOUBLE PRECISION,
    output_price    DOUBLE PRECISION,
    capabilities    TEXT[]          NOT NULL DEFAULT '{}',
    updated_at      TIMESTAMPTZ     NOT NULL DEFAULT now(),

    PRIMARY KEY (provider, id)
);

CREATE INDEX idx_ai_models_provider ON config.ai_models (provider);
