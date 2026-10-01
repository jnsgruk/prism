use ps_core::ingestion::{IngestionContext, IngestionPlan};
use ps_core::models::SourceConfig;
use restate_sdk::prelude::*;

use crate::infra::run_lifecycle::{
    create_owned_run, ensure_owned_active, journaled, journaled_value, register_owned_invocation,
    terminal_err,
};
use crate::infra::{
    SharedState, decrypt_optional_secret, decrypt_required_secret, load_source_config,
};

use super::lifecycle::{
    IngestionRun, RunWarnings, complete_ingestion_run_with_warnings, create_ingestion_run,
    fail_ingestion_run,
};

use super::chunk::{ChunkRequest, IngestionChunkServiceClient};
use super::finalise::{extract_failed_items, extract_watermark, finalise_run};
use super::progress::{IngestionSpec, SerFetchResult};
use super::scope::{reject_unavailable_scope, validate_selected_config};

/// Load a source config inside a Restate `ctx.run()` closure.
pub async fn load_ingestion_source_config(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    source_name: &str,
) -> Result<SourceConfig, TerminalError> {
    let repos = repos.clone();
    let name = source_name.to_string();
    Ok(journaled_value!(ctx, "load_config", [repos, name], {
        load_source_config(&repos, &name)
            .await
            .map_err(TerminalError::new)?
    }))
}

/// Shared ingestion orchestration used by all ingestion handlers.
///
/// Dispatches work to `IngestionChunkService` in batches of `chunk_size`.
/// Each chunk runs as a separate Restate invocation with its own small
/// journal. The coordinator's journal stays minimal (~1 entry per chunk).
///
/// Handles: source creation, run creation, secret decryption, planning,
/// watermark override, chunked dispatch, run finalisation.
/// `IngestionChunkService` in batches of `chunk_size`.
///
/// Each chunk runs as a separate Restate invocation with its own small
/// journal. The coordinator's journal stays minimal (~1 entry per chunk).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn execute_ingestion_chunked(
    ctx: &ObjectContext<'_>,
    state: &SharedState,
    spec: &IngestionSpec,
    source_name: &str,
    config: &SourceConfig,
    override_watermark: Option<String>,
    chunk_size: usize,
    trigger_downstream: impl FnOnce(&ObjectContext<'_>),
) -> Result<(), TerminalError> {
    execute_ingestion(
        ctx,
        state,
        IngestionArgs {
            spec,
            source_name,
            config,
            override_watermark,
            chunk_size,
            trigger_downstream,
            request: None,
        },
    )
    .await
}

struct IngestionArgs<'a, F> {
    spec: &'a IngestionSpec,
    source_name: &'a str,
    config: &'a SourceConfig,
    override_watermark: Option<String>,
    chunk_size: usize,
    trigger_downstream: F,
    request: Option<ps_core::ingestion::SourceRunContext>,
}

