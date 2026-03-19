use ps_core::models::AiProvider;
use ps_reasoning::catalogue;
use restate_sdk::prelude::*;
use serde::Serialize;
use tracing::{info, warn};
use uuid::Uuid;

use super::SharedState;
use super::run_lifecycle::create_run;

pub struct ModelCatalogueHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::service]
pub trait ModelCatalogueHandler {
    /// Refresh the model catalogue for all configured providers.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    openrouter: Option<ProviderProgress>,
    status_message: String,
}

#[derive(Serialize, Clone)]
struct ProviderProgress {
    models_fetched: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl ModelCatalogueHandlerImpl {
    /// Fetch models from each configured provider and store them.
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
            openrouter: None,
            status_message: "Starting model catalogue refresh".into(),
        };
        self.update_progress(run_id, 0, &progress).await;

        let providers = [
            (AiProvider::Google, "google_api_key"),
            (AiProvider::OpenRouter, "openrouter_api_key"),
        ];

        let mut total_models: i32 = 0;
        let mut had_error = false;
        let mut first_error: Option<String> = None;

        for (provider, secret_key_name) in &providers {
            let provider_str = provider.to_string();
            progress.phase = format!("fetching {provider_str}");
            self.update_progress(run_id, total_models, &progress).await;

            let Some(api_key) = self.decrypt_provider_key(secret_key_name).await else {
                info!(provider = %provider_str, "skipping catalogue refresh — no API key configured");
                continue;
            };

            let models = match catalogue::fetch_models(&self.state.http_client, *provider, &api_key)
                .await
            {
                Ok(m) => m,
                Err(e) => {
                    let err_msg = format!("{provider_str}: {e}");
                    warn!(provider = %provider_str, error = %e, "failed to fetch model catalogue");
                    let prov_progress = ProviderProgress {
                        models_fetched: 0,
                        error: Some(e.to_string()),
                    };
                    match *provider {
                        AiProvider::Google => progress.google = Some(prov_progress),
                        AiProvider::OpenRouter => progress.openrouter = Some(prov_progress),
                    }
                    had_error = true;
                    if first_error.is_none() {
                        first_error = Some(err_msg);
                    }
                    continue;
                }
            };

            let count = models.len();

            if let Err(e) = self
                .state
                .repos
                .config
                .replace_ai_models(&provider_str, &models)
                .await
            {
                let err_msg = format!("{provider_str}: failed to store models: {e}");
                warn!(provider = %provider_str, error = %e, "failed to store model catalogue");
                had_error = true;
                if first_error.is_none() {
                    first_error = Some(err_msg);
                }
                continue;
            }

            // Record refresh timestamp
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

            #[allow(clippy::cast_possible_wrap)]
            let count_i32 = count as i32;
            total_models += count_i32;

            let prov_progress = ProviderProgress {
                models_fetched: count,
                error: None,
            };
            match *provider {
                AiProvider::Google => progress.google = Some(prov_progress),
                AiProvider::OpenRouter => progress.openrouter = Some(prov_progress),
            }
            self.update_progress(run_id, total_models, &progress).await;

            info!(provider = %provider_str, count, "model catalogue refreshed");
        }

        // Complete or fail the run
        if total_models == 0 && had_error {
            let err_msg = first_error.unwrap_or_else(|| "all providers failed".into());
            progress.phase = "failed".into();
            progress.status_message = err_msg.clone();
            self.update_progress(run_id, 0, &progress).await;

            if let Err(e) = self.state.repos.activity.fail_run(run_id, &err_msg).await {
                warn!(error = %e, "failed to mark catalogue run as failed");
            }
            return Err(TerminalError::new(err_msg));
        }

        progress.phase = "completed".into();
        progress.status_message = format!("{total_models} models cached");
        self.update_progress(run_id, total_models, &progress).await;

        if let Err(e) = self
            .state
            .repos
            .activity
            .complete_run(run_id, total_models)
            .await
        {
            warn!(error = %e, "failed to complete catalogue run");
        }

        Ok(())
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
            warn!(error = %e, "failed to update catalogue progress");
        }
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
