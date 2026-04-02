use ps_core::models::SourceConfig;
use restate_sdk::prelude::*;
use tracing::{debug, info};
use uuid::Uuid;

use super::client::GitHubClient;
use super::types::GitHubTeam;
use crate::infra::SharedState;
use crate::infra::run_lifecycle::{
    complete_run, create_run, fail_run, journaled, journaled_value, terminal_err,
};
use crate::infra::secrets::decrypt_required_secret;

pub struct GithubTeamSyncHandlerImpl {
    pub state: SharedState,
}

#[restate_sdk::object]
pub trait GithubTeamSyncHandler {
    /// Discover and sync GitHub teams, members, and repos for this source.
    async fn sync_teams() -> Result<(), TerminalError>;
}

impl GithubTeamSyncHandler for GithubTeamSyncHandlerImpl {
    async fn sync_teams(&self, ctx: ObjectContext<'_>) -> Result<(), TerminalError> {
        let start = std::time::Instant::now();
        let source_type_key = ctx.key().to_string();
        let config = self.load_config(&ctx, &source_type_key).await?;
        let source_name = config.name.clone();

        let run_id = create_run!(
            ctx,
            self.state.repos,
            &source_name,
            "GithubTeamSyncHandler",
            "sync_teams"
        )?;

        let span = tracing::info_span!("handler", handler = "GithubTeamSyncHandler", source = %source_name, run_id = %run_id);
        let _guard = span.enter();

        info!("starting team sync");

        let token =
            decrypt_required_secret(&self.state, config.id, ps_core::models::SecretKey::ApiToken)
                .await?;

        let orgs = parse_orgs(&config);
        if orgs.is_empty() {
            fail_run!(
                ctx,
                self.state.repos,
                run_id,
                &source_name,
                "no orgs configured for this source"
            );
            return Err(TerminalError::new("no orgs configured for this source"));
        }

        let base_url = config
            .settings
            .get("base_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("https://api.github.com");

        let client = GitHubClient::new(self.state.http_client.clone(), base_url, &token);

        let mut total_teams = 0i32;
        for org in &orgs {
            match self.sync_org(&ctx, &client, config.id, org).await {
                Ok(count) => total_teams += count,
                Err(e) => {
                    fail_run!(ctx, self.state.repos, run_id, &source_name, &e.to_string());
                    return Err(e);
                }
            }
        }

        complete_run!(ctx, self.state.repos, run_id, &source_name, total_teams);

        info!(
            total_teams,
            duration_secs = start.elapsed().as_secs(),
            "complete"
        );
        Ok(())
    }
}

impl GithubTeamSyncHandlerImpl {
    async fn load_config(
        &self,
        ctx: &ObjectContext<'_>,
        source_name: &str,
    ) -> Result<SourceConfig, TerminalError> {
        let repos = &self.state.repos;
        let name = source_name.to_string();
        Ok(journaled_value!(ctx, "load_config", [repos, name], {
            crate::infra::load_source_config(&repos, &name)
                .await
                .map_err(TerminalError::new)?
        }))
    }

    /// Sync all teams for a single GitHub org. Returns the number of teams synced.
    ///
    /// API reads happen outside `ctx.run()` (they are idempotent and safe to
    /// replay on retry). Only DB writes are wrapped in `ctx.run()` for
    /// Restate durability.
    async fn sync_org(
        &self,
        ctx: &ObjectContext<'_>,
        client: &GitHubClient,
        source_id: Uuid,
        org: &str,
    ) -> Result<i32, TerminalError> {
        let all_teams = self.discover_teams(client, org).await?;

        debug!(org, team_count = all_teams.len(), "discovered GitHub teams");

        let mut synced_slugs = Vec::with_capacity(all_teams.len());

        // Sequential store_team is required by Restate's journaling model —
        // each side effect needs a unique name.
        for team in &all_teams {
            synced_slugs.push(team.slug.clone());

            let (members, team_repos) = tokio::try_join!(
                fetch_all_members(client, org, &team.slug),
                fetch_all_repos(client, org, &team.slug),
            )?;

            debug!(
                org,
                team = %team.slug,
                members = members.len(),
                repos = team_repos.len(),
                "fetched team details"
            );

            self.store_team(ctx, source_id, org, team, &members, &team_repos)
                .await?;
        }

        let removed = self
            .remove_stale_teams(ctx, source_id, org, &synced_slugs)
            .await?;

        if removed > 0 {
            debug!(org, removed, "removed stale GitHub teams");
        }

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        Ok(all_teams.len() as i32)
    }