#[allow(clippy::too_many_lines)]
async fn execute_ingestion(
    ctx: &ObjectContext<'_>,
    state: &SharedState,
    args: IngestionArgs<'_, impl FnOnce(&ObjectContext<'_>)>,
) -> Result<(), TerminalError> {
    let IngestionArgs {
        spec,
        source_name,
        config,
        override_watermark,
        chunk_size,
        trigger_downstream,
        request,
    } = args;
    let start = std::time::Instant::now();

    let source = crate::infra::registry::create_source(&config.source_type).ok_or_else(|| {
        TerminalError::new(format!("unsupported source type: {}", config.source_type))
    })?;

    let method = if override_watermark.is_some() {
        "backfill"
    } else {
        "run_ingestion"
    };
    let run_id = if let Some(ref request) = request {
        ensure_owned_active!(ctx, state.repos, request.pipeline_id)?;
        create_owned_run!(
            ctx,
            state.repos,
            request.pipeline_id,
            source_name,
            spec.handler_name,
            method
        )
    } else {
        create_ingestion_run(ctx, &state.repos, source_name, spec.handler_name, method).await?
    };

    if request.is_none() {
        let invocation_id = ctx.invocation_id().to_string();
        let repos = state.repos.clone();
        let sn = source_name.to_string();
        journaled!(ctx, "set_invocation_id", [repos, sn, invocation_id], {
            repos
                .activity
                .set_current_invocation_id(&sn, &invocation_id)
                .await
                .map_err(terminal_err("failed to set invocation ID"))?;
        });
    }

    let span = tracing::info_span!(
        "handler",
        handler = spec.handler_name,
        source = source_name,
        run_id = %run_id,
    );
    let _guard = span.enter();

    tracing::info!("starting chunked ingestion");

    // Decrypt secrets outside ctx.run() to avoid journaling plaintext.
    let token = match (spec.token_key, spec.token_required) {
        (Some(key), true) => Some(decrypt_required_secret(state, config.id, key).await?),
        (Some(key), false) => decrypt_optional_secret(state, config.id, key).await?,
        (None, _) => None,
    };
    let email = match spec.email_key {
        Some(key) => decrypt_optional_secret(state, config.id, key).await?,
        None => None,
    };
    let api_username = match spec.api_username_key {
        Some(key) => decrypt_optional_secret(state, config.id, key).await?,
        None => None,
    };

    let mut ing_ctx = build_ingestion_context(state, config, token, email, api_username);
    ing_ctx.request = request.clone();

    // Journal the plan. plan() reads watermarks and (for GitHub) the
    // team-repos list from the DB — both can change between replays,
    // which would alter the cursor parameter passed to process_chunk()
    // and trigger a Restate journal mismatch (error 570). Freezing the
    // plan output in the journal makes the cursor deterministic.
    let source_type = config.source_type.clone();
    let ic = ing_ctx.clone();
    let plan_result: Result<IngestionPlan, String> =
        journaled_value!(ctx, "plan", [source_type, ic], {
            let src = crate::infra::registry::create_source(&source_type).ok_or_else(|| {
                TerminalError::new(format!("unsupported source type: {source_type}"))
            })?;
            src.plan(&ic).await.map_err(|e| e.to_string())
        });

    let mut plan = match plan_result {
        Ok(p) => p,
        Err(e) => {
            fail_ingestion_run(
                ctx,
                &state.repos,
                IngestionRun {
                    id: run_id,
                    source_name,
                    owned: request.is_some(),
                },
                &e,
            )
            .await;
            return Err(TerminalError::new(format!("plan failed: {e}")));
        }
    };

    if let Some(ref wm) = override_watermark {
        plan.watermark = Some(wm.clone());
    }

    tracing::debug!(watermark = ?plan.watermark, "ingestion plan ready");

    let initial_cursor = source.initial_cursor(&ing_ctx, &plan);

    // Dispatch chunks sequentially to the chunk service.
    let mut cursor = initial_cursor;
    let mut total_items = 0i32;
    let mut chunk_num = 0u32;
    let mut chunk_error: Option<String> = None;

    loop {
        if let Some(ref request) = request {
            ensure_owned_active!(ctx, state.repos, request.pipeline_id)?;
        }
        chunk_num += 1;
        tracing::info!(chunk = chunk_num, "dispatching chunk");

        let request = ChunkRequest {
            source_type: config.source_type.clone(),
            cursor: cursor.clone(),
            run_id,
            max_batches: chunk_size,
            items_offset: total_items,
            request: request.clone(),
        };

        let chunk_result = if let Some(snapshot) = request.request.as_ref() {
            let pipeline_id = snapshot.pipeline_id;
            let call = ctx
                .service_client::<IngestionChunkServiceClient>()
                .process_scoped_chunk(Json(request))
                .header(
                    "x-prism-parent-invocation".into(),
                    ctx.invocation_id().to_string(),
                )
                .call();
            let handle = call.invocation_handle().await?;
            register_owned_invocation!(
                ctx,
                state.repos,
                pipeline_id,
                handle,
                "chunk",
                Some(run_id)
            );
            call.await
        } else {
            ctx.service_client::<IngestionChunkServiceClient>()
                .process_chunk(Json(request))
                .call()
                .await
        };

        match chunk_result {
            Ok(json) => {
                let result = json.into_inner();
                total_items += result.items_stored;
                cursor = result.cursor;

                tracing::info!(
                    chunk = chunk_num,
                    items_in_chunk = result.items_stored,
                    total_items,
                    is_complete = result.is_complete,
                    "chunk finished"
                );

                if result.is_complete {
                    break;
                }
            }
            Err(e) => {
                tracing::error!(chunk = chunk_num, error = %e, "chunk failed");
                chunk_error = Some(format!("chunk {chunk_num} failed: {e}"));
                break;
            }
        }
    }

    if let Some(ref request) = request {
        ensure_owned_active!(ctx, state.repos, request.pipeline_id)?;
    }

    // Always finalise the run, even if a chunk failed.
    if let Some(ref error_msg) = chunk_error {
        if total_items > 0 {
            let metadata = if request.is_some() {
                serde_json::json!({"chunk_error":"A chunk failed after partial ingestion"})
            } else {
                serde_json::json!({"chunk_error":error_msg})
            };
            complete_ingestion_run_with_warnings(
                ctx,
                &state.repos,
                run_id,
                source_name,
                RunWarnings {
                    items_collected: total_items,
                    error_summary: if request.is_some() {
                        "A chunk failed after partial ingestion"
                    } else {
                        error_msg
                    },
                    metadata,
                    owned: request.is_some(),
                },
            )
            .await;
        } else {
            fail_ingestion_run(
                ctx,
                &state.repos,
                IngestionRun {
                    id: run_id,
                    source_name,
                    owned: request.is_some(),
                },
                error_msg,
            )
            .await;
        }
        return Err(TerminalError::new(error_msg.clone()));
    }

    let failed_items = extract_failed_items(&cursor);
    finalise_run(
        ctx,
        &state.repos,
        &ing_ctx,
        run_id,
        source_name,
        total_items,
        &failed_items,
        spec.item_noun,
        &cursor,
        source.watermark_field(),
    )
    .await?;

    if total_items > 0 {
        tracing::debug!("triggering downstream handlers");
        trigger_downstream(ctx);
    }

    tracing::info!(
        total_items,
        chunks = chunk_num,
        duration_secs = start.elapsed().as_secs(),
        "chunked ingestion complete"
    );
    Ok(())
}

