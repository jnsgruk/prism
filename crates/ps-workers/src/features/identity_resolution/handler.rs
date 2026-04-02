use restate_sdk::prelude::*;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::features::ingestion::discourse::client::DiscourseClient;
use crate::infra::SharedState;
use crate::infra::run_lifecycle::{
    complete_run, create_run, journaled, journaled_value, terminal_err,
};
use crate::infra::secrets::decrypt_optional_secret;

/// Result of a Discourse API lookup attempt.
enum LookupOutcome {
    /// Found a matching username.
    Found(String),
    /// No match — try next strategy.
    NotFound,
    /// API rate-limited — caller should sleep and retry.
    RateLimited,
}

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
        let start = std::time::Instant::now();

        let run_id = create_run!(
            ctx,
            self.state.repos,
            "_system",
            "IdentityResolutionHandler",
            "resolve_identities"
        )?;

        let span =
            tracing::info_span!("handler", handler = "IdentityResolutionHandler", run_id = %run_id);
        let _guard = span.enter();

        info!("starting identity resolution");

        // List all enabled Discourse sources.
        let sources = self.list_discourse_sources(&ctx).await?;

        if sources.is_empty() {
            debug!("no enabled Discourse sources configured");
            complete_run!(ctx, self.state.repos, run_id, "_system", 0);
            return Ok(());
        }

        debug!(count = sources.len(), "found Discourse sources to resolve");

        let mut total_resolved = 0i32;

        for source in &sources {
            match self.resolve_source(&ctx, source).await {
                Ok(count) => {
                    total_resolved += count;
                    if count > 0 {
                        debug!(
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

        complete_run!(ctx, self.state.repos, run_id, "_system", total_resolved);

        info!(
            total_resolved,
            duration_secs = start.elapsed().as_secs(),
            "complete"
        );
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

/// Journaled result from an HTTP lookup call, so Restate replay is
/// deterministic regardless of live API behaviour.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum LookupResult {
    Found(String),
    NotFound,
    RateLimited,
}

/// Journaled result from a username-existence probe.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum ProbeResult {
    Exists,
    NotFound,
    RateLimited,
}

impl IdentityResolutionHandlerImpl {
    /// List all enabled Discourse sources from the config table.
    async fn list_discourse_sources(
        &self,
        ctx: &Context<'_>,
    ) -> Result<Vec<SourceInfo>, TerminalError> {
        let repos = &self.state.repos;
        Ok(journaled_value!(ctx, "list_discourse_sources", [repos], {
            let sources = repos
                .config
                .list_sources()
                .await
                .map_err(terminal_err("db error"))?;

            sources
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
                .collect::<Vec<SourceInfo>>()
        }))
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
            debug!(source = %source.name, ensured, "created pending resolution rows");
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
            .map_err(terminal_err("invalid source_id"))?;

        // Decrypt API key outside ctx.run() to avoid journaling plaintext.
        let api_key =
            decrypt_optional_secret(&self.state, source_id, ps_core::models::SecretKey::ApiKey)
                .await?;
        let api_username = decrypt_optional_secret(
            &self.state,
            source_id,
            ps_core::models::SecretKey::ApiUsername,
        )
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

        debug!(source = %source.name, count = pending.len(), "resolving pending identities");

        let mut resolved_count = 0i32;

        for (person_index, person) in pending.iter().enumerate() {
            match self
                .resolve_person(ctx, &client, platform, person, person_index)
                .await
            {
                Ok(Some(true)) => {
                    resolved_count += 1;
                    debug!(
                        source = %source.name,
                        person = %person.name,
                        "resolved identity"
                    );
                }
                Ok(Some(false)) => {
                    debug!(
                        source = %source.name,
                        person = %person.name,
                        "could not resolve identity"
                    );
                }
                Ok(None) => {
                    // Rate limited — sleep and continue to next person.
                    warn!(
                        source = %source.name,
                        resolved_count,
                        "rate limited during resolution, sleeping 60s"
                    );
                    ctx.sleep(std::time::Duration::from_secs(60)).await?;
                }
                Err(e) => {
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
        let repos = &self.state.repos;
        let p = platform.to_string();
        Ok(journaled_value!(ctx, "ensure_pending_rows", [repos, p], {
            repos
                .org
                .ensure_resolution_rows(&p)
                .await
                .map_err(terminal_err("db error"))?
        }))
    }

    async fn load_pending(
        &self,
        ctx: &Context<'_>,
        platform: &str,
    ) -> Result<Vec<PendingPerson>, TerminalError> {
        let repos = &self.state.repos;
        let p = platform.to_string();
        Ok(journaled_value!(ctx, "load_pending", [repos, p], {
            let rows = repos
                .org
                .get_pending_resolutions(&p)
                .await
                .map_err(terminal_err("db error"))?;

            rows.into_iter()
                .map(|r| PendingPerson {
                    person_id: r.person_id.to_string(),
                    name: r.person_name,
                    email: r.email,
                })
                .collect::<Vec<PendingPerson>>()
        }))
    }

    /// Try to resolve a single person's identity on a Discourse platform.
    ///
    /// Returns `Ok(Some(true))` if resolved, `Ok(Some(false))` if unresolved,
    /// or `Ok(None)` if rate-limited (caller should sleep and continue).
    async fn resolve_person(
        &self,
        ctx: &Context<'_>,
        client: &DiscourseClient,
        platform: &str,
        person: &PendingPerson,
        person_index: usize,
    ) -> Result<Option<bool>, TerminalError> {
        let person_id: Uuid = person
            .person_id
            .parse()
            .map_err(terminal_err("invalid person_id"))?;

        // Strategy 1: Admin API email lookup (preferred).
        match self
            .try_email_lookup(ctx, client, person, person_index)
            .await?
        {
            LookupOutcome::Found(username) => {
                self.store_resolution(ctx, person_id, platform, &username, person_index)
                    .await?;
                return Ok(Some(true));
            }
            LookupOutcome::RateLimited => return Ok(None),
            LookupOutcome::NotFound => {}
        }

        // Strategy 2: Username probing via existing identities.
        match self
            .try_username_probe(ctx, client, person_id, person_index)
            .await?
        {
            LookupOutcome::Found(username) => {
                self.store_resolution(ctx, person_id, platform, &username, person_index)
                    .await?;
                return Ok(Some(true));
            }
            LookupOutcome::RateLimited => return Ok(None),
            LookupOutcome::NotFound => {}
        }

        // No match found.
        self.store_unresolved(ctx, person_id, platform, person_index)
            .await?;
        Ok(Some(false))
    }

    /// Try resolving via Discourse admin email search.
    /// Wrapped in `ctx.run()` so the API result is journaled.
    async fn try_email_lookup(
        &self,
        ctx: &Context<'_>,
        client: &DiscourseClient,
        person: &PendingPerson,
        person_index: usize,
    ) -> Result<LookupOutcome, TerminalError> {
        let email = match &person.email {
            Some(e) if !e.is_empty() => e.clone(),
            _ => return Ok(LookupOutcome::NotFound),
        };

        let c = client.clone();
        let result = ctx
            .run(|| async move {
                match c.admin_user_search(&email).await {
                    Ok(Some(username)) => Ok(Json::from(LookupResult::Found(username))),
                    Ok(None) => Ok(Json::from(LookupResult::NotFound)),
                    Err(err) if err.is_rate_limit() => Ok(Json::from(LookupResult::RateLimited)),
                    Err(err) => Err(TerminalError::new(format!(
                        "discourse admin search failed: {err}"
                    ))
                    .into()),
                }
            })
            .name(format!("email_lookup_{person_index}"))
            .await?
            .into_inner();

        Ok(match result {
            LookupResult::Found(username) => LookupOutcome::Found(username),
            LookupResult::NotFound => LookupOutcome::NotFound,
            LookupResult::RateLimited => LookupOutcome::RateLimited,
        })
    }

    /// Try resolving via username probing against existing identities.
    async fn try_username_probe(
        &self,
        ctx: &Context<'_>,
        client: &DiscourseClient,
        person_id: Uuid,
        person_index: usize,
    ) -> Result<LookupOutcome, TerminalError> {
        let candidates = self
            .load_candidate_usernames(ctx, person_id, person_index)
            .await?;

        for (candidate_index, candidate) in candidates.iter().enumerate() {
            let c = client.clone();
            let cand = candidate.clone();
            let result = ctx
                .run(|| async move {
                    match c.user_exists(&cand).await {
                        Ok(true) => Ok(Json::from(ProbeResult::Exists)),
                        Ok(false) => Ok(Json::from(ProbeResult::NotFound)),
                        Err(err) if err.is_rate_limit() => Ok(Json::from(ProbeResult::RateLimited)),
                        Err(err) => Err(TerminalError::new(format!(
                            "discourse user probe failed: {err}"
                        ))
                        .into()),
                    }
                })
                .name(format!("probe_{person_index}_{candidate_index}"))
                .await?
                .into_inner();

            match result {
                ProbeResult::Exists => return Ok(LookupOutcome::Found(candidate.clone())),
                ProbeResult::RateLimited => return Ok(LookupOutcome::RateLimited),
                ProbeResult::NotFound => {}
            }
        }

        Ok(LookupOutcome::NotFound)
    }

    async fn load_candidate_usernames(
        &self,
        ctx: &Context<'_>,
        person_id: Uuid,
        person_index: usize,
    ) -> Result<Vec<String>, TerminalError> {
        let repos = &self.state.repos;
        Ok(journaled_value!(
            ctx,
            format!("load_candidates_{person_index}"),
            [repos],
            {
                repos
                    .org
                    .get_candidate_usernames(person_id)
                    .await
                    .map_err(terminal_err("db error"))?
            }
        ))
    }

    async fn store_resolution(
        &self,
        ctx: &Context<'_>,
        person_id: Uuid,
        platform: &str,
        username: &str,
        person_index: usize,
    ) -> Result<(), TerminalError> {
        let repos = &self.state.repos;
        let p = platform.to_string();
        let u = username.to_string();

        journaled!(
            ctx,
            format!("store_resolution_{person_index}"),
            [repos, p, u],
            {
                repos
                    .org
                    .resolve_identity(person_id, &p, &u)
                    .await
                    .map_err(terminal_err("db error"))?;
            }
        );

        Ok(())
    }

    async fn store_unresolved(
        &self,
        ctx: &Context<'_>,
        person_id: Uuid,
        platform: &str,
        person_index: usize,
    ) -> Result<(), TerminalError> {
        let repos = &self.state.repos;
        let p = platform.to_string();

        journaled!(
            ctx,
            format!("store_unresolved_{person_index}"),
            [repos, p],
            {
                repos
                    .org
                    .mark_unresolved(person_id, &p)
                    .await
                    .map_err(terminal_err("db error"))?;
            }
        );

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
                        .map_err(terminal_err("db error"))?;
                    Ok(Json::from(count))
                }
            })
            .name("backfill_contributions")
            .await;

        match result {
            Ok(count) => {
                let c = count.into_inner();
                if c > 0 {
                    debug!(
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
