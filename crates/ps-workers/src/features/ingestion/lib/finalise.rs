use ps_core::ingestion::{ContributionInput, FailedItem, IngestionContext, SkippedDiff};
use ps_core::models::RateLimitInfo;
use ps_core::repo::Repos;
use ps_core::repo::reasoning::{EnrichmentQueueEntry, content_hash};
use restate_sdk::prelude::*;
use tracing::debug;
use uuid::Uuid;

use crate::infra::run_lifecycle::terminal_err;

use super::orchestration::{
    advance_watermark, complete_ingestion_run, complete_ingestion_run_with_warnings,
    fail_ingestion_run,
};

/// Extract a named field from a serialised cursor JSON string.
pub fn extract_watermark(
    cursor_json: &str,
    field: ps_core::models::WatermarkField,
) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(cursor_json)
        .ok()?
        .get(field.as_str())?
        .as_str()
        .map(String::from)
}

/// Extract failed items from the final cursor JSON.
pub fn extract_failed_items(cursor: &str) -> Vec<FailedItem> {
    serde_json::from_str::<serde_json::Value>(cursor)
        .ok()
        .and_then(|v| v.get("failed_items").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Finalise an ingestion run based on whether there were failures.
///
/// Three outcomes:
/// - No failures → advance watermark + complete
/// - All items failed (`total_items` == 0) → fail
/// - Partial failure → complete with warnings (do NOT advance watermark)
#[allow(clippy::too_many_arguments)]
pub async fn finalise_run(
    ctx: &ObjectContext<'_>,
    repos: &ps_core::repo::Repos,
    ing_ctx: &IngestionContext,
    run_id: Uuid,
    source_name: &str,
    total_items: i32,
    failed_items: &[FailedItem],
    item_noun: &str,
    final_cursor: &str,
    watermark_field: ps_core::models::WatermarkField,
) -> Result<(), TerminalError> {
    if failed_items.is_empty() {
        if total_items > 0 {
            advance_watermark(ctx, ing_ctx, final_cursor, total_items, watermark_field).await?;
        }
        complete_ingestion_run(ctx, repos, run_id, source_name, total_items).await;
    } else if total_items == 0 {
        let summary = format!(
            "all {} {item_noun}(s) failed: {}",
            failed_items.len(),
            failed_items
                .iter()
                .map(|f| f.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        fail_ingestion_run(ctx, repos, run_id, source_name, &summary).await;
    } else {
        // Partial failure — do NOT advance watermark.
        let summary = format!(
            "{} {item_noun}(s) failed: {}",
            failed_items.len(),
            failed_items
                .iter()
                .map(|f| f.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let metadata = serde_json::json!({ "failed_items": failed_items });
        complete_ingestion_run_with_warnings(
            ctx,
            repos,
            run_id,
            source_name,
            total_items,
            &summary,
            metadata,
        )
        .await;
    }
    Ok(())
}

/// Compute how long to sleep until a rate limit reset, with a buffer.
pub fn diff_rate_limit_sleep_duration(rate_limit: &RateLimitInfo) -> std::time::Duration {
    let now = time::OffsetDateTime::now_utc();
    let delta = rate_limit.reset_at - now;
    let secs = delta.whole_seconds().max(1) + 1;
    #[allow(clippy::cast_sign_loss)]
    std::time::Duration::from_secs(secs as u64)
}

/// Retry fetching PR diffs that were skipped due to rate limiting,
/// then re-enqueue the affected contributions for enrichment with
/// updated content that includes the diff.
///
/// Called after a durable `ctx.sleep()` so the rate limit has reset.
pub async fn retry_skipped_diffs(
    ctx: &ObjectContext<'_>,
    ing_ctx: &IngestionContext,
    original_items: &[ContributionInput],
    skipped: &[SkippedDiff],
) -> Result<(), TerminalError> {
    let token = ing_ctx.token.as_deref().unwrap_or("");
    if token.is_empty() {
        return Ok(());
    }

    let api_base = ing_ctx
        .source_config
        .settings
        .get("base_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.github.com");

    let client = crate::features::ingestion::github::client::GitHubClient::new(
        ing_ctx.http_client.clone(),
        api_base,
        token,
    );

    // Fetch diffs for skipped PRs.
    let mut updated_items: Vec<(String, serde_json::Value)> = Vec::new();

    for sd in skipped {
        match crate::features::ingestion::github::source::fetch::fetch_single_pr_diff(
            &client,
            &sd.owner,
            &sd.repo,
            sd.pr_number,
        )
        .await
        {
            crate::features::ingestion::github::source::fetch::DiffFetchResult::Ok(diff_text) => {
                // Get the original enrichment_content and add the diff.
                if let Some(item) = original_items.get(sd.item_index)
                    && let Some(ref enrichment) = item.enrichment_content
                {
                    let mut content = enrichment.clone();
                    if let Some(obj) = content.as_object_mut() {
                        obj.insert("diff".to_string(), serde_json::Value::String(diff_text));
                    }
                    let platform_id = item.platform_id.clone();
                    updated_items.push((platform_id, content));
                }
            }
            crate::features::ingestion::github::source::fetch::DiffFetchResult::RateLimited(_) => {
                tracing::warn!(
                    remaining = skipped.len() - updated_items.len(),
                    "diff retry also hit rate limit, skipping remaining"
                );
                break;
            }
            crate::features::ingestion::github::source::fetch::DiffFetchResult::Failed => {}
        }
    }

    if updated_items.is_empty() {
        return Ok(());
    }

    // Re-enqueue enrichments with updated content (now including diffs).
    let repos = ing_ctx.repos.clone();
    let items_for_closure = updated_items.clone();

    let result = ctx
        .run(|| {
            let repos = repos.clone();
            let items = items_for_closure.clone();
            async move {
                let platform_ids: Vec<String> = items.iter().map(|(pid, _)| pid.clone()).collect();
                let id_pairs = repos
                    .activity
                    .get_contribution_ids_by_platform_ids("github", &platform_ids)
                    .await
                    .map_err(terminal_err("db error"))?;

                let content_by_pid: std::collections::HashMap<&str, &serde_json::Value> = items
                    .iter()
                    .map(|(pid, content)| (pid.as_str(), content))
                    .collect();

                let entries: Vec<ps_core::repo::reasoning::EnrichmentQueueEntry> = id_pairs
                    .iter()
                    .filter_map(|(contribution_id, platform_id)| {
                        let content = content_by_pid.get(platform_id.as_str())?;
                        Some(ps_core::repo::reasoning::EnrichmentQueueEntry {
                            contribution_id: *contribution_id,
                            content: (*content).clone(),
                            content_hash: ps_core::repo::reasoning::content_hash(content),
                        })
                    })
                    .collect();

                if !entries.is_empty() {
                    repos
                        .reasoning
                        .bulk_enqueue_enrichments(&entries)
                        .await
                        .map_err(terminal_err("enqueue error"))?;
                }

                #[allow(clippy::cast_possible_wrap)]
                Ok(Json::from(entries.len() as i32))
            }
        })
        .name("retry_diff_enqueue")
        .await;

    match result {
        Ok(count) => {
            let re_enqueued = count.into_inner();
            tracing::info!(
                fetched = updated_items.len(),
                re_enqueued,
                "retried skipped diffs"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to re-enqueue retried diffs");
        }
    }

    Ok(())
}

/// Enqueue enrichment content for upserted contributions.
///
/// Maps `(id, platform_id)` pairs from `bulk_upsert_contributions` back to
/// the `enrichment_content` blobs on the original items, computes content
/// hashes, and bulk-inserts into the enrichment queue.
///
/// Called from each source's `store_batch_impl` after upsert.
pub async fn enqueue_enrichments(
    repos: &Repos,
    items: &[&ContributionInput],
    upserted: &[(Uuid, String)],
) -> Result<u64, ps_core::Error> {
    // Build a platform_id → enrichment_content lookup from items.
    let content_by_platform_id: std::collections::HashMap<&str, &serde_json::Value> = items
        .iter()
        .filter_map(|item| {
            item.enrichment_content
                .as_ref()
                .map(|c| (item.platform_id.as_str(), c))
        })
        .collect();

    let entries: Vec<EnrichmentQueueEntry> = upserted
        .iter()
        .filter_map(|(contribution_id, platform_id)| {
            let content = content_by_platform_id.get(platform_id.as_str())?;
            Some(EnrichmentQueueEntry {
                contribution_id: *contribution_id,
                content: (*content).clone(),
                content_hash: content_hash(content),
            })
        })
        .collect();

    if entries.is_empty() {
        return Ok(0);
    }

    let count = repos.reasoning.bulk_enqueue_enrichments(&entries).await?;
    debug!(
        queued = entries.len(),
        updated = count,
        "enqueued enrichments"
    );
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- extract_watermark --

    #[test]
    fn extract_watermark_max_updated_at() {
        let cursor = r#"{"max_updated_at": "2025-01-15T12:00:00Z", "repo_index": 0}"#;
        assert_eq!(
            extract_watermark(cursor, ps_core::models::WatermarkField::MaxUpdatedAt),
            Some("2025-01-15T12:00:00Z".into())
        );
    }

    #[test]
    fn extract_watermark_max_bumped_at() {
        let cursor = r#"{"max_bumped_at": "2025-02-01T00:00:00Z"}"#;
        assert_eq!(
            extract_watermark(cursor, ps_core::models::WatermarkField::MaxBumpedAt),
            Some("2025-02-01T00:00:00Z".into())
        );
    }

    #[test]
    fn extract_watermark_missing_field() {
        let cursor = r#"{"repo_index": 0}"#;
        assert_eq!(
            extract_watermark(cursor, ps_core::models::WatermarkField::MaxUpdatedAt),
            None
        );
    }

    #[test]
    fn extract_watermark_null_value() {
        let cursor = r#"{"max_updated_at": null}"#;
        assert_eq!(
            extract_watermark(cursor, ps_core::models::WatermarkField::MaxUpdatedAt),
            None
        );
    }

    #[test]
    fn extract_watermark_invalid_json() {
        assert_eq!(
            extract_watermark("not json", ps_core::models::WatermarkField::MaxUpdatedAt),
            None
        );
    }

    // -- extract_failed_items --

    #[test]
    fn extract_failed_items_empty() {
        let cursor = r#"{"failed_items": []}"#;
        assert!(extract_failed_items(cursor).is_empty());
    }

    #[test]
    fn extract_failed_items_with_entries() {
        let cursor = r#"{"failed_items": [{"key": "org/repo", "error": "403 forbidden"}]}"#;
        let items = extract_failed_items(cursor);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, "org/repo");
        assert_eq!(items[0].error, "403 forbidden");
    }

    #[test]
    fn extract_failed_items_missing_field() {
        let cursor = r#"{"repo_index": 0}"#;
        assert!(extract_failed_items(cursor).is_empty());
    }

    #[test]
    fn extract_failed_items_invalid_json() {
        assert!(extract_failed_items("bad json").is_empty());
    }

    #[test]
    fn extract_failed_items_multiple() {
        let cursor = r#"{"failed_items": [
            {"key": "a", "error": "e1"},
            {"key": "b", "error": "e2"},
            {"key": "c", "error": "e3"}
        ]}"#;
        let items = extract_failed_items(cursor);
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].key, "c");
    }
}
