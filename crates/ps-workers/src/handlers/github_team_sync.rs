use ps_core::models::SourceConfig;
use restate_sdk::prelude::*;
use tracing::{error, info};
use uuid::Uuid;

use super::SharedState;
use crate::github::client::GitHubClient;

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
        let source_name = ctx.key().to_string();
        info!(source = %source_name, "starting GitHub team sync");

        // Create run record (generate ID inside side effect to survive replays)
        let repos = self.state.repos.clone();
        let sn = source_name.clone();
        let run_id: Uuid = ctx
            .run(|| {
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
            .map_err(|e| TerminalError::new(format!("invalid run_id: {e}")))?;

        // Step 1: Load source config
        let repos = self.state.repos.clone();
        let name = source_name.clone();
        let config: SourceConfig = ctx
            .run(|| {
                let repos = repos.clone();
                let name = name.clone();
                async move {
                    let row = repos
                        .config
                        .get_enabled_source_by_name(&name)
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?
                        .ok_or_else(|| {
                            TerminalError::new(format!("source '{name}' not found or disabled"))
                        })?;
                    Ok(Json::from(row))
                }
            })
            .name("load_config")
            .await?
            .into_inner();

        // Step 2: Decrypt token
        let repos = self.state.repos.clone();
        let source_id = config.id;
        let sk = self.state.secret_key;
        let token: String = ctx
            .run(|| {
                let repos = repos.clone();
                async move {
                    let encrypted = repos
                        .config
                        .get_encrypted_secret(source_id, "api_token")
                        .await
                        .map_err(|e| TerminalError::new(format!("db error: {e}")))?
                        .ok_or_else(|| TerminalError::new("no api_token configured"))?;

                    let decrypted = ps_core::crypto::decrypt(&sk, &encrypted)
                        .map_err(|e| TerminalError::new(format!("decrypt error: {e}")))?;

                    let token = String::from_utf8(decrypted)
                        .map_err(|e| TerminalError::new(format!("invalid token: {e}")))?;

                    Ok(Json::from(token))
                }
            })
            .name("decrypt_token")
            .await?
            .into_inner();

        // Step 3: Parse orgs from settings
        let orgs: Vec<String> = config
            .settings
            .get("orgs")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

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

        // Step 4: For each org, discover teams → members → repos
        let mut total_teams = 0i32;
        for org in &orgs {
            match self.sync_org(&ctx, &client, source_id, org).await {
                Ok(count) => total_teams += count,
                Err(e) => {
                    self.fail_run(&ctx, run_id, &source_name, &e.to_string())
                        .await;
                    return Err(e);
                }
            }
        }

        // Complete run
        self.complete_run(&ctx, run_id, &source_name, total_teams)
            .await;

        info!(source = %source_name, total_teams, "GitHub team sync complete");
        Ok(())
    }
}

impl GithubTeamSyncHandlerImpl {
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
        source_id: uuid::Uuid,
        org: &str,
    ) -> Result<i32, TerminalError> {
        // Discover all teams in the org (paginated)
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

        info!(org, team_count = all_teams.len(), "discovered GitHub teams");

        let mut synced_slugs = Vec::with_capacity(all_teams.len());

        for team in &all_teams {
            synced_slugs.push(team.slug.clone());

            // Fetch members (all pages)
            let members = fetch_all_members(client, org, &team.slug).await?;

            // Fetch repos (all pages, skip archived)
            let team_repos = fetch_all_repos(client, org, &team.slug).await?;

            info!(
                org,
                team = %team.slug,
                members = members.len(),
                repos = team_repos.len(),
                "fetched team details"
            );

            // Store team + members + repos as a durable side effect
            let repos = self.state.repos.clone();
            let team_id = team.id;
            let slug = team.slug.clone();
            let name = team.name.clone();
            let description = team.description.clone();
            let org_owned = org.to_string();

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
        }

        // Remove teams that no longer exist in GitHub
        let repos = self.state.repos.clone();
        let org_owned = org.to_string();
        let removed: u64 = ctx
            .run(|| {
                let repos = repos.clone();
                let org = org_owned.clone();
                let slugs = synced_slugs.clone();
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
            .into_inner();

        if removed > 0 {
            info!(org, removed, "removed stale GitHub teams");
        }

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        Ok(all_teams.len() as i32)
    }
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
