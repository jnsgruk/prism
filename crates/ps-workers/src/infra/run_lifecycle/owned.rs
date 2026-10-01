/// Create a run with its owner and invocation fixed at insertion time.
macro_rules! create_owned_run {
    ($ctx:expr, $repos:expr, $pipeline:expr, $source:expr, $handler:expr, $method:expr) => {{
        let repos = $repos.clone();
        let pipeline_id = $pipeline;
        let source = ::ps_core::models::SourceName::new($source);
        let handler = ::ps_core::models::HandlerName::new($handler);
        let method = ::ps_core::models::HandlerMethod::new($method);
        let invocation_id = $ctx.invocation_id().to_string();
        journaled_value!(
            $ctx,
            "create_owned_run",
            [repos, source, handler, method, invocation_id],
            {
                let id = ::uuid::Uuid::now_v7();
                let admitted = repos
                    .activity
                    .create_pipeline_run(::ps_core::repo::activity::PipelineRunParams {
                        run_id: id,
                        source_name: &source,
                        handler_name: &handler,
                        method: &method,
                        pipeline_id,
                        invocation_id: &invocation_id,
                    })
                    .await
                    .map_err(terminal_err("failed to create owned run"))?;
                if !admitted {
                    return Err(
                        ::restate_sdk::prelude::TerminalError::new("pipeline cancelled").into(),
                    );
                }
                id
            }
        )
    }};
}

/// Register eagerly dispatched children before awaiting their completion.
/// If cancellation won the registration race, stop the child immediately.
macro_rules! register_owned_invocation {
    ($ctx:expr, $repos:expr, $pipeline:expr, $handle:expr, $kind:expr, $run:expr) => {{
        let repos = $repos.clone();
        let pipeline_id = $pipeline;
        let invocation_id = $handle.invocation_id().to_string();
        let parent_invocation_id = $ctx.invocation_id().to_string();
        let kind = $kind.to_string();
        let run_id = $run;
        let admitted = journaled_value!(
            $ctx,
            "register_owned_invocation",
            [repos, invocation_id, parent_invocation_id, kind],
            {
                repos
                    .activity
                    .register_pipeline_invocation(
                        ::ps_core::repo::activity::PipelineInvocationParams {
                            pipeline_id,
                            invocation_id: &invocation_id,
                            parent_invocation_id: Some(&parent_invocation_id),
                            kind: &kind,
                            run_id,
                        },
                    )
                    .await
                    .map_err(terminal_err("failed to register owned invocation"))?
            }
        );
        if !admitted {
            $handle.cancel();
            let _ = $handle
                .attach::<::restate_sdk::prelude::Json<::serde_json::Value>>()
                .await;
            return Err(::restate_sdk::prelude::TerminalError::new(
                "pipeline cancelled",
            ));
        }
    }};
}

pub(crate) use create_owned_run;
pub(crate) use register_owned_invocation;

/// Freeze each cancellation decision at its journal position; later decisions
/// observe new cancellations without changing replayed command sequences.
macro_rules! ensure_owned_active {
    ($ctx:expr, $repos:expr, $pipeline:expr) => {{
        let repos = $repos.clone();
        let pipeline_id = $pipeline;
        let cancelled = journaled_value!($ctx, "check_owned_cancellation", [repos], {
            repos
                .activity
                .pipeline_cancel_requested(pipeline_id)
                .await
                .map_err(terminal_err("failed to check cancellation"))?
        });
        if cancelled {
            Err(::restate_sdk::prelude::TerminalError::new(
                "pipeline cancelled",
            ))
        } else {
            Ok(())
        }
    }};
}
pub(crate) use ensure_owned_active;

macro_rules! complete_owned_run {
    ($ctx:expr, $repos:expr, $run_id:expr, $source_name:expr, $items:expr) => {{
        let repos = $repos.clone();
        let run_id = $run_id;
        let items = $items;
        let result = $ctx
            .run(move || {
                let repos = repos.clone();
                async move {
                    repos
                        .activity
                        .complete_pipeline_run(run_id, items)
                        .await
                        .map_err(|e| {
                            ::restate_sdk::prelude::TerminalError::new(format!("db error: {e}"))
                        })?;
                    Ok(::restate_sdk::prelude::Json::from(()))
                }
            })
            .name("complete_owned_run")
            .await;
        if let Err(e) = result {
            ::tracing::error!(source = $source_name, error = %e, "failed to record run completion");
        }
    }};
}

