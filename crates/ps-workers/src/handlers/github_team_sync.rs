use ps_core::models::SourceConfig;
use restate_sdk::prelude::*;
use tracing::{error, info};
use uuid::Uuid;

use super::SharedState;
use crate::github::client::GitHubClient;
use crate::github::types::GitHubTeam;

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
        let source_type_key = ctx.key().to_string();
        let config = self.load_config(&ctx, &source_type_key).await?;
        let source_name = config.name.clone();
        info!(source = %source_name, "starting GitHub team sync");

        let run_id = self.create_run(&ctx, &source_name).await?;
        let token = self.decrypt_token(&ctx, config.id).await?;

        let orgs = parse_orgs(&config);
        if orgs.is_empty() {
            self.fail_run(
                &ctx,
                run_id,
                &source_name,
                "no orgs configured for this source",
            )
            .await;
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
                    self.fail_run(&ctx, run_id, &source_name, &e.to_string())
                        .await;
                    return Err(e);
                }
            }
        }

        self.complete_run(&ctx, run_id, &source_name, total_teams)
            .await;

        info!(source = %source_name, total_teams, "GitHub team sync complete");
        Ok(())
    }
}

impl GithubTeamSyncHandlerImpl {
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
    ) -> Result<Uuid, TerminalError> {
        let repos = self.state.repos.clone();
        let sn = source_name.to_string();
        ctx.run(|| {
            let repos = repos.clone();
            let sn = sn.clone();
            async move {
                let id = Uuid::now_v7();
                repos
                    .activity
                    .create_run(id, &sn, "GithubTeamSyncHandler", "sync_teams")
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

    async fn decrypt_token(
        &self,
        ctx: &ObjectContext<'_>,
        source_id: Uuid,
    ) -> Result<String, TerminalError> {
        // Load encrypted bytes inside ctx.run() for durability, but decrypt
        // OUTSIDE ctx.run() so the plaintext token is never persisted in the
        // Restate journal.
        let repos = self.state.repos.clone();
        let encrypted = ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    let encrypted = repos
                        .config
                        .get_encrypted_secret(source_id, "api_token")
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?
                        .ok_or_else(|| TerminalError::new("no api_token configured"))?;

                    Ok(Json::from(encrypted))
                }
            })
            .name("load_encrypted_token")
            .await?
            .into_inner();

        let decrypted = ps_core::crypto::decrypt(&self.state.secret_key, &encrypted)
            .map_err(|e| TerminalError::new(format!("decrypt error: {e}")))?;

        String::from_utf8(decrypted).map_err(|e| TerminalError::new(format!("invalid token: {e}")))
    }

    async fn complete_run(
        &self,
        ctx: &ObjectContext<'_>,
        run_id: Uuid,
        source_name: &str,
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
            error!(source = source_name, "failed to update run status: {e}");
        }
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

        info!(org, team_count = all_teams.len(), "discovered GitHub teams");

        let mut synced_slugs = Vec::with_capacity(all_teams.len());

        // Sequential store_team is required by Restate's journaling model —
        // each side effect needs a unique name.
        for team in &all_teams {
            synced_slugs.push(team.slug.clone());

            let (members, team_repos) = tokio::try_join!(
                fetch_all_members(client, org, &team.slug),
                fetch_all_repos(client, org, &team.slug),
            )?;

            info!(
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
            info!(org, removed, "removed stale GitHub teams");
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
                .map_err(|e| TerminalError::new(format!("GitHub API error: {e}")))?;

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
        let repos = self.state.repos.clone();
        let slug = team.slug.clone();
        let name = team.name.clone();
        let description = team.description.clone();
        let team_id = team.id;
        let org_owned = org.to_string();
        let members = members.to_vec();
        let team_repos = team_repos.to_vec();

        ctx.run(|| {
            let repos = repos.clone();
            let org = org_owned.clone();
            let slug = slug.clone();
            let name = name.clone();
            let description = description.clone();
            let members = members.clone();
            let team_repos = team_repos.clone();
            async move {
                let db_id = repos
                    .org
                    .upsert_github_team(
                        source_id,
                        &org,
                        team_id,
                        &slug,
                        &name,
                        description.as_deref(),
                    )
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;

                repos
                    .org
                    .replace_github_team_members(db_id, &members)
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;

                repos
                    .org
                    .replace_github_team_repos(db_id, &team_repos)
                    .await
                    .map_err(|e| TerminalError::new(format!("db error: {e}")))?;

                Ok(Json::from(()))
            }
        })
        .name(format!("store_team_{slug}"))
        .await?;

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
        let repos = self.state.repos.clone();
        let org_owned = org.to_string();
        let slugs = synced_slugs.to_vec();

        Ok(ctx
            .run(|| {
                let repos = repos.clone();
                let org = org_owned.clone();
                let slugs = slugs.clone();
                async move {
                    let count = repos
                        .org
                        .remove_stale_github_teams(source_id, &org, &slugs)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?;
                    Ok(Json::from(count))
                }
            })
            .name("remove_stale_teams")
            .await?
            .into_inner())
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
            .map_err(|e| TerminalError::new(format!("GitHub API error: {e}")))?;

        members.extend(result.items.into_iter().map(|u| u.login));

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
            .map_err(|e| TerminalError::new(format!("GitHub API error: {e}")))?;

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
