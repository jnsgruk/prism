use ps_core::models::SourceConfig;
use restate_sdk::prelude::*;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::SharedState;
use crate::discourse::client::DiscourseClient;

pub struct IdentityResolutionHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::object]
pub trait IdentityResolutionHandler {
    /// Resolve pending platform identities for known directory people.
    ///
    /// Keyed by platform string (e.g. "discourse-ubuntu"). Strategies:
    /// 1. Admin API email lookup (if API key has admin scope)
    /// 2. Username probing via existing identities (public endpoint)
    async fn resolve_identities() -> Result<(), TerminalError>;
}

impl IdentityResolutionHandler for IdentityResolutionHandlerImpl {
    async fn resolve_identities(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let platform = ctx.key().to_string();
        info!(platform = %platform, "starting identity resolution");

        let run_id = self.create_run(&ctx, &platform).await?;

        // Only Discourse platforms are supported for now.
        if !platform.starts_with("discourse-") {
            info!(platform = %platform, "identity resolution not yet supported for this platform");
            self.complete_run(&ctx, run_id, &platform, 0).await;
            return Ok(());
        }

        // Ensure pending resolution rows exist for all active people.
        let ensured = self.ensure_pending_rows(&ctx, &platform).await?;
        if ensured > 0 {
            info!(platform = %platform, ensured, "created pending resolution rows");
        }

        // Load source config to get base_url and decrypt API key.
        let config = self.load_discourse_config(&ctx, &platform).await?;

        // Decrypt API key outside ctx.run() to avoid journaling plaintext.
        let api_key = self
            .decrypt_source_secret_optional(config.id, "api_key")
            .await?;
        let api_username = self
            .decrypt_source_secret_optional(config.id, "api_username")
            .await?;

        let base_url = config
            .settings
            .get("base_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim_end_matches('/')
            .to_string();

        if base_url.is_empty() {
            self.fail_run(&ctx, run_id, &platform, "no base_url configured")
                .await;
            return Err(TerminalError::new("no base_url configured for source"));
        }

        let client = DiscourseClient::new(
            self.state.http_client.clone(),
            &base_url,
            api_key.as_deref().unwrap_or(""),
            api_username.as_deref().unwrap_or("system"),
        );

        // Fetch pending resolutions.
        let pending = self.load_pending(&ctx, &platform).await?;

        if pending.is_empty() {
            info!(platform = %platform, "no pending resolutions");
            self.complete_run(&ctx, run_id, &platform, 0).await;
            return Ok(());
        }

        info!(platform = %platform, count = pending.len(), "resolving pending identities");

        let mut resolved_count = 0i32;

        for person in &pending {
            let result = self.resolve_person(&ctx, &client, &platform, person).await;

            match result {
                Ok(true) => {
                    resolved_count += 1;
                    debug!(
                        platform = %platform,
                        person = %person.name,
                        "resolved identity"
                    );
                }
                Ok(false) => {
                    debug!(
                        platform = %platform,
                        person = %person.name,
                        "could not resolve identity"
                    );
                }
                Err(e) => {
                    // Rate limit — sleep and retry will happen on next run.
                    if let Some(retry_after) = extract_rate_limit_secs(&e) {
                        warn!(
                            platform = %platform,
                            retry_after,
                            resolved_count,
                            "rate limited during resolution, stopping"
                        );
                        // Use durable sleep for rate limit backoff.
                        ctx.sleep(std::time::Duration::from_secs(retry_after))
                            .await?;
                        // Continue with remaining people after sleep.
                        continue;
                    }
                    warn!(
                        platform = %platform,
                        person = %person.name,
                        error = %e,
                        "error resolving identity, skipping"
                    );
                }
            }
        }

        self.complete_run(&ctx, run_id, &platform, resolved_count)
            .await;

        // Backfill contributions now that new identities may exist.
        if resolved_count > 0 {
            self.backfill_contributions(&ctx, &platform).await;
        }

        info!(
            platform = %platform,
            resolved_count,
            total = pending.len(),
            "identity resolution complete"
        );
        Ok(())
    }
}

/// Serialisable pending person info for Restate journaling.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PendingPerson {
    person_id: String,
    name: String,
    email: Option<String>,
}

