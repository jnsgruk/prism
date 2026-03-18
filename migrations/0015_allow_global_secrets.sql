-- Allow global secrets (NULL source_id) for AI provider API keys.
-- Previously source_id was NOT NULL with a FK to source_configs.

-- Drop the existing unique constraint and FK
ALTER TABLE config.secrets
    DROP CONSTRAINT IF EXISTS secrets_source_id_secret_key_key;

ALTER TABLE config.secrets
    ALTER COLUMN source_id DROP NOT NULL;

ALTER TABLE config.secrets
    DROP CONSTRAINT IF EXISTS secrets_source_id_fkey;

-- Re-add FK only for non-null source_id rows
ALTER TABLE config.secrets
    ADD CONSTRAINT secrets_source_id_fkey
    FOREIGN KEY (source_id) REFERENCES config.source_configs(id) ON DELETE CASCADE
    NOT VALID;

-- Unique per source (existing behaviour)
CREATE UNIQUE INDEX IF NOT EXISTS uq_secrets_source_key
    ON config.secrets(source_id, secret_key)
    WHERE source_id IS NOT NULL;

-- Unique per global secret key (new)
CREATE UNIQUE INDEX IF NOT EXISTS uq_secrets_global_key
    ON config.secrets(secret_key)
    WHERE source_id IS NULL;