/// Construct an `IngestionContext` from shared state and config.
pub fn build_ingestion_context(
    state: &SharedState,
    config: &SourceConfig,
    token: Option<String>,
    email: Option<String>,
    api_username: Option<String>,
) -> IngestionContext {
    IngestionContext {
        repos: state.repos.clone(),
        source_config: config.clone(),
        http_client: state.http_client.clone(),
        token,
        email,
        api_username,
        request: None,
    }
}

/// Fetch a batch — NOT called inside `ctx.run()` directly, but used
/// within `journaled_value!` in `fetch_store_loop`.
pub async fn fetch_batch(
    ing_ctx: &IngestionContext,
    cursor: &str,
) -> Result<SerFetchResult, TerminalError> {
    let src = crate::infra::registry::create_source(&ing_ctx.source_config.source_type)
        .ok_or_else(|| TerminalError::new("source unavailable"))?;
    let result = src
        .fetch_batch(ing_ctx, cursor)
        .await
        .map_err(terminal_err("fetch failed"))?;

    Ok(SerFetchResult {
        items: result.items,
        next_cursor: result.next_cursor,
        rate_limit: result.rate_limit,
        display_rate_limit: result.display_rate_limit,
        etag: result.etag,
        skipped_diffs: result.skipped_diffs,
    })
}

/// Advance the watermark inside a Restate `ctx.run()` closure.
///
/// `watermark_field` is the JSON field to extract from the cursor
/// (e.g. `"max_updated_at"` or `"max_bumped_at"`).
pub async fn advance_watermark(
    ctx: &ObjectContext<'_>,
    ing_ctx: &IngestionContext,
    cursor: &str,
    total_items: i32,
    watermark_field: ps_core::models::WatermarkField,
) -> Result<(), TerminalError> {
    let ic = ing_ctx.clone();
    let wm = cursor.to_string();

    journaled!(ctx, "advance_watermark", [ic, wm], {
        let src = crate::infra::registry::create_source(&ic.source_config.source_type)
            .ok_or_else(|| TerminalError::new("source unavailable"))?;
        let watermark = extract_watermark(&wm, watermark_field).unwrap_or_default();
        src.advance_watermark(&ic, &watermark, total_items)
            .await
            .map_err(terminal_err("advance failed"))?;
    });

    Ok(())
}

/// Versioned coordinator path consumes the admitted source and identity snapshot.
pub async fn execute_scoped_ingestion(
    ctx: &ObjectContext<'_>,
    state: &SharedState,
    spec: &IngestionSpec,
    request: ps_core::ingestion::SourceRunContext,
) -> Result<(), TerminalError> {
    reject_unavailable_scope(&request)?;
    request
        .validate()
        .map_err(terminal_err("invalid scoped ingestion request"))?;
    ensure_owned_active!(ctx, state.repos, request.pipeline_id)?;
    let repos = state.repos.clone();
    let selected = request.source.clone();
    let config = journaled_value!(ctx, "load_exact_source", [repos, selected], {
        let config = repos
            .config
            .get_source(selected.source_id.into_inner())
            .await
            .map_err(terminal_err("failed to load exact source"))?
            .ok_or_else(|| TerminalError::new("selected source no longer exists"))?;
        validate_selected_config(&config, &selected)?;
        config
    });
    let source_name = config.name.clone();
    let watermark = request.since_date.clone();
    execute_ingestion(
        ctx,
        state,
        IngestionArgs {
            spec,
            source_name: &source_name,
            config: &config,
            override_watermark: watermark,
            chunk_size: 50,
            trigger_downstream: |_ctx: &ObjectContext<'_>| {},
            request: Some(request),
        },
    )
    .await
}
