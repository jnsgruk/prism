-- Resetting run history must preserve exact invocation ownership for cancellation.
ALTER TABLE activity.pipeline_invocations
    DROP CONSTRAINT pipeline_invocations_run_id_fkey,
    ADD CONSTRAINT pipeline_invocations_run_id_fkey
        FOREIGN KEY (run_id) REFERENCES activity.ingestion_runs(id) ON DELETE SET NULL;
