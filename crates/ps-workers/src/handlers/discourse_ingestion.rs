use ps_core::ingestion::ContributionInput;
use ps_core::models::RateLimitInfo;
use restate_sdk::prelude::*;
use tracing::info;

use super::SharedState;
use super::identity_resolution::IdentityResolutionHandlerClient;
use super::ingestion_common::{
    IngestionSpec, ProgressTracker, execute_ingestion, load_source_config,
};
use super::metrics_compute::MetricsComputeHandlerClient;

pub struct DiscourseIngestionHandlerImpl {
    pub state: SharedState,
}

const DISCOURSE_SPEC: IngestionSpec = IngestionSpec {
    handler_name: "DiscourseIngestionHandler",
    token_key: Some("api_key"),
    token_required: false,
    email_key: None,
    api_username_key: Some("api_username"),
    item_noun: "category",
};

#[restate_sdk::object]
pub trait DiscourseIngestionHandler {
    /// Run an incremental ingestion for the Discourse source identified by the object key.
    async fn run_ingestion() -> Result<(), TerminalError>;

    /// Run a backfill from a specific date.
    async fn backfill(since_date: String) -> Result<(), TerminalError>;
}

impl DiscourseIngestionHandler for DiscourseIngestionHandlerImpl {
    async fn run_ingestion(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let source_type_key = ctx.key().to_string();
        let config = load_source_config(&ctx, &self.state.repos, &source_type_key).await?;
        let source_name = config.name.clone();
        info!(source = %source_name, "starting Discourse ingestion run");

        let mut tracker = DiscourseProgressTracker::default();
        execute_ingestion(
            &ctx,
            &self.state,
            &DISCOURSE_SPEC,
            &source_name,
            &config,
            None,
            &mut tracker,
            |ctx| {
                ctx.service_client::<MetricsComputeHandlerClient>()
                    .compute_current_periods()
                    .send();
                ctx.service_client::<IdentityResolutionHandlerClient>()
                    .resolve_identities()
                    .send();
            },
        )
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
        info!(source = %source_name, since = %since_date, "starting Discourse backfill");

        let mut tracker = DiscourseProgressTracker::default();
        execute_ingestion(
            &ctx,
            &self.state,
            &DISCOURSE_SPEC,
            &source_name,
            &config,
            Some(since_date),
            &mut tracker,
            |ctx| {
                ctx.service_client::<MetricsComputeHandlerClient>()
                    .compute_current_periods()
                    .send();
                ctx.service_client::<IdentityResolutionHandlerClient>()
                    .resolve_identities()
                    .send();
            },
        )
        .await
    }
}

#[derive(Default)]
struct DiscourseProgressTracker {
    topics_fetched: u32,
}

impl ProgressTracker for DiscourseProgressTracker {
    fn count_batch(&mut self, items: &[ContributionInput], _stored: i32) {
        #[allow(clippy::cast_possible_truncation)]
        {
            self.topics_fetched += items.len() as u32;
        }
    }

    fn build_progress(
        &self,
        cursor_json: &str,
        rate_limit: Option<&RateLimitInfo>,
    ) -> serde_json::Value {
        build_progress_json(cursor_json, self.topics_fetched, rate_limit)
    }

    fn build_final_progress(&self) -> serde_json::Value {
        serde_json::json!({
            "phase": "complete",
            "topics_fetched": self.topics_fetched,
        })
    }
}

/// Build a structured progress JSON for the Discourse ingestion run.
fn build_progress_json(
    _cursor_json: &str,
    topics_fetched: u32,
    rate_limit: Option<&RateLimitInfo>,
) -> serde_json::Value {
    let status_message = format!("Fetching Discourse topics ({topics_fetched} items so far)");

    let mut progress = serde_json::json!({
        "phase": "topic_scan",
        "topics_fetched": topics_fetched,
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