    /// Discover all teams in a GitHub org (paginated).
    async fn discover_teams(
        &self,
        client: &GitHubClient,
        org: &str,
    ) -> Result<Vec<GitHubTeam>, TerminalError> {
        let mut all_teams = Vec::new();
        let mut page = 1u32;

        loop {
            let result = client
                .list_org_teams(org, page)
                .await
                .map_err(terminal_err("GitHub API error"))?;

            all_teams.extend(result.items);

            match result.next_page {
                Some(next) => page = next,
                None => break,
            }
        }

        Ok(all_teams)
    }

    /// Store a team and its members/repos as a durable side effect.
    async fn store_team(
        &self,
        ctx: &ObjectContext<'_>,
        source_id: Uuid,
        org: &str,
        team: &GitHubTeam,
        members: &[String],
        team_repos: &[(String, String)],
    ) -> Result<(), TerminalError> {
        let repos = &self.state.repos;
        let slug = team.slug.clone();
        let name = team.name.clone();
        let description = team.description.clone();
        let team_id = team.id;
        let org_owned = org.to_string();
        let members = members.to_vec();
        let team_repos = team_repos.to_vec();

        let step_name = format!("store_team_{slug}");
        journaled!(
            ctx,
            step_name,
            [
                repos,
                org_owned,
                slug,
                name,
                description,
                members,
                team_repos
            ],
            {
                let db_id = repos
                    .org
                    .upsert_github_team(
                        source_id,
                        &org_owned,
                        team_id,
                        &slug,
                        &name,
                        description.as_deref(),
                    )
                    .await
                    .map_err(terminal_err("db error"))?;

                repos
                    .org
                    .replace_github_team_members(db_id, &members)
                    .await
                    .map_err(terminal_err("db error"))?;

                repos
                    .org
                    .replace_github_team_repos(db_id, &team_repos)
                    .await
                    .map_err(terminal_err("db error"))?;
            }
        );

        Ok(())
    }

    /// Remove teams that no longer exist in GitHub.
    async fn remove_stale_teams(
        &self,
        ctx: &ObjectContext<'_>,
        source_id: Uuid,
        org: &str,
        synced_slugs: &[String],
    ) -> Result<u64, TerminalError> {
        let repos = &self.state.repos;
        let org_owned = org.to_string();
        let slugs = synced_slugs.to_vec();

        Ok(journaled_value!(
            ctx,
            "remove_stale_teams",
            [repos, org_owned, slugs],
            {
                repos
                    .org
                    .remove_stale_github_teams(source_id, &org_owned, &slugs)
                    .await
                    .map_err(terminal_err("db error"))?
            }
        ))
    }
}

/// Parse the `orgs` array from source settings.
fn parse_orgs(config: &SourceConfig) -> Vec<String> {
    config
        .settings
        .get("orgs")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Fetch all members of a team across all pages.
async fn fetch_all_members(
    client: &GitHubClient,
    org: &str,
    team_slug: &str,
) -> Result<Vec<String>, TerminalError> {
    let mut members = Vec::new();
    let mut page = 1u32;

    loop {
        let result = client
            .list_team_members(org, team_slug, page)
            .await
            .map_err(terminal_err("GitHub API error"))?;

        members.extend(result.items.into_iter().map(|u| u.login.to_lowercase()));

        match result.next_page {
            Some(next) => page = next,
            None => break,
        }
    }

    Ok(members)
}

/// Fetch all repos of a team across all pages, excluding archived repos.
async fn fetch_all_repos(
    client: &GitHubClient,
    org: &str,
    team_slug: &str,
) -> Result<Vec<(String, String)>, TerminalError> {
    let mut repos = Vec::new();
    let mut page = 1u32;

    loop {
        let result = client
            .list_team_repos(org, team_slug, page)
            .await
            .map_err(terminal_err("GitHub API error"))?;

        repos.extend(
            result
                .items
                .into_iter()
                .filter(|r| !r.archived.unwrap_or(false))
                .map(|r| (r.owner.login.clone(), r.name)),
        );

        match result.next_page {
            Some(next) => page = next,
            None => break,
        }
    }

    Ok(repos)
}