impl IdentityResolutionHandlerImpl {
    async fn create_run(
        &self,
        ctx: &ObjectContext<'_>,
        platform: &str,
    ) -> Result<Uuid, TerminalError> {
        let repos = self.state.repos.clone();
        let p = platform.to_string();
        ctx.run(|| {
            let repos = repos.clone();
            let p = p.clone();
            async move {
                let id = Uuid::now_v7();
                repos
                    .activity
                    .create_run(id, &p, "IdentityResolutionHandler", "resolve_identities")
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

    async fn complete_run(
        &self,
        ctx: &ObjectContext<'_>,
        run_id: Uuid,
        platform: &str,
        items: i32,
    ) {
        let repos = self.state.repos.clone();
        let result = ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    repos
                        .activity
                        .complete_run(run_id, items)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("complete_run")
            .await;

        if let Err(e) = result {
            error!(platform, "failed to update run status: {e}");
        }
    }

    async fn fail_run(
        &self,
        ctx: &ObjectContext<'_>,
        run_id: Uuid,
        platform: &str,
        error_msg: &str,
    ) {
        let repos = self.state.repos.clone();
        let err = error_msg.to_string();
        let result = ctx
            .run(|| {
                let repos = repos.clone();
                let err = err.clone();
                async move {
                    repos
                        .activity
                        .fail_run(run_id, &err)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(()))
                }
            })
            .name("fail_run")
            .await;

        if let Err(e) = result {
            error!(platform, "failed to update run status: {e}");
        }
    }

    async fn ensure_pending_rows(
        &self,
        ctx: &ObjectContext<'_>,
        platform: &str,
    ) -> Result<u64, TerminalError> {
        let repos = self.state.repos.clone();
        let p = platform.to_string();
        Ok(ctx
            .run(|| {
                let repos = repos.clone();
                let p = p.clone();
                async move {
                    let count = repos
                        .org
                        .ensure_resolution_rows(&p)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(count))
                }
            })
            .name("ensure_pending_rows")
            .await?
            .into_inner())
    }

    /// Load the source config for a Discourse platform.
    ///
    /// The source name matches the platform string (e.g. "discourse-ubuntu").
    async fn load_discourse_config(
        &self,
        ctx: &ObjectContext<'_>,
        platform: &str,
    ) -> Result<SourceConfig, TerminalError> {
        let repos = self.state.repos.clone();
        let p = platform.to_string();
        Ok(ctx
            .run(|| {
                let repos = repos.clone();
                let p = p.clone();
                async move {
                    let config = super::load_source_config(&repos, &p)
                        .await
                        .map_err(TerminalError::new)?;
                    Ok(Json::from(config))
                }
            })
            .name("load_config")
            .await?
            .into_inner())
    }

    async fn decrypt_source_secret_optional(
        &self,
        source_id: Uuid,
        key: &str,
    ) -> Result<Option<String>, TerminalError> {
        let encrypted = self
            .state
            .repos
            .config
            .get_encrypted_secret(source_id, key)
            .await
            .map_err(|e| TerminalError::new(format!("db error: {e}")))?;

        match encrypted {
            Some(enc) => {
                let decrypted = ps_core::crypto::decrypt(&self.state.secret_key, &enc)
                    .map_err(|e| TerminalError::new(format!("decrypt error: {e}")))?;
                let s = String::from_utf8(decrypted)
                    .map_err(|e| TerminalError::new(format!("invalid encoding: {e}")))?;
                Ok(Some(s))
            }
            None => Ok(None),
        }
    }

    async fn load_pending(
        &self,
        ctx: &ObjectContext<'_>,
        platform: &str,
    ) -> Result<Vec<PendingPerson>, TerminalError> {
        let repos = self.state.repos.clone();
        let p = platform.to_string();
        Ok(ctx
            .run(|| {
                let repos = repos.clone();
                let p = p.clone();
                async move {
                    let rows = repos
                        .org
                        .get_pending_resolutions(&p)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;

                    let people: Vec<PendingPerson> = rows
                        .into_iter()
                        .map(|r| PendingPerson {
                            person_id: r.person_id.to_string(),
                            name: r.person_name,
                            email: r.email,
                        })
                        .collect();

                    Ok(Json::from(people))
                }
            })
            .name("load_pending")
            .await?
            .into_inner())
    }

    /// Try to resolve a single person's identity on a Discourse platform.
    ///
    /// Returns `Ok(true)` if resolved, `Ok(false)` if unresolved.
    async fn resolve_person(
        &self,
        ctx: &ObjectContext<'_>,
        client: &DiscourseClient,
        platform: &str,
        person: &PendingPerson,
    ) -> Result<bool, TerminalError> {
        let person_id: Uuid = person
            .person_id
            .parse()
            .map_err(|e| TerminalError::new(format!("invalid person_id: {e}")))?;

        // Strategy 1: Admin API email lookup (preferred).
        if let Some(email) = &person.email
            && !email.is_empty()
        {
            // API call outside ctx.run() — idempotent, safe to replay.
            let result = client
                .admin_user_search(email)
                .await
                .map_err(|e| TerminalError::new(format!("discourse admin search failed: {e}")))?;

            if let Some(username) = result {
                self.store_resolution(ctx, person_id, platform, &username)
                    .await?;
                return Ok(true);
            }
        }

        // Strategy 2: Username probing via existing identities.
        let candidates = self.load_candidate_usernames(ctx, person_id).await?;

        for candidate in &candidates {
            let exists = client
                .user_exists(candidate)
                .await
                .map_err(|e| TerminalError::new(format!("discourse user probe failed: {e}")))?;

            if exists {
                self.store_resolution(ctx, person_id, platform, candidate)
                    .await?;
                return Ok(true);
            }
        }

        // No match found.
        self.store_unresolved(ctx, person_id, platform).await?;
        Ok(false)
    }

    async fn load_candidate_usernames(
        &self,
        ctx: &ObjectContext<'_>,
        person_id: Uuid,
    ) -> Result<Vec<String>, TerminalError> {
        let repos = self.state.repos.clone();
        Ok(ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    let names = repos
                        .org
                        .get_candidate_usernames(person_id)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(names))
                }
            })
            .name("load_candidates")
            .await?
            .into_inner())
    }

    async fn store_resolution(
        &self,
        ctx: &ObjectContext<'_>,
        person_id: Uuid,
        platform: &str,
        username: &str,
    ) -> Result<(), TerminalError> {
        let repos = self.state.repos.clone();
        let p = platform.to_string();
        let u = username.to_string();

        ctx.run(|| {
            let repos = repos.clone();
            let p = p.clone();
            let u = u.clone();
            async move {
                repos
                    .org
                    .resolve_identity(person_id, &p, &u)
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                Ok(Json::from(()))
            }
        })
        .name("store_resolution")
        .await?;

        Ok(())
    }

    async fn store_unresolved(
        &self,
        ctx: &ObjectContext<'_>,
        person_id: Uuid,
        platform: &str,
    ) -> Result<(), TerminalError> {
        let repos = self.state.repos.clone();
        let p = platform.to_string();

        ctx.run(|| {
            let repos = repos.clone();
            let p = p.clone();
            async move {
                repos
                    .org
                    .mark_unresolved(person_id, &p)
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                Ok(Json::from(()))
            }
        })
        .name("store_unresolved")
        .await?;

        Ok(())
    }

    /// Backfill `person_id` on Discourse contributions after new identities
    /// have been created by resolution.
    async fn backfill_contributions(&self, ctx: &ObjectContext<'_>, platform: &str) {
        let repos = self.state.repos.clone();
        let p = platform.to_string();

        let result = ctx
            .run(|| {
                let repos = repos.clone();
                let p = p.clone();
                async move {
                    let count = repos
                        .activity
                        .backfill_discourse_person_ids(&p)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(count))
                }
            })
            .name("backfill_contributions")
            .await;

        match result {
            Ok(count) => {
                let c = count.into_inner();
                if c > 0 {
                    info!(
                        platform,
                        backfilled = c,
                        "backfilled contribution person_ids"
                    );
                }
            }
            Err(e) => {
                warn!(platform, "backfill failed: {e}");
            }
        }
    }
}

/// Extract rate-limit retry-after seconds from a `TerminalError` message.
fn extract_rate_limit_secs(err: &TerminalError) -> Option<u64> {
    let msg = err.to_string();
    if msg.contains("rate limit") || msg.contains("429") {
        // Try to extract retry_after from the error message.
        // The ps_core::Error::RateLimit format includes retry_after_secs.
        Some(60) // Default to 60 seconds
    } else {
        None
    }
}
