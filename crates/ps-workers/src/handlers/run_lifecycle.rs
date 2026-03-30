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

/// Run a block inside a journaled `ctx.run()` closure.
///
/// Handles the double-clone dance required by Restate's `Fn` closures
/// (first clone to move into the closure, second clone because the
/// closure may be called multiple times on replay).
///
/// Variables listed in the capture list are cloned twice automatically.
/// Variables that are `Copy` (e.g. `Uuid`) can be used in the body
/// without listing them — the `move` closure captures them directly.
///
/// The body must be a block of async statements. Errors should use `?`
/// with `TerminalError` (use `.map_err(terminal_err("context"))` for
/// concise conversion).
///
/// ```ignore
/// journaled!(ctx, "step_name", [repos, some_string], {
///     repos.reasoning.update_something(id, &some_string).await
///         .map_err(terminal_err("failed to update"))?;
/// });
/// ```
macro_rules! journaled {
    ($ctx:expr, $name:expr, [$($var:ident),* $(,)?], $body:block) => {{
        $(let $var = $var.clone();)*
        $ctx.run(move || {
            $(let $var = $var.clone();)*
            async move {
                $body
                Ok(::restate_sdk::prelude::Json::from(()))
            }
        })
        .name($name)
        .await?;
    }};
}

/// Concise error mapper for converting any `Display` error into a
/// `TerminalError` with a contextual prefix.
///
/// ```ignore
/// repos.reasoning.update_status(id, "running").await
///     .map_err(terminal_err("failed to update status"))?;
/// ```
pub(super) fn terminal_err<E: std::fmt::Display>(
    context: &str,
) -> impl FnOnce(E) -> ::restate_sdk::prelude::TerminalError + '_ {
    move |e| ::restate_sdk::prelude::TerminalError::new(format!("{context}: {e}"))
}

pub(super) use complete_run;
pub(super) use complete_run_with_warnings;
pub(super) use create_run;
pub(super) use fail_run;
pub(super) use journaled;
