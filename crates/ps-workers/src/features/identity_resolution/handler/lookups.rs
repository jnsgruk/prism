use restate_sdk::prelude::*;
use tracing::{debug, warn};
use uuid::Uuid;

use super::{
    IdentityResolutionHandlerImpl, LookupOutcome, LookupResult, PendingPerson, ProbeResult,
};
use crate::features::ingestion::discourse::client::DiscourseClient;
use crate::infra::run_lifecycle::{journaled, journaled_value, terminal_err};

impl IdentityResolutionHandlerImpl {
    pub(super) async fn ensure_pending_rows(
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

    pub(super) async fn load_pending(
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
    pub(super) async fn resolve_person(
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
    pub(super) async fn try_email_lookup(
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
    pub(super) async fn try_username_probe(
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

    pub(super) async fn load_candidate_usernames(
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

    pub(super) async fn store_resolution(
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

    pub(super) async fn store_unresolved(
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
    pub(super) async fn backfill_contributions(&self, ctx: &Context<'_>, platform: &str) {
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
