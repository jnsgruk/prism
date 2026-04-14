use ps_core::ingestion::ContributionInput;
use ps_core::models::{ContributionType, RateLimitInfo, SecretKey};
use restate_sdk::prelude::*;
use serde::Serialize;
use tracing::info;

use crate::features::ingestion::lib::{
    IngestionSpec, ProgressTracker, execute_ingestion, load_ingestion_source_config,
};
use crate::infra::SharedState;

pub struct GithubIngestionHandlerImpl {
    pub state: SharedState,
}

const GITHUB_SPEC: IngestionSpec = IngestionSpec {
    handler_name: "GithubIngestionHandler",
    token_key: Some(SecretKey::ApiToken),
    token_required: true,
    email_key: None,
    api_username_key: None,
    item_noun: "repo",
};

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
        let config =
            load_ingestion_source_config(&ctx, &self.state.repos, &source_type_key).await?;
        let source_name = config.name.clone();
        info!(source = %source_name, "starting ingestion run");

        let mut tracker = GithubProgressTracker::default();
        execute_ingestion(
            &ctx,
            &self.state,
            &GITHUB_SPEC,
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
        info!(source = %source_name, since = %since_date, "starting backfill");

        let mut tracker = GithubProgressTracker::default();
        execute_ingestion(
            &ctx,
            &self.state,
            &GITHUB_SPEC,
            &source_name,
            &config,
            Some(since_date),
            &mut tracker,
            |_ctx| {},
        )
        .await
    }
}

#[derive(Default)]
struct GithubProgressTracker {
    prs_fetched: u32,
    reviews_fetched: u32,
    identities_skipped: u32,
}

impl ProgressTracker for GithubProgressTracker {
    fn count_batch(&mut self, items: &[ContributionInput], stored: i32) {
        for item in items {
            match item.contribution_type {
                ContributionType::PullRequest => self.prs_fetched += 1,
                ContributionType::PrReview => self.reviews_fetched += 1,
                _ => {}
            }
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            self.identities_skipped += (items.len() as u32).saturating_sub(stored as u32);
        }
    }

    fn build_progress(
        &self,
        cursor_json: &str,
        rate_limit: Option<&RateLimitInfo>,
    ) -> serde_json::Value {
        build_progress_json(
            cursor_json,
            self.prs_fetched,
            self.reviews_fetched,
            self.identities_skipped,
            rate_limit,
        )
    }

    fn build_final_progress(&self) -> serde_json::Value {
        serde_json::json!({
            "phase": "complete",
            "prs_fetched": self.prs_fetched,
            "reviews_fetched": self.reviews_fetched,
            "identities_skipped": self.identities_skipped,
        })
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

#[cfg(test)]
mod tests {
    use ps_core::ingestion::ContributionInput;
    use ps_core::models::{ContributionState, ContributionType, Platform};
    use time::OffsetDateTime;

    use super::*;

    fn make_item(ct: ContributionType) -> ContributionInput {
        ContributionInput {
            platform: Platform::Github,
            contribution_type: ct,
            platform_id: "test-1".into(),
            platform_username: "user".into(),
            title: Some("test".into()),
            url: None,
            state: Some(ContributionState::Merged),
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
    fn github_progress_counts_prs_and_reviews() {
        let mut tracker = GithubProgressTracker::default();
        let items = vec![
            make_item(ContributionType::PullRequest),
            make_item(ContributionType::PullRequest),
            make_item(ContributionType::PrReview),
        ];
        tracker.count_batch(&items, 3);
        assert_eq!(tracker.prs_fetched, 2);
        assert_eq!(tracker.reviews_fetched, 1);
        assert_eq!(tracker.identities_skipped, 0);
    }

    #[test]
    fn github_progress_counts_skipped_identities() {
        let mut tracker = GithubProgressTracker::default();
        let items = vec![
            make_item(ContributionType::PullRequest),
            make_item(ContributionType::PrReview),
        ];
        // Only 1 stored out of 2 items → 1 identity skipped
        tracker.count_batch(&items, 1);
        assert_eq!(tracker.identities_skipped, 1);
    }

    #[test]
    fn github_final_progress_includes_counts() {
        let mut tracker = GithubProgressTracker::default();
        let items = vec![make_item(ContributionType::PullRequest)];
        tracker.count_batch(&items, 1);

        let progress = tracker.build_final_progress();
        assert_eq!(progress["phase"], "complete");
        assert_eq!(progress["prs_fetched"], 1);
        assert_eq!(progress["reviews_fetched"], 0);
    }

    #[test]
    fn github_build_progress_team_repos_phase() {
        let cursor = serde_json::json!({
            "phase": "TeamRepos",
            "repo_index": 1,
            "repos": [
                {"owner": "canonical", "repo": "lxd"},
                {"owner": "canonical", "repo": "juju"}
            ],
            "watermark": "2025-01-01T00:00:00Z"
        });
        let progress = build_progress_json(&cursor.to_string(), 5, 2, 0, None);
        assert_eq!(progress["phase"], "team_repos");
        assert_eq!(progress["repos_total"], 2);
        assert_eq!(progress["repos_completed"], 1);
        assert_eq!(progress["current_repo"], "canonical/juju");
        let msg = progress["status_message"].as_str().unwrap();
        assert!(msg.contains("canonical/juju"));
        assert!(msg.contains("2/2"));
    }

    #[test]
    fn github_build_progress_member_search_phase() {
        let cursor = serde_json::json!({
            "phase": "MemberSearch",
            "search_users": ["alice", "bob", "carol"],
            "search_user_index": 0
        });
        let progress = build_progress_json(&cursor.to_string(), 10, 3, 0, None);
        assert_eq!(progress["phase"], "member_search");
        assert_eq!(progress["search_users_total"], 3);
        let msg = progress["status_message"].as_str().unwrap();
        assert!(msg.contains("alice"));
    }
}
