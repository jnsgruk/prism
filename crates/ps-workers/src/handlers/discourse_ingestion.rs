use ps_core::ingestion::IngestionPlan;
use ps_core::models::{RateLimitInfo, SourceConfig};
use restate_sdk::prelude::*;
use tracing::{info, warn};
use uuid::Uuid;

use super::SharedState;
use super::identity_resolution::IdentityResolutionHandlerClient;
use super::ingestion_common::{
    advance_watermark, build_ingestion_context, complete_ingestion_run, create_ingestion_run,
    decrypt_optional_secret, fail_ingestion_run, fetch_batch, load_source_config, store_batch,
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

        let initial_cursor = build_discourse_cursor(config, &plan);

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

        if total_items > 0
            && let Err(e) = advance_watermark(
                ctx,
                &self.state,
                config,
                &final_cursor,
                total_items,
                ing_ctx.token.as_deref(),
                "max_bumped_at",
            )
            .await
        {
            let msg = e.to_string();
            fail_ingestion_run(ctx, &self.state.repos, run_id, source_name, &msg).await;
            return Err(TerminalError::new(format!(
                "watermark advance failed: {msg}"
            )));
        }

        complete_ingestion_run(ctx, &self.state.repos, run_id, source_name, total_items).await;

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
        let mut topics_fetched = 0u32;

        loop {
            let batch = fetch_batch(&self.state, config, &cursor, token).await?;

            topics_fetched += batch.items.len() as u32;

            // The fetch always carries the latest cursor state in etag
            // (including updated max_bumped_at) for watermark advancement.
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
                    "stored Discourse batch"
                );
            }

            let progress = build_progress_json(&cursor, topics_fetched, batch.rate_limit.as_ref());

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

        let final_progress = serde_json::json!({
            "phase": "complete",
            "topics_fetched": topics_fetched,
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

/// Build the initial Discourse cursor with full config.
fn build_discourse_cursor(config: &SourceConfig, plan: &IngestionPlan) -> String {
    let settings = &config.settings;

    let base_url = settings
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();

    let categories: Vec<i64> = settings
        .get("categories")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let min_posts = settings
        .get("min_posts")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0) as i32;

    let instance = crate::discourse::source::extract_instance(&config.name);

    let cursor = crate::discourse::source::Cursor {
        watermark: plan.watermark.clone(),
        page: 0,
        category_ids: categories,
        min_posts,
        base_url,
        instance,
        max_bumped_at: plan.watermark.clone(),
        has_more: true,
        category_map: std::collections::HashMap::new(),
    };

    serde_json::to_string(&cursor).unwrap_or_default()
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
