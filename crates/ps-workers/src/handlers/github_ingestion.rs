use ps_core::ingestion::FailedItem;
use ps_core::models::{ContributionType, RateLimitInfo, SourceConfig};
use restate_sdk::prelude::*;
use serde::Serialize;
use tracing::{info, warn};

use super::SharedState;
use super::ingestion_common::{
    advance_watermark, build_ingestion_context, complete_ingestion_run,
    complete_ingestion_run_with_warnings, create_ingestion_run, decrypt_required_secret,
    fail_ingestion_run, fetch_batch, load_source_config, store_batch,
};
use super::metrics_compute::MetricsComputeHandlerClient;
use crate::registry;

pub struct GithubIngestionHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::object]
pub trait GithubIngestionHandler {
    /// Run an incremental ingestion for the source identified by the object key.
    async fn run_ingestion() -> Result<(), TerminalError>;

    /// Run a backfill from a specific date.
    async fn backfill(since_date: String) -> Result<(), TerminalError>;
}

impl GithubIngestionHandler for GithubIngestionHandlerImpl {
    async fn run_ingestion(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let source_type_key = ctx.key().to_string();
        let config = load_source_config(&ctx, &self.state.repos, &source_type_key).await?;
        let source_name = config.name.clone();
        info!(source = %source_name, "starting ingestion run");

        self.execute_ingestion(&ctx, &source_name, &config, None)
            .await
    }

    async fn backfill(
        &self,
        ctx: ObjectContext<'_>,
        since_date: String,
    ) -> Result<(), TerminalError> {
        let source_type_key = ctx.key().to_string();
        let config = load_source_config(&ctx, &self.state.repos, &source_type_key).await?;
        let source_name = config.name.clone();
        info!(source = %source_name, since = %since_date, "starting backfill");

        self.execute_ingestion(&ctx, &source_name, &config, Some(since_date))
            .await
    }
}

