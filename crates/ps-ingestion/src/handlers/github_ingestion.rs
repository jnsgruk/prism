use ps_core::ingestion::IngestionContext;
use ps_core::models::SourceConfig;
use restate_sdk::prelude::*;
use tracing::{error, info, warn};
use uuid::Uuid;

use super::SharedState;
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
        let source_name = ctx.key().to_string();
        info!(source = %source_name, "starting ingestion run");

        self.execute_ingestion(&ctx, &source_name, None).await
    }

    async fn backfill(
        &self,
        ctx: ObjectContext<'_>,
        since_date: String,
    ) -> Result<(), TerminalError> {
        let source_name = ctx.key().to_string();
        info!(source = %source_name, since = %since_date, "starting backfill");

        self.execute_ingestion(&ctx, &source_name, Some(since_date))
            .await
    }
}

impl GithubIngestionHandlerImpl {
    #[allow(clippy::too_many_lines)]
    async fn execute_ingestion(
        &self,
        ctx: &ObjectContext<'_>,
        source_name: &str,
        override_watermark: Option<String>,
    ) -> Result<(), TerminalError> {
        // Step 1: Load source config from DB
        let repos = self.state.repos.clone();
        let name = source_name.to_string();
        let config: SourceConfig = ctx
            .run(|| {
                let repos = repos.clone();
                let name = name.clone();
                async move {
                    let row = repos
                        .config
                        .get_enabled_source_by_name(&name)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?
                        .ok_or_else(|| {
                            TerminalError::new(format!("source '{name}' not found or disabled"))
                        })?;

                    Ok(Json::from(row))
                }
            })
            .name("load_config")
            .await?
            .into_inner();

        // Step 2: Create source adapter
        let source = registry::create_source(&config.source_type).ok_or_else(|| {
            TerminalError::new(format!("unsupported source type: {}", config.source_type))
        })?;

        // Build the IngestionContext
        let ing_ctx = IngestionContext {
            repos: self.state.repos.clone(),
            source_config: config.clone(),
            secret_key: self.state.secret_key,
            http_client: self.state.http_client.clone(),
        };

        // Step 3: Create ingestion run record
        // Generate run_id inside the side effect so it survives Restate replays.
        let repos = self.state.repos.clone();
        let sn = source_name.to_string();
        let method = if override_watermark.is_some() {
            "backfill"
        } else {
            "run_ingestion"
        };
        let method_owned = method.to_string();
        let run_id: Uuid = ctx
            .run(|| {
                let repos = repos.clone();
                let sn = sn.clone();
                let method_owned = method_owned.clone();
                async move {
                    let id = Uuid::now_v7();
                    repos
                        .activity
                        .create_run(id, &sn, "GithubIngestionHandler", &method_owned)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(id.to_string()))
                }
            })
            .name("create_run")
            .await?
            .into_inner()
            .parse()
            .map_err(|e| TerminalError::new(format!("invalid run_id: {e}")))?;

        // Step 4: Plan
        let plan = match source.plan(&ing_ctx).await {
            Ok(p) => p,
            Err(e) => {
                self.fail_run(ctx, run_id, source_name, &e.to_string())
                    .await;
                return Err(TerminalError::new(format!("plan failed: {e}")));
            }
        };

