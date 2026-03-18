use ps_core::models::AiProvider;
use ps_reasoning::catalogue;
use restate_sdk::prelude::*;
use tracing::{info, warn};

use super::SharedState;

pub struct ModelCatalogueHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::service]
pub trait ModelCatalogueHandler {
    /// Refresh the model catalogue for all configured providers.
    async fn refresh_catalogue() -> Result<(), TerminalError>;
}

impl ModelCatalogueHandler for ModelCatalogueHandlerImpl {
    async fn refresh_catalogue(&self, _ctx: Context<'_>) -> Result<(), TerminalError> {
        self.do_refresh().await
    }
}

impl ModelCatalogueHandlerImpl {
    /// Fetch models from each configured provider and store them.
    ///
    /// All operations are outside `ctx.run()`:
    /// - API calls are read-only and safe to retry
    /// - DB writes use delete+insert in a transaction (idempotent)
    /// - No secrets should be journaled
    async fn do_refresh(&self) -> Result<(), TerminalError> {
        let providers = [
            (AiProvider::Google, "google_api_key"),
            (AiProvider::OpenRouter, "openrouter_api_key"),
        ];

        for (provider, secret_key_name) in &providers {
            let provider_str = provider.to_string();

            let Some(api_key) = self.decrypt_provider_key(secret_key_name).await else {
                info!(provider = %provider_str, "skipping catalogue refresh — no API key configured");
                continue;
            };

            let models = match catalogue::fetch_models(&self.state.http_client, *provider, &api_key)
                .await
            {
                Ok(m) => m,
                Err(e) => {
                    warn!(provider = %provider_str, error = %e, "failed to fetch model catalogue");
                    continue;
                }
            };

            let count = models.len();

            self.state
                .repos
                .config
                .replace_ai_models(&provider_str, &models)
                .await
                .map_err(|e| TerminalError::new(format!("failed to store models: {e}")))?;

            // Record refresh timestamp
            let ts_key = format!("ai.models_refreshed.{provider_str}");
            let now = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            self.state
                .repos
                .config
                .set_global_setting(&ts_key, &serde_json::json!(now))
                .await
                .map_err(|e| {
                    TerminalError::new(format!("failed to update refresh timestamp: {e}"))
                })?;

            info!(provider = %provider_str, count, "model catalogue refreshed");
        }

        Ok(())
    }

    /// Decrypt a provider API key from the global secrets store.
    async fn decrypt_provider_key(&self, secret_key_name: &str) -> Option<String> {
        let encrypted = self
            .state
            .repos
            .config
            .get_global_secret(secret_key_name)
            .await
            .ok()
            .flatten()?;

        let decrypted = ps_core::crypto::decrypt(&self.state.secret_key, &encrypted).ok()?;
        String::from_utf8(decrypted).ok()
    }
}