impl GithubIngestionHandlerImpl {
    async fn execute_ingestion(
        &self,
        ctx: &ObjectContext<'_>,
        source_name: &str,
        config: &SourceConfig,
        override_watermark: Option<String>,
    ) -> Result<(), TerminalError> {
        let source = registry::create_source(&config.source_type).ok_or_else(|| {
            TerminalError::new(format!("unsupported source type: {}", config.source_type))
        })?;

        let method = if override_watermark.is_some() {
            "backfill"
        } else {
            "run_ingestion"
        };
        let run_id = create_ingestion_run(
            ctx,
            &self.state.repos,
            source_name,
            "GithubIngestionHandler",
            method,
        )
        .await?;

        // Decrypt token once per run, outside ctx.run() to avoid journaling
        let token = decrypt_required_secret(&self.state, config.id, "api_token").await?;

        let ing_ctx = build_ingestion_context(&self.state, config, Some(token), None, None);
        let mut plan = match source.plan(&ing_ctx).await {
            Ok(p) => p,
            Err(e) => {
                fail_ingestion_run(ctx, &self.state.repos, run_id, source_name, &e.to_string())
                    .await;
                return Err(TerminalError::new(format!("plan failed: {e}")));
            }
        };

        if let Some(ref wm) = override_watermark {
            plan.watermark = Some(wm.clone());
        }

        info!(
            source = source_name,
            repos = plan.repos.len(),
            watermark = ?plan.watermark,
            "ingestion plan ready"
        );

        if plan.repos.is_empty() {
            info!(source = source_name, "no repos to ingest");
            complete_ingestion_run(ctx, &self.state.repos, run_id, source_name, 0).await;
            return Ok(());
        }

        let (total_items, final_cursor) = self
            .fetch_store_loop(
                ctx,
                run_id,
                source_name,
                config,
                source.as_ref(),
                &plan,
                ing_ctx.token.as_deref(),
            )
            .await?;

        // Extract failed_items from final cursor
        let failed_items: Vec<FailedItem> =
            serde_json::from_str::<serde_json::Value>(&final_cursor)
                .ok()
                .and_then(|v| v.get("failed_items").cloned())
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();

        if failed_items.is_empty() {
            if total_items > 0 {
                advance_watermark(
                    ctx,
                    &self.state,
                    config,
                    &final_cursor,
                    total_items,
                    ing_ctx.token.as_deref(),
                    "max_updated_at",
                )
                .await?;
            }
            complete_ingestion_run(ctx, &self.state.repos, run_id, source_name, total_items).await;
        } else if total_items == 0 {
            let summary = format!(
                "all {} repo(s) failed: {}",
                failed_items.len(),
                failed_items
                    .iter()
                    .map(|f| f.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            fail_ingestion_run(ctx, &self.state.repos, run_id, source_name, &summary).await;
        } else {
            // Partial failure — do NOT advance watermark.
            let summary = format!(
                "{} repo(s) failed: {}",
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
                &self.state.repos,
                run_id,
                source_name,
                total_items,
                &summary,
                metadata,
            )
            .await;
        }

        if total_items > 0 {
            info!(source = source_name, "triggering metrics recomputation");
            ctx.service_client::<MetricsComputeHandlerClient>()
                .compute_current_periods()
                .send();
        }

        info!(source = source_name, total_items, "ingestion complete");
        Ok(())
    }

    /// Fetch and store batches in a loop, returning `(total_items, final_cursor)`.
    #[allow(clippy::too_many_arguments)]
    async fn fetch_store_loop(
        &self,
        ctx: &ObjectContext<'_>,
        run_id: uuid::Uuid,
        source_name: &str,
        config: &SourceConfig,
        source: &dyn ps_core::ingestion::Source,
        plan: &ps_core::ingestion::IngestionPlan,
        token: Option<&str>,
    ) -> Result<(i32, String), TerminalError> {
        let mut cursor = source.initial_cursor(plan);
        let mut total_items = 0i32;
        let mut prs_fetched = 0u32;
        let mut reviews_fetched = 0u32;
        let mut identities_skipped = 0u32;

        loop {
            let batch = fetch_batch(&self.state, config, &cursor, token).await?;

            // Count PRs vs reviews in the batch.
            for item in &batch.items {
                match item.contribution_type {
                    ContributionType::PullRequest => prs_fetched += 1,
                    ContributionType::PrReview => reviews_fetched += 1,
                    _ => {}
                }
            }

            if !batch.items.is_empty() {
                let batch_size = batch.items.len();
                let stored = store_batch(ctx, &self.state, config, &batch.items, token).await?;
                total_items += stored;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    identities_skipped += (batch_size as u32).saturating_sub(stored as u32);
                }

                info!(
                    source = source_name,
                    batch_stored = stored,
                    total_items,
                    "stored batch"
                );
            }

            // Build progress JSON from cursor state.
            let progress = build_progress_json(
                &cursor,
                prs_fetched,
                reviews_fetched,
                identities_skipped,
                batch.rate_limit.as_ref(),
            );

            if let Err(e) = self
                .state
                .repos
                .activity
                .update_run_progress_detail(run_id, total_items, &progress)
                .await
            {
                warn!(source = source_name, "failed to update run progress: {e}");
            }

            let Some(next_cursor) = batch.next_cursor else {
                break;
            };
            cursor = next_cursor;
        }

        // Final progress update with "complete" phase.
        let final_progress = serde_json::json!({
            "phase": "complete",
            "prs_fetched": prs_fetched,
            "reviews_fetched": reviews_fetched,
            "identities_skipped": identities_skipped,
        });
        if let Err(e) = self
            .state
            .repos
            .activity
            .update_run_progress_detail(run_id, total_items, &final_progress)
            .await
        {
            warn!(source = source_name, "failed to update final progress: {e}");
        }

        Ok((total_items, cursor))
    }
}

/// Structured progress report for an ingestion run.
#[derive(Serialize)]
struct ProgressReport {
    phase: String,
    prs_fetched: u32,
    reviews_fetched: u32,
    identities_skipped: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    repos_total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repos_completed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search_users_total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search_users_completed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_limit_remaining: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_limit_limit: Option<i32>,
    status_message: String,
}

/// Build a structured progress JSON object from cursor state and counters.
fn build_progress_json(
    cursor_json: &str,
    prs_fetched: u32,
    reviews_fetched: u32,
    identities_skipped: u32,
    rate_limit: Option<&RateLimitInfo>,
) -> serde_json::Value {
    let cursor: serde_json::Value =
        serde_json::from_str(cursor_json).unwrap_or(serde_json::Value::Null);

    let phase = cursor
        .get("phase")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let phase_label = match phase {
        "TeamRepos" => "team_repos",
        "MemberSearch" => "member_search",
        other => other,
    };

    let mut report = ProgressReport {
        phase: phase_label.to_string(),
        prs_fetched,
        reviews_fetched,
        identities_skipped,
        repos_total: None,
        repos_completed: None,
        current_repo: None,
        search_users_total: None,
        search_users_completed: None,
        rate_limit_remaining: None,
        rate_limit_limit: None,
        status_message: String::new(),
    };

    // Add phase-specific fields.
    if phase == "TeamRepos" {
        let repos_total = cursor
            .get("repos")
            .and_then(|v| v.as_array())
            .map_or(0, Vec::len);
        let repo_index = cursor
            .get("repo_index")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let current_repo = cursor
            .get("repos")
            .and_then(|v| v.as_array())
            .and_then(|repos| repos.get(repo_index as usize))
            .map(|r| {
                format!(
                    "{}/{}",
                    r.get("owner").and_then(|v| v.as_str()).unwrap_or(""),
                    r.get("repo").and_then(|v| v.as_str()).unwrap_or(""),
                )
            });

        report.repos_total = Some(repos_total);
        report.repos_completed = Some(repo_index);
        report.current_repo = current_repo;
    } else if phase == "MemberSearch" {
        let search_users_total = cursor
            .get("search_users")
            .and_then(|v| v.as_array())
            .map_or(0, Vec::len);
        let search_user_index = cursor
            .get("search_user_index")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);

        report.search_users_total = Some(search_users_total);
        report.search_users_completed = Some(search_user_index);
    }