        // Override watermark for backfill
        let mut plan = plan;
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
            self.complete_run(ctx, run_id, source_name, 0).await;
            return Ok(());
        }

        // Step 5: Fetch/Store/Advance loop
        let mut cursor = source.initial_cursor(&plan);
        let mut total_items = 0i32;

        loop {
            // Fetch batch (as a durable side effect)
            let repos = self.state.repos.clone();
            let http = self.state.http_client.clone();
            let cfg = config.clone();
            let sk = self.state.secret_key;
            let cur = cursor.clone();
            let source_type = config.source_type.clone();

            let fetch_result = ctx
                .run(|| {
                    let repos = repos.clone();
                    let http = http.clone();
                    let cfg = cfg.clone();
                    let cur = cur.clone();
                    let source_type = source_type.clone();
                    async move {
                        let src = registry::create_source(&source_type)
                            .ok_or_else(|| TerminalError::new("source unavailable"))?;
                        let ic = IngestionContext {
                            repos,
                            source_config: cfg,
                            secret_key: sk,
                            http_client: http,
                        };
                        let result = src
                            .fetch_batch(&ic, &cur)
                            .await
                            .map_err(|e| TerminalError::new(format!("fetch failed: {e}")))?;

                        // Serialise for Restate journaling
                        let serialised = serde_json::to_value(&SerFetchResult {
                            items: result.items,
                            next_cursor: result.next_cursor,
                        })
                        .map_err(|e| TerminalError::new(format!("serialise error: {e}")))?;

                        Ok(Json::from(serialised))
                    }
                })
                .name("fetch_batch")
                .await?
                .into_inner();

            let batch: SerFetchResult = serde_json::from_value(fetch_result)
                .map_err(|e| TerminalError::new(format!("deserialise error: {e}")))?;

            // Store batch
            if !batch.items.is_empty() {
                let repos = self.state.repos.clone();
                let http = self.state.http_client.clone();
                let cfg = config.clone();
                let sk = self.state.secret_key;
                let items = batch.items.clone();
                let source_type = config.source_type.clone();

                let stored: i32 = ctx
                    .run(|| {
                        let repos = repos.clone();
                        let http = http.clone();
                        let cfg = cfg.clone();
                        let items = items.clone();
                        let source_type = source_type.clone();
                        async move {
                            let src = registry::create_source(&source_type)
                                .ok_or_else(|| TerminalError::new("source unavailable"))?;
                            let ic = IngestionContext {
                                repos,
                                source_config: cfg,
                                secret_key: sk,
                                http_client: http,
                            };
                            let count = src
                                .store_batch(&ic, &items)
                                .await
                                .map_err(|e| TerminalError::new(format!("store failed: {e}")))?;
                            #[allow(clippy::cast_possible_wrap)]
                            Ok(Json::from(count as i32))
                        }
                    })
                    .name("store_batch")
                    .await?
                    .into_inner();

                total_items += stored;

                info!(
                    source = source_name,
                    batch_stored = stored,
                    total_items,
                    "stored batch"
                );

                // Update progress on the run record (best-effort, outside Restate journal)
                if let Err(e) = self
                    .state
                    .repos
                    .activity
                    .update_run_progress(run_id, total_items)
                    .await
                {
                    warn!(source = source_name, "failed to update run progress: {e}");
                }
            }

            // Check if we're done
            let Some(next_cursor) = batch.next_cursor else {
                break;
            };
            cursor = next_cursor;

            // TODO: check rate limit and ctx.sleep() if needed
            // For now, basic adaptive throttling is handled by the source adapter
        }

        // Step 6: Advance watermark
        if total_items > 0 {
            let repos = self.state.repos.clone();
            let http = self.state.http_client.clone();
            let cfg = config.clone();
            let sk = self.state.secret_key;
            let wm = cursor.clone();
            let source_type = config.source_type.clone();
            let ti = total_items;

            ctx.run(|| {
                let repos = repos.clone();
                let http = http.clone();
                let cfg = cfg.clone();
                let wm = wm.clone();
                let source_type = source_type.clone();
                async move {
                    let src = registry::create_source(&source_type)
                        .ok_or_else(|| TerminalError::new("source unavailable"))?;
                    let ic = IngestionContext {
                        repos,
                        source_config: cfg,
                        secret_key: sk,
                        http_client: http,
                    };
                    // Extract the max_updated_at from the cursor as the watermark value
                    let watermark = extract_watermark(&wm).unwrap_or_default();
                    src.advance_watermark(&ic, &watermark, ti)
                        .await
                        .map_err(|e| TerminalError::new(format!("advance failed: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("advance_watermark")
            .await?;
        }

        // Step 7: Complete run
        self.complete_run(ctx, run_id, source_name, total_items)
            .await;

        info!(source = source_name, total_items, "ingestion complete");
        Ok(())
    }

    async fn complete_run(
        &self,
        ctx: &ObjectContext<'_>,
        run_id: Uuid,
        source_name: &str,
        items_collected: i32,
    ) {
        let repos = self.state.repos.clone();
        let sn = source_name.to_string();
        let result = ctx
            .run(|| {
                let repos = repos.clone();
                let sn = sn.clone();
                async move {
                    repos
                        .activity
                        .complete_run(run_id, items_collected)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    repos
                        .activity
                        .clear_current_invocation_id(&sn)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("complete_run")
            .await;

        if let Err(e) = result {
            error!(source = source_name, "failed to update run status: {e}");
        }
    }

    async fn fail_run(
        &self,
        ctx: &ObjectContext<'_>,
        run_id: Uuid,
        source_name: &str,
        error_msg: &str,
    ) {
        let repos = self.state.repos.clone();
        let err = error_msg.to_string();
        let sn = source_name.to_string();
        let result = ctx
            .run(|| {
                let repos = repos.clone();
                let err = err.clone();
                let sn = sn.clone();
                async move {
                    repos
                        .activity
                        .fail_run(run_id, &err)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    repos
                        .activity
                        .clear_current_invocation_id(&sn)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("fail_run")
            .await;

        if let Err(e) = result {
            error!(source = source_name, "failed to update run status: {e}");
        }
    }
}

/// Serialisable fetch result for Restate journaling.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SerFetchResult {
    items: Vec<ps_core::ingestion::ContributionInput>,
    next_cursor: Option<String>,
}

/// Extract the `max_updated_at` field from a serialised cursor JSON.
fn extract_watermark(cursor_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(cursor_json)
        .ok()?
        .get("max_updated_at")?
        .as_str()
        .map(String::from)
}
