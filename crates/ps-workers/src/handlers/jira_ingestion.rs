use ps_core::ingestion::{FailedItem, IngestionPlan};
use ps_core::models::{ContributionType, RateLimitInfo, SourceConfig};
use restate_sdk::prelude::*;
use tracing::{info, warn};
use uuid::Uuid;

use super::SharedState;
use super::ingestion_common::{
    advance_watermark, build_ingestion_context, complete_ingestion_run,
    complete_ingestion_run_with_warnings, create_ingestion_run, decrypt_optional_secret,
    decrypt_required_secret, fail_ingestion_run, fetch_batch, load_source_config, store_batch,
};
use super::metrics_compute::MetricsComputeHandlerClient;
use crate::registry;

pub struct JiraIngestionHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::object]
pub trait JiraIngestionHandler {
    /// Run an incremental ingestion for the Jira source identified by the object key.
    async fn run_ingestion() -> Result<(), TerminalError>;

    /// Run a backfill from a specific date.
    async fn backfill(since_date: String) -> Result<(), TerminalError>;
}

impl JiraIngestionHandler for JiraIngestionHandlerImpl {
    async fn run_ingestion(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let source_type_key = ctx.key().to_string();
        let config = load_source_config(&ctx, &self.state.repos, &source_type_key).await?;
        let source_name = config.name.clone();
        info!(source = %source_name, "starting Jira ingestion run");

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
        info!(source = %source_name, since = %since_date, "starting Jira backfill");

        self.execute_ingestion(&ctx, &source_name, &config, Some(since_date))
            .await
    }
}

