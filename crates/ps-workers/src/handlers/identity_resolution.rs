use restate_sdk::prelude::*;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::SharedState;
use crate::discourse::client::DiscourseClient;

pub struct IdentityResolutionHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::service]
pub trait IdentityResolutionHandler {
    /// Resolve pending platform identities for known directory people
    /// across all configured Discourse sources.
    async fn resolve_identities() -> Result<(), TerminalError>;
}

impl IdentityResolutionHandler for IdentityResolutionHandlerImpl {
    async fn resolve_identities(&self, ctx: Context<'_>) -> Result<(), TerminalError> {
        info!("starting identity resolution across all Discourse sources");

        let run_id = self.create_run(&ctx).await?;

        // List all enabled Discourse sources.
        let sources = self.list_discourse_sources(&ctx).await?;

        if sources.is_empty() {
            info!("no enabled Discourse sources configured");
            self.complete_run(&ctx, run_id, 0).await;
            return Ok(());
        }

        info!(count = sources.len(), "found Discourse sources to resolve");

        let mut total_resolved = 0i32;

        for source in &sources {
            match self.resolve_source(&ctx, source).await {
                Ok(count) => {
                    total_resolved += count;
                    if count > 0 {
                        info!(
                            source = %source.name,
                            resolved = count,
                            "resolved identities for source"
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        source = %source.name,
                        error = %e,
                        "failed to resolve identities for source, continuing"
                    );
                }
            }
        }

        self.complete_run(&ctx, run_id, total_resolved).await;

        info!(total_resolved, "identity resolution complete");
        Ok(())
    }
}

/// Serialisable source info for Restate journaling.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SourceInfo {
    id: String,
    name: String,
    platform: String,
    base_url: String,
}

/// Serialisable pending person info for Restate journaling.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PendingPerson {
    person_id: String,
    name: String,
    email: Option<String>,
}

impl IdentityResolutionHandlerImpl {
    async fn create_run(&self, ctx: &Context<'_>) -> Result<Uuid, TerminalError> {
        let repos = self.state.repos.clone();
        ctx.run(|| {
            let repos = repos.clone();
            async move {
                let id = Uuid::now_v7();
                repos
                    .activity
                    .create_run(
                        id,
                        "_system",
                        "IdentityResolutionHandler",
                        "resolve_identities",
                    )
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

    async fn complete_run(&self, ctx: &Context<'_>, run_id: Uuid, items: i32) {
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
            error!("failed to update run status: {e}");
        }
    }

    /// List all enabled Discourse sources from the config table.
    async fn list_discourse_sources(
        &self,
        ctx: &Context<'_>,
    ) -> Result<Vec<SourceInfo>, TerminalError> {
        let repos = self.state.repos.clone();
        Ok(ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    let sources = repos
                        .config
                        .list_sources()
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;

                    let discourse_sources: Vec<SourceInfo> = sources
                        .into_iter()
                        .filter(|s| s.enabled && s.source_type.is_discourse())
                        .map(|s| {
                            let base_url = s
                                .settings
                                .get("base_url")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("")
                                .trim_end_matches('/')
                                .to_string();
                            SourceInfo {
                                id: s.id.to_string(),
                                name: s.name,
                                platform: s.source_type.to_string(),
                                base_url,
                            }
                        })
                        .collect();

                    Ok(Json::from(discourse_sources))
                }
            })
            .name("list_discourse_sources")
            .await?
            .into_inner())
    }

    /// Resolve identities for a single Discourse source.
    /// Returns the number of people resolved.
    async fn resolve_source(
        &self,
        ctx: &Context<'_>,
        source: &SourceInfo,
    ) -> Result<i32, TerminalError> {
        let platform = &source.platform;

        // Ensure pending resolution rows exist for all active people.
        let ensured = self.ensure_pending_rows(ctx, platform).await?;
        if ensured > 0 {
            info!(source = %source.name, ensured, "created pending resolution rows");
        }

        if source.base_url.is_empty() {
            return Err(TerminalError::new(format!(
                "no base_url configured for source '{}'",
                source.name
            )));
        }

        let source_id: Uuid = source
            .id
            .parse()
            .map_err(|e| TerminalError::new(format!("invalid source_id: {e}")))?;

        // Decrypt API key outside ctx.run() to avoid journaling plaintext.
        let api_key = self
            .decrypt_source_secret_optional(source_id, "api_key")
            .await?;
        let api_username = self
            .decrypt_source_secret_optional(source_id, "api_username")
            .await?;

        let client = DiscourseClient::new(
            self.state.http_client.clone(),
            &source.base_url,
            api_key.as_deref().unwrap_or(""),
            api_username.as_deref().unwrap_or("system"),
        );

        // Fetch pending resolutions.
        let pending = self.load_pending(ctx, platform).await?;

        if pending.is_empty() {
            debug!(source = %source.name, "no pending resolutions");
            return Ok(0);
        }

        info!(source = %source.name, count = pending.len(), "resolving pending identities");

        let mut resolved_count = 0i32;

        for person in &pending {
            let result = self.resolve_person(ctx, &client, platform, person).await;

            match result {
                Ok(true) => {
                    resolved_count += 1;
                    debug!(
                        source = %source.name,
                        person = %person.name,
                        "resolved identity"
                    );
                }
                Ok(false) => {
                    debug!(
                        source = %source.name,
                        person = %person.name,
                        "could not resolve identity"
                    );
                }
                Err(e) => {
                    if is_rate_limited(&e) {
                        warn!(
                            source = %source.name,
                            resolved_count,
                            "rate limited during resolution, sleeping 60s"
                        );
                        ctx.sleep(std::time::Duration::from_secs(60)).await?;
                        continue;
                    }
                    warn!(
                        source = %source.name,
                        person = %person.name,
                        error = %e,
                        "error resolving identity, skipping"
                    );
                }
            }
        }

        // Backfill contributions now that new identities may exist.
        if resolved_count > 0 {
            self.backfill_contributions(ctx, platform).await;
        }

        Ok(resolved_count)
    }

    async fn ensure_pending_rows(
        &self,
        ctx: &Context<'_>,
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
        ctx: &Context<'_>,
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
        ctx: &Context<'_>,
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
        ctx: &Context<'_>,
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
        ctx: &Context<'_>,
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
        ctx: &Context<'_>,
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
    async fn backfill_contributions(&self, ctx: &Context<'_>, platform: &str) {
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

/// Check if a `TerminalError` is caused by rate limiting.
fn is_rate_limited(err: &TerminalError) -> bool {
    let msg = err.to_string();
    msg.contains("rate limit") || msg.contains("429")
}
