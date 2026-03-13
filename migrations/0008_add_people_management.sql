-- Add people management columns: active flag and import tracking.
ALTER TABLE org.people ADD COLUMN active BOOLEAN NOT NULL DEFAULT true;
ALTER TABLE org.people ADD COLUMN last_import_at TIMESTAMPTZ;