impl JiraIngestionHandlerImpl {
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
            "JiraIngestionHandler",
            method,
        )
        .await?;

        // Decrypt token and email once per run, outside ctx.run() to avoid journaling
        let token = decrypt_required_secret(&self.state, config.id, "api_token").await?;
        let email = decrypt_optional_secret(&self.state, config.id, "email").await?;

        let ing_ctx = build_ingestion_context(&self.state, config, Some(token), email, None);

        let mut plan: IngestionPlan = match source.plan(&ing_ctx).await {
            Ok(p) => p,
            Err(e) => {
                let msg = e.to_string();
                fail_ingestion_run(ctx, &self.state.repos, run_id, source_name, &msg).await;
                return Err(TerminalError::new(format!("plan failed: {msg}")));
            }
        };

        if let Some(ref wm) = override_watermark {
            plan.watermark = Some(wm.clone());
        }

        info!(
            source = source_name,
            projects = ?plan.items,
            watermark = ?plan.watermark,
            "Jira ingestion plan ready"
        );

        // Build the initial cursor with full Jira config
        let initial_cursor = build_jira_cursor(config, &plan);

        let result = self
            .fetch_store_loop(
                ctx,
                run_id,
                source_name,
                config,
                &initial_cursor,
                ing_ctx.token.as_deref(),
            )
            .await;

        let (total_items, final_cursor) = match result {
            Ok(v) => v,
            Err(e) => {
                let msg = e.to_string();
                fail_ingestion_run(ctx, &self.state.repos, run_id, source_name, &msg).await;
                return Err(TerminalError::new(format!("ingestion failed: {msg}")));
            }
        };

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
                "all {} project(s) failed: {}",
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
                "{} project(s) failed: {}",
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

        info!(source = source_name, total_items, "Jira ingestion complete");
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn fetch_store_loop(
        &self,
        ctx: &ObjectContext<'_>,
        run_id: Uuid,
        source_name: &str,
        config: &SourceConfig,
        initial_cursor: &str,
        token: Option<&str>,
    ) -> Result<(i32, String), TerminalError> {
        let mut cursor = initial_cursor.to_string();
        let mut total_items = 0i32;
        let mut tickets_fetched = 0u32;

        loop {
            let batch = fetch_batch(&self.state, config, &cursor, token).await?;

            for item in &batch.items {
                if item.contribution_type == ContributionType::JiraTicket {
                    tickets_fetched += 1;
                }
            }

            // The fetch always carries the latest cursor state in etag
            // (including updated max_updated_at) for watermark advancement.
            if let Some(ref latest) = batch.etag {
                cursor = latest.clone();
            }

            if !batch.items.is_empty() {
                let stored = store_batch(ctx, &self.state, config, &batch.items, token).await?;
                total_items += stored;

                info!(
                    source = source_name,
                    batch_stored = stored,
                    total_items,
                    "stored Jira batch"
                );
            }

            // Build progress JSON
            let progress = build_progress_json(&cursor, tickets_fetched, batch.rate_limit.as_ref());

            if let Err(e) = self
                .state
                .repos
                .activity
                .update_run_progress_detail(run_id, total_items, &progress)
                .await
            {
                warn!(source = source_name, "failed to update run progress: {e}");
            }

            if batch.next_cursor.is_none() {
                break;
            }
        }

        // Final progress
        let final_progress = serde_json::json!({
            "phase": "complete",
            "tickets_fetched": tickets_fetched,
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

/// Build the initial Jira cursor with full config.
fn build_jira_cursor(config: &SourceConfig, plan: &IngestionPlan) -> String {
    let settings = &config.settings;

    let projects: Vec<String> = settings
        .get("projects")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let base_url = settings
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();

    let story_points_field = settings
        .get("story_points_field")
        .and_then(serde_json::Value::as_str)
        .map(String::from);

    let api_mode = settings
        .get("api_mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("cloud")
        .to_string();

    let cursor = crate::jira::source::Cursor {
        watermark: plan.watermark.clone(),
        projects,
        project_index: 0,
        next_page_token: None,
        max_updated_at: plan.watermark.clone(),
        base_url,
        story_points_field,
        api_mode,
        failed_items: vec![],
    };

    serde_json::to_string(&cursor).unwrap_or_default()
}

/// Build a structured progress JSON for the Jira ingestion run.
fn build_progress_json(
    cursor_json: &str,
    tickets_fetched: u32,
    rate_limit: Option<&RateLimitInfo>,
) -> serde_json::Value {
    let cursor: serde_json::Value =
        serde_json::from_str(cursor_json).unwrap_or(serde_json::Value::Null);

    let projects_total = cursor
        .get("projects")
        .and_then(|v| v.as_array())
        .map_or(0, Vec::len);
    let project_index = cursor
        .get("project_index")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let current_project = cursor
        .get("projects")
        .and_then(|v| v.as_array())
        .and_then(|ps| ps.get(project_index as usize))
        .and_then(serde_json::Value::as_str);
    let failed_count = cursor
        .get("failed_items")
        .and_then(|v| v.as_array())
        .map_or(0, Vec::len);

    let status_message = if let Some(proj) = current_project {
        format!(
            "Fetching Jira issues from {proj} ({}/{projects_total}, {tickets_fetched} so far)",
            project_index + 1
        )
    } else if projects_total > 0 {
        format!("Jira ingestion complete ({tickets_fetched} tickets)")
    } else {
        format!("Fetching Jira issues ({tickets_fetched} so far)")
    };

    let mut progress = serde_json::json!({
        "phase": current_project.map_or("complete".to_string(), |p| format!("project:{p}")),
        "tickets_fetched": tickets_fetched,
        "projects_total": projects_total,
        "projects_completed": project_index,
        "current_project": current_project,
        "failed_items": failed_count,
        "status_message": status_message,
    });

    if let Some(rl) = rate_limit
        && let Some(obj) = progress.as_object_mut()
    {
        obj.insert(
            "rate_limit_remaining".into(),
            serde_json::json!(rl.remaining),
        );
        obj.insert("rate_limit_limit".into(), serde_json::json!(rl.limit));
    }

    progress
}
