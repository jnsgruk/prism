use ps_core::ingestion::{ContributionInput, IngestionContext, IngestionPlan};
use ps_core::models::{ContributionType, RateLimitInfo, SourceConfig};
use restate_sdk::prelude::*;
use tracing::{error, info, warn};
use uuid::Uuid;

use super::SharedState;
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
        let source_name = ctx.key().to_string();
        info!(source = %source_name, "starting Jira ingestion run");

        self.execute_ingestion(&ctx, &source_name, None).await
    }

    async fn backfill(
        &self,
        ctx: ObjectContext<'_>,
        since_date: String,
    ) -> Result<(), TerminalError> {
        let source_name = ctx.key().to_string();
        info!(source = %source_name, since = %since_date, "starting Jira backfill");

        self.execute_ingestion(&ctx, &source_name, Some(since_date))
            .await
    }
}

impl JiraIngestionHandlerImpl {
    async fn execute_ingestion(
        &self,
        ctx: &ObjectContext<'_>,
        source_name: &str,
        override_watermark: Option<String>,
    ) -> Result<(), TerminalError> {
        let config = self.load_config(ctx, source_name).await?;

        let source = registry::create_source(&config.source_type).ok_or_else(|| {
            TerminalError::new(format!("unsupported source type: {}", config.source_type))
        })?;

        let method = if override_watermark.is_some() {
            "backfill"
        } else {
            "run_ingestion"
        };
        let run_id = self.create_run(ctx, source_name, method).await?;

        // Decrypt token and email once per run, outside ctx.run() to avoid journaling
        let token = self.decrypt_source_token(config.id, "api_token").await?;
        let email = self.decrypt_source_secret(config.id, "email").await.ok();

        let ing_ctx = self.ingestion_context(&config, Some(token), email);

        let mut plan: IngestionPlan = match source.plan(&ing_ctx).await {
            Ok(p) => p,
            Err(e) => {
                let msg = e.to_string();
                self.fail_run(ctx, run_id, source_name, &msg).await;
                return Err(TerminalError::new(format!("plan failed: {msg}")));
            }
        };

        if let Some(ref wm) = override_watermark {
            plan.watermark = Some(wm.clone());
        }

        info!(
            source = source_name,
            watermark = ?plan.watermark,
            "Jira ingestion plan ready"
        );

        // Build the initial cursor with full Jira config
        let initial_cursor = build_jira_cursor(&config, &plan);

        let (total_items, final_cursor) = self
            .fetch_store_loop(
                ctx,
                run_id,
                source_name,
                &config,
                &initial_cursor,
                ing_ctx.token.as_deref(),
            )
            .await?;

        if total_items > 0 {
            self.advance_watermark(
                ctx,
                &config,
                &final_cursor,
                total_items,
                ing_ctx.token.as_deref(),
            )
            .await?;
        }

        self.complete_run(ctx, run_id, source_name, total_items)
            .await;

        if total_items > 0 {
            info!(source = source_name, "triggering metrics recomputation");
            ctx.service_client::<MetricsComputeHandlerClient>()
                .compute_current_periods()
                .send();
        }

        info!(source = source_name, total_items, "Jira ingestion complete");
        Ok(())
    }

    fn ingestion_context(
        &self,
        config: &SourceConfig,
        token: Option<String>,
        email: Option<String>,
    ) -> IngestionContext {
        IngestionContext {
            repos: self.state.repos.clone(),
            source_config: config.clone(),
            http_client: self.state.http_client.clone(),
            token,
            email,
            api_username: None,
        }
    }

    async fn decrypt_source_token(
        &self,
        source_id: uuid::Uuid,
        key: &str,
    ) -> Result<String, TerminalError> {
        let encrypted = self
            .state
            .repos
            .config
            .get_encrypted_secret(source_id, key)
            .await
            .map_err(|e| TerminalError::new(format!("db error: {e}")))?
            .ok_or_else(|| TerminalError::new(format!("source has no {key} configured")))?;

        let decrypted = ps_core::crypto::decrypt(&self.state.secret_key, &encrypted)
            .map_err(|e| TerminalError::new(format!("decrypt error: {e}")))?;

        String::from_utf8(decrypted)
            .map_err(|e| TerminalError::new(format!("invalid encoding: {e}")))
    }

    async fn decrypt_source_secret(
        &self,
        source_id: uuid::Uuid,
        key: &str,
    ) -> Result<String, TerminalError> {
        self.decrypt_source_token(source_id, key).await
    }

    async fn load_config(
        &self,
        ctx: &ObjectContext<'_>,
        source_name: &str,
    ) -> Result<SourceConfig, TerminalError> {
        let repos = self.state.repos.clone();
        let name = source_name.to_string();
        Ok(ctx
            .run(|| {
                let repos = repos.clone();
                let name = name.clone();
                async move {
                    let config = super::load_source_config(&repos, &name)
                        .await
                        .map_err(TerminalError::new)?;
                    Ok(Json::from(config))
                }
            })
            .name("load_config")
            .await?
            .into_inner())
    }

