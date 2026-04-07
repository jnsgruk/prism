use ps_core::models::AiProvider;
use ps_reasoning::catalogue;
use restate_sdk::prelude::*;
use serde::Serialize;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::infra::SharedState;
use crate::infra::run_lifecycle::{complete_run, create_run, fail_run};

pub struct ModelCatalogueHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::service]
pub trait ModelCatalogueHandler {
    /// Refresh the model catalogue for the configured Google provider.
    async fn refresh_catalogue() -> Result<(), TerminalError>;
}

impl ModelCatalogueHandler for ModelCatalogueHandlerImpl {
    async fn refresh_catalogue(&self, ctx: Context<'_>) -> Result<(), TerminalError> {
        self.do_refresh(&ctx).await
    }
}

/// Progress report stored in the run's `progress` JSONB column.
#[derive(Serialize)]
struct CatalogueProgress {
    phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    google: Option<ProviderProgress>,
    status_message: String,
}

#[derive(Serialize, Clone)]
struct ProviderProgress {
    models_fetched: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl ModelCatalogueHandlerImpl {
    /// Fetch models from the Google provider and store them.
    ///
    /// Run creation is journaled via `ctx.run()` so retries reuse the same
    /// run ID. All other operations are outside `ctx.run()`:
    /// - API calls are read-only and safe to retry
    /// - DB writes use delete+insert in a transaction (idempotent)
    /// - No secrets should be journaled
    async fn do_refresh(&self, ctx: &Context<'_>) -> Result<(), TerminalError> {
        // Create run record (journaled — retries reuse the same UUID)
        let run_id = create_run!(
            ctx,
            self.state.repos,
            "_model_catalogue",
            "ModelCatalogueHandler",
            "refresh_catalogue"
        )?;

        let mut progress = CatalogueProgress {
            phase: "starting".into(),
            google: None,
            status_message: "Starting model catalogue refresh".into(),
        };
        self.update_progress(run_id, 0, &progress).await;

        progress.phase = "fetching google".into();
        self.update_progress(run_id, 0, &progress).await;

        let Some(api_key) = self.decrypt_provider_key("google_api_key").await else {
            debug!("skipping — no Google API key configured");
            progress.phase = "completed".into();
            progress.status_message = "No Google API key configured".into();
            self.update_progress(run_id, 0, &progress).await;

            complete_run!(ctx, self.state.repos, run_id, "_model_catalogue", 0);
            return Ok(());
        };

        match self.refresh_provider(AiProvider::Google, &api_key).await {
            Ok(count) => {
                #[allow(clippy::cast_possible_wrap)]
                let count_i32 = count as i32;
                progress.google = Some(ProviderProgress {
                    models_fetched: count,
                    error: None,
                });
                progress.phase = "completed".into();
                progress.status_message = format!("{count_i32} models cached");
                self.update_progress(run_id, count_i32, &progress).await;

                complete_run!(ctx, self.state.repos, run_id, "_model_catalogue", count_i32);
                debug!(count, "model catalogue refreshed");
                Ok(())
            }
            Err(err_msg) => {
                warn!("{err_msg}");
                progress.google = Some(ProviderProgress {
                    models_fetched: 0,
                    error: Some(err_msg.clone()),
                });
                progress.phase = "failed".into();
                progress.status_message = err_msg.clone();
                self.update_progress(run_id, 0, &progress).await;

                fail_run!(ctx, self.state.repos, run_id, "_model_catalogue", &err_msg);
                Err(TerminalError::new(err_msg))
            }
        }
    }

    /// Best-effort progress update (not journaled).
    async fn update_progress(&self, run_id: Uuid, items: i32, progress: &CatalogueProgress) {
        let json = serde_json::to_value(progress).unwrap_or_default();
        if let Err(e) = self
            .state
            .repos
            .activity
            .update_run_progress_detail(run_id, items, &json)
            .await
        {
            debug!(error = %e, "failed to update catalogue progress");
        }
    }

    /// Fetch models from one provider and store them. Returns the count on
    /// success, or an error message string on failure.
    async fn refresh_provider(&self, provider: AiProvider, api_key: &str) -> Result<usize, String> {
        let provider_str = provider.to_string();
        let models = catalogue::fetch_models(&self.state.http_client, provider, api_key)
            .await
            .map_err(|e| format!("{provider_str}: {e}"))?;

        let count = models.len();
        self.state
            .repos
            .config
            .replace_ai_models(&provider_str, &models)
            .await
            .map_err(|e| format!("{provider_str}: failed to store models: {e}"))?;

        // Record refresh timestamp.
        let ts_key = format!("ai.models_refreshed.{provider_str}");
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let _ = self
            .state
            .repos
            .config
            .set_global_setting(&ts_key, &serde_json::json!(now))
            .await;

        Ok(count)
    }

    /// Decrypt a provider API key from the global secrets store.
    async fn decrypt_provider_key(&self, secret_key_name: &str) -> Option<String> {
        let encrypted = match self
            .state
            .repos
            .config
            .get_global_secret(secret_key_name)
            .await
        {
            Ok(Some(enc)) => enc,
            Ok(None) => return None,
            Err(e) => {
                warn!(key = secret_key_name, error = %e, "failed to read provider secret");
                return None;
            }
        };

        match ps_core::crypto::decrypt(&self.state.secret_key, &encrypted) {
            Ok(decrypted) => String::from_utf8(decrypted).ok(),
            Err(e) => {
                warn!(key = secret_key_name, error = %e, "failed to decrypt provider secret");
                None
            }
        }
    }
}
