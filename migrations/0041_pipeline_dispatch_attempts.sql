-- Preserve uncertain submission state across retries and deployment changes.
ALTER TABLE activity.pipelines
    ADD COLUMN dispatch_attempts INTEGER NOT NULL DEFAULT 0;