    if let Some(rl) = rate_limit {
        report.rate_limit_remaining = Some(rl.remaining);
        report.rate_limit_limit = Some(rl.limit);
    }

    // Build a human-readable status message. We need to pass the intermediate
    // map representation for `build_status_message` which reads named fields.
    // Serialize to Value first, extract the map, build the message, then set it.
    let mut value = serde_json::to_value(&report).unwrap_or(serde_json::Value::Null);
    let message = build_status_message(
        &cursor,
        phase,
        value.as_object().unwrap_or(&serde_json::Map::new()),
        rate_limit,
    );
    if let Some(obj) = value.as_object_mut() {
        obj.insert("status_message".into(), serde_json::json!(message));
    }

    value
}

/// Build a human-readable status message from the current state.
fn build_status_message(
    cursor: &serde_json::Value,
    phase: &str,
    progress: &serde_json::Map<String, serde_json::Value>,
    rate_limit: Option<&RateLimitInfo>,
) -> String {
    // Check for rate limit pressure first.
    if let Some(rl) = rate_limit
        && rl.remaining < 100
    {
        return format!(
            "Rate limited — only {} API calls remaining, resets in {}m",
            rl.remaining,
            ((rl.reset_at - time::OffsetDateTime::now_utc()).whole_minutes()).max(1)
        );
    }

    match phase {
        "TeamRepos" => {
            let current_repo = progress
                .get("current_repo")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let repos_completed = progress
                .get("repos_completed")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let repos_total = progress
                .get("repos_total")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);

            let watermark = cursor
                .get("watermark")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let since = if watermark.is_empty() {
                "all time".into()
            } else {
                // Extract just the date part for readability.
                watermark.split('T').next().unwrap_or(watermark).to_string()
            };

            format!(
                "Fetching PRs updated since {since} from {current_repo} ({}/{})",
                repos_completed + 1,
                repos_total
            )
        }
        "MemberSearch" => {
            let search_users = cursor.get("search_users").and_then(|v| v.as_array());
            let search_user_index = cursor
                .get("search_user_index")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;

            let total = search_users.map_or(0, Vec::len);
            let batch_end = (search_user_index + 5).min(total); // matches SEARCH_BATCH_SIZE

            // Show the first user in the batch for context.
            let first_user = search_users
                .and_then(|users| users.get(search_user_index))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            if batch_end - search_user_index > 1 {
                format!(
                    "Searching cross-repo PRs for {first_user} + {} others ({}/{})",
                    batch_end - search_user_index - 1,
                    batch_end,
                    total
                )
            } else {
                format!("Searching cross-repo PRs for {first_user} ({batch_end}/{total})")
            }
        }
        _ => "Starting ingestion".into(),
    }
}
