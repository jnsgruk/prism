use ps_core::ingestion::{IngestionContext, IngestionPlan};
use ps_core::models::SourceConfig;
use restate_sdk::prelude::*;

use crate::infra::run_lifecycle::{
    complete_run, complete_run_with_warnings, create_run, fail_run, journaled, journaled_value,
    terminal_err,
};
use crate::infra::{
    SharedState, decrypt_optional_secret, decrypt_required_secret, load_source_config,
};

use super::chunk::{ChunkRequest, IngestionChunkServiceClient};
use super::finalise::{extract_failed_items, extract_watermark, finalise_run};
use super::progress::{IngestionSpec, SerFetchResult};

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
    let start = std::time::Instant::now();

    let source = crate::infra::registry::create_source(&config.source_type).ok_or_else(|| {
        TerminalError::new(format!("unsupported source type: {}", config.source_type))
    })?;

    let method = if override_watermark.is_some() {
        "backfill"
    } else {
        "run_ingestion"
    };
    let run_id =
        create_ingestion_run(ctx, &state.repos, source_name, spec.handler_name, method).await?;

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

    let ing_ctx = build_ingestion_context(state, config, token, email, api_username);

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
            fail_ingestion_run(ctx, &state.repos, run_id, source_name, &e).await;
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
        chunk_num += 1;
        tracing::info!(chunk = chunk_num, "dispatching chunk");

        let request = ChunkRequest {
            source_type: config.source_type.clone(),
            cursor: cursor.clone(),
            run_id,
            max_batches: chunk_size,
            items_offset: total_items,
        };

        let chunk_result = ctx
            .service_client::<IngestionChunkServiceClient>()
            .process_chunk(Json(request))
            .call()
            .await;

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

    // Always finalise the run, even if a chunk failed.
    if let Some(ref error_msg) = chunk_error {
        if total_items > 0 {
            let metadata = serde_json::json!({ "chunk_error": error_msg });
            complete_ingestion_run_with_warnings(
                ctx,
                &state.repos,
                run_id,
                source_name,
                total_items,
                error_msg,
                metadata,
            )
            .await;
        } else {
            fail_ingestion_run(ctx, &state.repos, run_id, source_name, error_msg).await;
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
    }
}

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

/// Mark a run as complete inside a Restate `ctx.run()` closure.
pub(super) async fn complete_ingestion_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run_id: uuid::Uuid,
    source_name: &str,
    items_collected: i32,
) {
    complete_run!(ctx, repos, run_id, source_name, items_collected);
}

/// Mark a run as completed with warnings inside a Restate `ctx.run()` closure.
pub(super) async fn complete_ingestion_run_with_warnings(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    run_id: uuid::Uuid,
    source_name: &str,
    items_collected: i32,
    error_summary: &str,
    metadata: serde_json::Value,
) {
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
    run_id: uuid::Uuid,
    source_name: &str,
    error_msg: &str,
) {
    fail_run!(ctx, repos, run_id, source_name, error_msg);
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
