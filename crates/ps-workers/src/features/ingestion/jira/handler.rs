use ps_core::ingestion::ContributionInput;
use ps_core::models::{ContributionType, RateLimitInfo, SecretKey};
use restate_sdk::prelude::*;
use tracing::info;

use crate::features::ingestion::lib::{
    IngestionSpec, ProgressTracker, execute_ingestion, load_ingestion_source_config,
    trigger_enrichment_and_embedding,
};
use crate::infra::SharedState;

pub struct JiraIngestionHandlerImpl {
    pub state: SharedState,
}

const JIRA_SPEC: IngestionSpec = IngestionSpec {
    handler_name: "JiraIngestionHandler",
    token_key: Some(SecretKey::ApiToken),
    token_required: true,
    email_key: Some(SecretKey::Email),
    api_username_key: None,
    item_noun: "project",
};

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
        let config =
            load_ingestion_source_config(&ctx, &self.state.repos, &source_type_key).await?;
        let source_name = config.name.clone();
        info!(source = %source_name, "starting Jira ingestion run");

        let mut tracker = JiraProgressTracker::default();
        execute_ingestion(
            &ctx,
            &self.state,
            &JIRA_SPEC,
            &source_name,
            &config,
            None,
            &mut tracker,
            |_ctx| {},
        )
        .await
    }

    async fn backfill(
        &self,
        ctx: ObjectContext<'_>,
        since_date: String,
    ) -> Result<(), TerminalError> {
        let source_type_key = ctx.key().to_string();
        let config =
            load_ingestion_source_config(&ctx, &self.state.repos, &source_type_key).await?;
        let source_name = config.name.clone();
        info!(source = %source_name, since = %since_date, "starting Jira backfill");

        let mut tracker = JiraProgressTracker::default();
        execute_ingestion(
            &ctx,
            &self.state,
            &JIRA_SPEC,
            &source_name,
            &config,
            Some(since_date),
            &mut tracker,
            trigger_enrichment_and_embedding,
        )
        .await
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

#[cfg(test)]
mod tests {
    use ps_core::ingestion::ContributionInput;
    use ps_core::models::{ContributionState, ContributionType, Platform};
    use time::OffsetDateTime;

    use super::*;

    fn make_jira_item() -> ContributionInput {
        ContributionInput {
            platform: Platform::Jira,
            contribution_type: ContributionType::JiraTicket,
            platform_id: "PROJ-1".into(),
            platform_username: "user".into(),
            title: Some("test ticket".into()),
            url: None,
            state: Some(ContributionState::Closed),
            created_at: OffsetDateTime::now_utc(),
            updated_at: None,
            closed_at: None,
            metrics: serde_json::json!({}),
            metadata: serde_json::json!({}),
            content: None,
            state_history: None,
            enrichment_content: None,
        }
    }

    #[test]
    fn jira_progress_counts_tickets() {
        let mut tracker = JiraProgressTracker::default();
        let items = vec![make_jira_item(), make_jira_item(), make_jira_item()];
        tracker.count_batch(&items, 3);
        assert_eq!(tracker.tickets_fetched, 3);
    }

    #[test]
    fn jira_final_progress() {
        let mut tracker = JiraProgressTracker::default();
        tracker.count_batch(&[make_jira_item()], 1);
        let progress = tracker.build_final_progress();
        assert_eq!(progress["phase"], "complete");
        assert_eq!(progress["tickets_fetched"], 1);
    }

    #[test]
    fn jira_build_progress_shows_project() {
        let cursor = serde_json::json!({
            "projects": ["PROJ-A", "PROJ-B"],
            "project_index": 0,
            "failed_items": []
        });
        let progress = build_progress_json(&cursor.to_string(), 5, None);
        assert_eq!(progress["current_project"], "PROJ-A");
        assert_eq!(progress["projects_total"], 2);
        let msg = progress["status_message"].as_str().unwrap();
        assert!(msg.contains("PROJ-A"));
        assert!(msg.contains("1/2"));
        assert!(msg.contains("5 so far"));
    }

    #[test]
    fn jira_build_progress_complete_phase() {
        // project_index past the end of projects array
        let cursor = serde_json::json!({
            "projects": ["X"],
            "project_index": 1,
            "failed_items": []
        });
        let progress = build_progress_json(&cursor.to_string(), 10, None);
        assert_eq!(progress["phase"], "complete");
        let msg = progress["status_message"].as_str().unwrap();
        assert!(msg.contains("complete"));
    }
}
