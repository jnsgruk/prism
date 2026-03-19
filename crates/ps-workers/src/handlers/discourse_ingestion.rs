use ps_core::ingestion::{ContributionInput, IngestionPlan};
use ps_core::models::{RateLimitInfo, SourceConfig};
use restate_sdk::prelude::*;
use tracing::info;

use super::SharedState;
use super::identity_resolution::IdentityResolutionHandlerClient;
use super::ingestion_common::{
    ProgressTracker, build_ingestion_context, create_ingestion_run, decrypt_optional_secret,
    extract_failed_items, fail_ingestion_run, finalise_run, load_source_config,
};
use super::metrics_compute::MetricsComputeHandlerClient;
use crate::registry;

pub struct DiscourseIngestionHandlerImpl {
    pub state: SharedState,
}

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
        info!(source = %source_name, since = %since_date, "starting Discourse backfill");

        self.execute_ingestion(&ctx, &source_name, &config, Some(since_date))
            .await
    }
}

impl DiscourseIngestionHandlerImpl {
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
            "DiscourseIngestionHandler",
            method,
        )
        .await?;

        // Decrypt API key and username once per run, outside ctx.run() to avoid journaling.
        // API key is optional — Discourse public endpoints work without auth.
        let api_key = decrypt_optional_secret(&self.state, config.id, "api_key").await?;
        let api_username = decrypt_optional_secret(&self.state, config.id, "api_username").await?;

        let ing_ctx = build_ingestion_context(&self.state, config, api_key, None, api_username);

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
            watermark = ?plan.watermark,
            "Discourse ingestion plan ready"
        );

        let initial_cursor = source.initial_cursor(&ing_ctx, &plan);

        let mut tracker = DiscourseProgressTracker::default();
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
            "category",
            &final_cursor,
            source.watermark_field(),
        )
        .await?;

        if total_items > 0 {
            info!(source = source_name, "triggering metrics recomputation");
            ctx.service_client::<MetricsComputeHandlerClient>()
                .compute_current_periods()
                .send();

            // Trigger identity resolution across all Discourse sources.
            info!(source = source_name, "triggering identity resolution");
            ctx.service_client::<IdentityResolutionHandlerClient>()
                .resolve_identities()
                .send();
        }

        info!(
            source = source_name,
            total_items, "Discourse ingestion complete"
        );
        Ok(())
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
        _cursor_json: &str,
        rate_limit: Option<&RateLimitInfo>,
    ) -> serde_json::Value {
        build_progress_json(_cursor_json, self.topics_fetched, rate_limit)
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
