use ps_core::ingestion::IngestionContext;
use ps_core::models::WatermarkField;
use restate_sdk::prelude::*;

use super::super::{
    orchestration::fetch_batch,
    progress::{BatchAction, SerFetchResult},
};
use super::batch::compute_batch_action;
use crate::infra::run_lifecycle::journaled_value;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct FetchCheckpoint {
    action: BatchAction,
    /// Bounded cursor/rate-limit metadata; API bodies never enter this journal.
    result: SerFetchResult,
    items_hash: String,
    fetch_error: Option<String>,
}

pub(super) async fn fetch_for_chunk(
    ctx: &Context<'_>,
    ing_ctx: &IngestionContext,
    cursor: &str,
    watermark_field: WatermarkField,
) -> Result<(SerFetchResult, BatchAction, Option<String>), TerminalError> {
    if ing_ctx.advances_global_watermark() {
        // Preserve the legacy journal layout for existing global invocations.
        let ic = ing_ctx.clone();
        let cur = cursor.to_owned();
        let batch = journaled_value!(ctx, "fetch_batch", [ic, cur], {
            fetch_batch(&ic, &cur).await?
        });
        let action = compute_batch_action(&batch, cursor, watermark_field);
        return Ok((batch, action, None));
    }

    let (mut live, live_error) = match fetch_batch(ing_ctx, cursor).await {
        Ok(live) => (live, None),
        Err(error) => {
            tracing::warn!(%error, "person activity fetch failed");
            (
                SerFetchResult {
                    items: vec![],
                    next_cursor: Some(cursor.into()),
                    etag: Some(cursor.into()),
                    rate_limit: None,
                    display_rate_limit: None,
                    skipped_diffs: vec![],
                },
                // Provider errors can contain response bodies. Keep full
                // details in server logs, never in durable journal payloads.
                Some("person activity fetch failed; check source access and server logs".into()),
            )
        }
    };
    let action = compute_batch_action(&live, cursor, watermark_field);
    let items_json = serde_json::json!({
        "items": live.items,
        "next_cursor": stable_cursor(live.next_cursor.as_deref()),
        "etag": stable_cursor(live.etag.as_deref()),
        "skipped_diffs": live.skipped_diffs,
    });
    let items_hash = ps_core::repo::reasoning::content_hash(&items_json);
    let items = std::mem::take(&mut live.items);
    let proposed = FetchCheckpoint {
        action,
        result: live,
        items_hash: items_hash.clone(),
        fetch_error: live_error.clone(),
    };
    let checkpoint = journaled_value!(ctx, "checkpoint_person_fetch", [proposed], { proposed });
    let sleeping = matches!(checkpoint.action, BatchAction::SleepForRateLimit { .. });
    let fingerprint_error = if sleeping {
        None
    } else if checkpoint.fetch_error.is_some() {
        checkpoint.fetch_error.clone()
    } else if live_error.is_some() {
        live_error
    } else if checkpoint.items_hash != items_hash {
        tracing::warn!(recorded_hash = %checkpoint.items_hash, current_hash = %items_hash, "scoped page fingerprint differs on replay");
        Some("upstream activity page changed before its store committed; restart this backfill to establish coverage".into())
    } else {
        None
    };
    let mut batch = checkpoint.result;
    if !sleeping {
        batch.items = items;
    }
    Ok((batch, checkpoint.action, fingerprint_error))
}

fn stable_cursor(cursor: Option<&str>) -> serde_json::Value {
    let mut value = cursor
        .and_then(|cursor| serde_json::from_str::<serde_json::Value>(cursor).ok())
        .unwrap_or_else(|| cursor.map_or(serde_json::Value::Null, Into::into));
    if let Some(object) = value.as_object_mut() {
        object.remove("last_rate_limit_remaining");
        object.remove("rate_limit_reset_at");
    }
    value
}
