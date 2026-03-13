-- Add people management columns: active flag and import tracking.
ALTER TABLE org.people ADD COLUMN IF NOT EXISTS active BOOLEAN NOT NULL DEFAULT true;
ALTER TABLE org.people ADD COLUMN IF NOT EXISTS last_import_at TIMESTAMPTZ;