    async fn create_run(
        &self,
        ctx: &ObjectContext<'_>,
        source_name: &str,
        method: &str,
    ) -> Result<Uuid, TerminalError> {
        let repos = self.state.repos.clone();
        let sn = source_name.to_string();
        let method_owned = method.to_string();
        ctx.run(|| {
            let repos = repos.clone();
            let sn = sn.clone();
            let method_owned = method_owned.clone();
            async move {
                let id = Uuid::now_v7();
                repos
                    .activity
                    .create_run(id, &sn, "JiraIngestionHandler", &method_owned)
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                Ok(Json::from(id.to_string()))
            }
        })
        .name("create_run")
        .await?
        .into_inner()
        .parse()
        .map_err(|e| TerminalError::new(format!("invalid run_id: {e}")))
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
            let batch = self.fetch_batch(ctx, config, &cursor, token).await?;

            for item in &batch.items {
                if item.contribution_type == ContributionType::JiraTicket {
                    tickets_fetched += 1;
                }
            }

            if !batch.items.is_empty() {
                let stored = self.store_batch(ctx, config, &batch.items, token).await?;
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

            let Some(next_cursor) = batch.next_cursor else {
                break;
            };
            cursor = next_cursor;
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

    async fn fetch_batch(
        &self,
        ctx: &ObjectContext<'_>,
        config: &SourceConfig,
        cursor: &str,
        token: Option<&str>,
    ) -> Result<SerFetchResult, TerminalError> {
        let repos = self.state.repos.clone();
        let http = self.state.http_client.clone();
        let cfg = config.clone();
        let tok = token.map(String::from);
        let cur = cursor.to_string();
        let source_type = config.source_type.clone();

        Ok(ctx
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
                        http_client: http,
                        token: tok,
                        email: None,
                        api_username: None,
                    };
                    let result = src
                        .fetch_batch(&ic, &cur)
                        .await
                        .map_err(|e| TerminalError::new(format!("fetch failed: {e}")))?;

                    Ok(Json::from(SerFetchResult {
                        items: result.items,
                        next_cursor: result.next_cursor,
                        rate_limit: result.rate_limit,
                    }))
                }
            })
            .name("fetch_batch")
            .await?
            .into_inner())
    }

    async fn store_batch(
        &self,
        ctx: &ObjectContext<'_>,
        config: &SourceConfig,
        items: &[ContributionInput],
        token: Option<&str>,
    ) -> Result<i32, TerminalError> {
        let repos = self.state.repos.clone();
        let http = self.state.http_client.clone();
        let cfg = config.clone();
        let tok = token.map(String::from);
        let items = items.to_vec();
        let source_type = config.source_type.clone();

        Ok(ctx
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
                        http_client: http,
                        token: tok,
                        email: None,
                        api_username: None,
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
            .into_inner())
    }

    async fn advance_watermark(
        &self,
        ctx: &ObjectContext<'_>,
        config: &SourceConfig,
        cursor: &str,
        total_items: i32,
        token: Option<&str>,
    ) -> Result<(), TerminalError> {
        let repos = self.state.repos.clone();
        let http = self.state.http_client.clone();
        let cfg = config.clone();
        let tok = token.map(String::from);
        let wm = cursor.to_string();
        let source_type = config.source_type.clone();

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
                    http_client: http,
                    token: tok,
                    email: None,
                    api_username: None,
                };
                let watermark = extract_watermark(&wm).unwrap_or_default();
                src.advance_watermark(&ic, &watermark, total_items)
                    .await
                    .map_err(|e| TerminalError::new(format!("advance failed: {e}")))?;
                Ok(Json::from(()))
            }
        })
        .name("advance_watermark")
        .await?;

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
        next_page_token: None,
        max_updated_at: plan.watermark.clone(),
        base_url,
        story_points_field,
        api_mode,
    };

    serde_json::to_string(&cursor).unwrap_or_default()
}

/// Serialisable fetch result for Restate journaling.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SerFetchResult {
    items: Vec<ContributionInput>,
    next_cursor: Option<String>,
    #[serde(default)]
    rate_limit: Option<RateLimitInfo>,
}

/// Extract the `max_updated_at` field from a serialised cursor JSON.
fn extract_watermark(cursor_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(cursor_json)
        .ok()?
        .get("max_updated_at")?
        .as_str()
        .map(String::from)
}

/// Build a structured progress JSON for the Jira ingestion run.
fn build_progress_json(
    cursor_json: &str,
    tickets_fetched: u32,
    rate_limit: Option<&RateLimitInfo>,
) -> serde_json::Value {
    let cursor: serde_json::Value =
        serde_json::from_str(cursor_json).unwrap_or(serde_json::Value::Null);

    let projects = cursor
        .get("projects")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let scope = if projects.is_empty() {
        "all projects".to_string()
    } else {
        projects
    };

    let status_message = format!("Fetching Jira issues from {scope} ({tickets_fetched} so far)");

    let mut progress = serde_json::json!({
        "phase": "jql_search",
        "tickets_fetched": tickets_fetched,
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