pub(crate) use complete_owned_run;

macro_rules! fail_owned_run {
    ($ctx:expr, $repos:expr, $run_id:expr, $source_name:expr, $error_msg:expr) => {{
        let repos = $repos.clone();
        let err = $error_msg.to_string();
        ::tracing::error!(run_id = %$run_id, error = %err, "owned handler failed");
        let err = "Handler processing failed".to_string();
        let run_id = $run_id;
        let result = $ctx
            .run(move || {
                let repos = repos.clone();
                let err = err.clone();
                async move {
                    repos
                        .activity
                        .fail_pipeline_run(run_id, &err)
                        .await
                        .map_err(|e| {
                            ::restate_sdk::prelude::TerminalError::new(format!("db error: {e}"))
                        })?;
                    Ok(::restate_sdk::prelude::Json::from(()))
                }
            })
            .name("fail_owned_run")
            .await;
        if let Err(e) = result {
            ::tracing::error!(source = $source_name, error = %e, "failed to record run failure");
        }
    }};
}

pub(crate) use fail_owned_run;

macro_rules! complete_owned_run_with_warnings {
    ($ctx:expr, $repos:expr, $run_id:expr, $source_name:expr, $items:expr, $summary:expr, $metadata:expr) => {{
        let repos = $repos.clone();
        let run_id = $run_id;
        let items = $items;
        let err_msg = $summary.to_string();
        let meta: ::serde_json::Value = $metadata;
        let result = $ctx
            .run(move || {
                let repos = repos.clone();
                let err_msg = err_msg.clone();
                let meta = meta.clone();
                async move {
                    repos
                        .activity
                        .complete_pipeline_run_with_warnings(run_id, items, &err_msg, meta)
                        .await
                        .map_err(|e| {
                            ::restate_sdk::prelude::TerminalError::new(format!("db error: {e}"))
                        })?;
                    Ok(::restate_sdk::prelude::Json::from(()))
                }
            })
            .name("complete_owned_run_with_warnings")
            .await;
        if let Err(e) = result {
            ::tracing::error!(source = $source_name, error = %e, "failed to record run completion");
        }
    }};
}

pub(crate) use complete_owned_run_with_warnings;

macro_rules! complete_handler_run {
    ($owned:expr, $($args:tt)*) => {
        if $owned {
            complete_owned_run!($($args)*);
        } else {
            complete_run!($($args)*);
        }
    };
}
macro_rules! fail_handler_run {
    ($owned:expr, $($args:tt)*) => {
        if $owned {
            fail_owned_run!($($args)*);
        } else {
            fail_run!($($args)*);
        }
    };
}
pub(crate) use complete_handler_run;
pub(crate) use fail_handler_run;

/// Children register before work, even when their parent's registration fails.
macro_rules! register_owned_self {
    ($ctx:expr,$repos:expr,$pipeline:expr,$kind:expr,$run:expr) => {{
        let repos = $repos.clone();
        let pipeline_id = $pipeline;
        let invocation_id = $ctx.invocation_id().to_string();
        let parent_invocation_id = $ctx.headers().get("x-prism-parent-invocation").cloned();
        let kind = $kind.to_string();
        let run_id = $run;
        let admitted = journaled_value!(
            $ctx,
            "register_owned_self",
            [repos, invocation_id, parent_invocation_id, kind],
            {
                repos
                    .activity
                    .register_pipeline_invocation(
                        ::ps_core::repo::activity::PipelineInvocationParams {
                            pipeline_id,
                            invocation_id: &invocation_id,
                            parent_invocation_id: parent_invocation_id.as_deref(),
                            kind: &kind,
                            run_id,
                        },
                    )
                    .await
                    .map_err(terminal_err("failed to register owned invocation"))?
            }
        );
        if !admitted {
            return Err(::restate_sdk::prelude::TerminalError::new(
                "pipeline stopped",
            ));
        }
    }};
}
pub(crate) use register_owned_self;
