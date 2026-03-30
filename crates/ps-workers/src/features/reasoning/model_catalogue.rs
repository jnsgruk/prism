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
                debug!(provider = %provider_str, "skipping — no API key configured");
                continue;
            };

            match self.refresh_provider(*provider, &api_key).await {
                Ok(count) => {
                    #[allow(clippy::cast_possible_wrap)]
                    let count_i32 = count as i32;
                    total_models += count_i32;
                    set_provider_progress(&mut progress, *provider, count, None);
                    self.update_progress(run_id, total_models, &progress).await;
                    debug!(provider = %provider_str, count, "model catalogue refreshed");
                }
                Err(err_msg) => {
                    warn!(provider = %provider_str, "{err_msg}");
                    set_provider_progress(&mut progress, *provider, 0, Some(&err_msg));
                    had_error = true;
                    if first_error.is_none() {
                        first_error = Some(err_msg);
                    }
                }
            }
        }

        // Complete or fail the run
        if total_models == 0 && had_error {
            let err_msg = first_error.unwrap_or_else(|| "all providers failed".into());
            progress.phase = "failed".into();
            progress.status_message = err_msg.clone();
            self.update_progress(run_id, 0, &progress).await;

            fail_run!(ctx, self.state.repos, run_id, "_model_catalogue", &err_msg);
            return Err(TerminalError::new(err_msg));
        }

        progress.phase = "completed".into();
        progress.status_message = format!("{total_models} models cached");
        self.update_progress(run_id, total_models, &progress).await;

        complete_run!(
            ctx,
            self.state.repos,
            run_id,
            "_model_catalogue",
            total_models
        );

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

fn set_provider_progress(
    progress: &mut CatalogueProgress,
    provider: AiProvider,
    models_fetched: usize,
    error: Option<&str>,
) {
    let prov = ProviderProgress {
        models_fetched,
        error: error.map(String::from),
    };
    match provider {
        AiProvider::Google => progress.google = Some(prov),
        AiProvider::OpenRouter => progress.openrouter = Some(prov),
    }
}
