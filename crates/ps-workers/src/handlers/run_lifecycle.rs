//! Shared run lifecycle wrappers for all Restate handlers.
//!
//! Uses macros to generate `ctx.run()` wrappers that work with any
//! Restate context type (`Context`, `ObjectContext`, etc.) without
//! running into async-lifetime issues with generic trait bounds.

/// Create a run record inside a journaled `ctx.run()` closure.
///
/// `Uuid::now_v7()` is called inside the closure so Restate retries
/// reuse the journaled ID, preventing duplicate/orphaned run records.
///
/// Usage: `create_run!(ctx, repos, "source", "Handler", "method")`
macro_rules! create_run {
    ($ctx:expr, $repos:expr, $source:expr, $handler:expr, $method:expr) => {{
        let repos = $repos.clone();
        let source = $source.to_string();
        let handler = $handler.to_string();
        let method = $method.to_string();
        $ctx.run(move || {
            let repos = repos.clone();
            let source = source.clone();
            let handler = handler.clone();
            let method = method.clone();
            async move {
                let id = ::uuid::Uuid::now_v7();
                repos
                    .activity
                    .create_run(id, &source, &handler, &method)
                    .await
                    .map_err(|e| {
                        ::restate_sdk::prelude::TerminalError::new(format!("db error: {e}"))
                    })?;
                Ok(::restate_sdk::prelude::Json::from(id.to_string()))
            }
        })
        .name("create_run")
        .await?
        .into_inner()
        .parse::<::uuid::Uuid>()
        .map_err(|e| ::restate_sdk::prelude::TerminalError::new(format!("invalid run_id: {e}")))
    }};
}

/// Mark a run as complete inside a journaled `ctx.run()` closure.
///
/// Also clears the current invocation ID for the source. Logs errors
/// rather than propagating, since run completion failure should not
/// abort the handler (the work is already done).
///
/// Usage: `complete_run!(ctx, repos, run_id, "source", items)`
macro_rules! complete_run {
    ($ctx:expr, $repos:expr, $run_id:expr, $source_name:expr, $items:expr) => {{
        let repos = $repos.clone();
        let sn = $source_name.to_string();
        let run_id = $run_id;
        let items = $items;
        let result = $ctx
            .run(move || {
                let repos = repos.clone();
                let sn = sn.clone();
                async move {
                    repos
                        .activity
                        .complete_run(run_id, items)
                        .await
                        .map_err(|e| {
                            ::restate_sdk::prelude::TerminalError::new(format!("db error: {e}"))
                        })?;
                    repos
                        .activity
                        .clear_current_invocation_id(&sn)
                        .await
                        .map_err(|e| {
                            ::restate_sdk::prelude::TerminalError::new(format!("db error: {e}"))
                        })?;
                    Ok(::restate_sdk::prelude::Json::from(()))
                }
            })
            .name("complete_run")
            .await;
        if let Err(e) = result {
            ::tracing::error!(source = $source_name, error = %e, "failed to record run completion");
        }
    }};
}

/// Mark a run as completed with warnings inside a journaled `ctx.run()` closure.
///
/// Usage: `complete_run_with_warnings!(ctx, repos, run_id, "source", items, "summary", metadata)`
macro_rules! complete_run_with_warnings {
    ($ctx:expr, $repos:expr, $run_id:expr, $source_name:expr, $items:expr, $summary:expr, $metadata:expr) => {{
        let repos = $repos.clone();
        let sn = $source_name.to_string();
        let run_id = $run_id;
        let items = $items;
        let err_msg = $summary.to_string();
        let meta: ::serde_json::Value = $metadata;
        let result = $ctx
            .run(move || {
                let repos = repos.clone();
                let sn = sn.clone();
                let err_msg = err_msg.clone();
                let meta = meta.clone();
                async move {
                    repos
                        .activity
                        .complete_run_with_warnings(run_id, items, &err_msg, meta)
                        .await
                        .map_err(|e| {
                            ::restate_sdk::prelude::TerminalError::new(format!("db error: {e}"))
                        })?;
                    repos
                        .activity
                        .clear_current_invocation_id(&sn)
                        .await
                        .map_err(|e| {
                            ::restate_sdk::prelude::TerminalError::new(format!("db error: {e}"))
                        })?;
                    Ok(::restate_sdk::prelude::Json::from(()))
                }
            })
            .name("complete_run_with_warnings")
            .await;
        if let Err(e) = result {
            ::tracing::error!(source = $source_name, error = %e, "failed to record run completion");
        }
    }};
}

/// Mark a run as failed inside a journaled `ctx.run()` closure.
///
/// Usage: `fail_run!(ctx, repos, run_id, "source", "error message")`
macro_rules! fail_run {
    ($ctx:expr, $repos:expr, $run_id:expr, $source_name:expr, $error_msg:expr) => {{
        let repos = $repos.clone();
        let err = $error_msg.to_string();
        let sn = $source_name.to_string();
        let run_id = $run_id;
        let result = $ctx
            .run(move || {
                let repos = repos.clone();
                let err = err.clone();
                let sn = sn.clone();
                async move {
                    repos
                        .activity
                        .fail_run(run_id, &err)
                        .await
                        .map_err(|e| {
                            ::restate_sdk::prelude::TerminalError::new(format!("db error: {e}"))
                        })?;
                    repos
                        .activity
                        .clear_current_invocation_id(&sn)
                        .await
                        .map_err(|e| {
                            ::restate_sdk::prelude::TerminalError::new(format!("db error: {e}"))
                        })?;
                    Ok(::restate_sdk::prelude::Json::from(()))
                }
            })
            .name("fail_run")
            .await;
        if let Err(e) = result {
            ::tracing::error!(source = $source_name, error = %e, "failed to record run failure");
        }
    }};
}

pub(super) use complete_run;
pub(super) use complete_run_with_warnings;
pub(super) use create_run;
pub(super) use fail_run;
