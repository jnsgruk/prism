use crate::infra::run_lifecycle::{
    complete_owned_run, complete_owned_run_with_warnings, complete_run, complete_run_with_warnings,
    create_run, fail_owned_run, fail_run,
};
use restate_sdk::prelude::*;

/// Create an ingestion run record inside a Restate `ctx.run()` closure.
pub(super) async fn create_ingestion_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    source_name: &str,
    handler_name: &str,
    method: &str,
) -> Result<uuid::Uuid, TerminalError> {
    create_run!(ctx, repos, source_name, handler_name, method)
}

pub(super) struct IngestionRun<'a> {
    pub id: uuid::Uuid,
    pub source_name: &'a str,
    pub owned: bool,
}

/// Mark a run as complete inside a Restate `ctx.run()` closure.
pub(super) async fn complete_ingestion_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run: IngestionRun<'_>,
    items_collected: i32,
) {
    let IngestionRun {
        id: run_id,
        source_name,
        owned,
    } = run;
    if owned {
        complete_owned_run!(ctx, repos, run_id, source_name, items_collected);
        return;
    }
    complete_run!(ctx, repos, run_id, source_name, items_collected);
}

pub(super) struct RunWarnings<'a> {
    pub items_collected: i32,
    pub error_summary: &'a str,
    pub metadata: serde_json::Value,
    pub owned: bool,
}

/// Mark a run as completed with warnings inside a Restate `ctx.run()` closure.
pub(super) async fn complete_ingestion_run_with_warnings(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run_id: uuid::Uuid,
    source_name: &str,
    warnings: RunWarnings<'_>,
) {
    let RunWarnings {
        items_collected,
        error_summary,
        metadata,
        owned,
    } = warnings;
    if owned {
        complete_owned_run_with_warnings!(
            ctx,
            repos,
            run_id,
            source_name,
            items_collected,
            error_summary,
            metadata
        );
        return;
    }
    complete_run_with_warnings!(
        ctx,
        repos,
        run_id,
        source_name,
        items_collected,
        error_summary,
        metadata
    );
}

/// Mark a run as failed inside a Restate `ctx.run()` closure.
pub(super) async fn fail_ingestion_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run: IngestionRun<'_>,
    error_msg: &str,
) {
    let IngestionRun {
        id: run_id,
        source_name,
        owned,
    } = run;
    if owned {
        fail_owned_run!(ctx, repos, run_id, source_name, error_msg);
        return;
    }
    fail_run!(ctx, repos, run_id, source_name, error_msg);
}
