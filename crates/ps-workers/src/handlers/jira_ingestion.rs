use ps_core::ingestion::{ContributionInput, IngestionPlan};
use ps_core::models::{ContributionType, RateLimitInfo, SourceConfig};
use restate_sdk::prelude::*;
use tracing::info;

use super::SharedState;
use super::ingestion_common::{
    ProgressTracker, build_ingestion_context, create_ingestion_run, decrypt_optional_secret,
    decrypt_required_secret, extract_failed_items, fail_ingestion_run, finalise_run,
    load_source_config,
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

        let initial_cursor = source.initial_cursor(&ing_ctx, &plan);

        let mut tracker = JiraProgressTracker::default();
        let result = super::ingestion_common::fetch_store_loop(
            ctx,
            &ing_ctx,
            run_id,
            source_name,
            &initial_cursor,
            &mut tracker,
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

        let failed_items = extract_failed_items(&final_cursor);
        finalise_run(
            ctx,
            &self.state.repos,
            &ing_ctx,
            run_id,
            source_name,
            total_items,
            &failed_items,
            "project",
            &final_cursor,
            source.watermark_field(),
        )
        .await?;

        if total_items > 0 {
            info!(source = source_name, "triggering metrics recomputation");
            ctx.service_client::<MetricsComputeHandlerClient>()
                .compute_current_periods()
                .send();
        }

        info!(source = source_name, total_items, "Jira ingestion complete");
        Ok(())
    }
}

#[derive(Default)]
struct JiraProgressTracker {
    tickets_fetched: u32,
}

impl ProgressTracker for JiraProgressTracker {
    fn count_batch(&mut self, items: &[ContributionInput], _stored: i32) {
        for item in items {
            if item.contribution_type == ContributionType::JiraTicket {
                self.tickets_fetched += 1;
            }
        }
    }

    fn build_progress(
        &self,
        cursor_json: &str,
        rate_limit: Option<&RateLimitInfo>,
    ) -> serde_json::Value {
        build_progress_json(cursor_json, self.tickets_fetched, rate_limit)
    }

    fn build_final_progress(&self) -> serde_json::Value {
        serde_json::json!({
            "phase": "complete",
            "tickets_fetched": self.tickets_fetched,
        })
    }
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
