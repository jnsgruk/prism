use ps_core::ingestion::RepoTarget;
use sqlx::PgPool;
use tracing::{debug, info};
use uuid::Uuid;

use super::client::GitHubClient;

/// Discover repositories for the given GitHub orgs and sync them to `org.repositories`.
///
/// Applies exclude filters from the source config settings:
/// - `exclude_archived`: skip archived repos (default: true)
/// - `exclude_repos`: list of repo names to skip
///
/// Returns the list of `RepoTarget` values to ingest.
pub async fn discover_repos(
    client: &GitHubClient,
    orgs: &[String],
    pool: &PgPool,
    exclude_archived: bool,
    exclude_repos: &[String],
) -> Result<Vec<RepoTarget>, ps_core::Error> {
    let mut targets = Vec::new();

    for org in orgs {
        let mut page = 1u32;
        loop {
            let result = client
                .list_org_repos(org, page, 100)
                .await
                .map_err(|e| ps_core::Error::Internal(format!("GitHub API error: {e}")))?;

            for repo in &result.items {
                // Skip archived repos if configured
                if exclude_archived && repo.archived.unwrap_or(false) {
                    continue;
                }

                // Skip explicitly excluded repos
                if exclude_repos.contains(&repo.name) {
                    continue;
                }

                let owner = &repo.owner.login;
                let repo_name = &repo.name;
                let default_branch = repo.default_branch.as_deref();
                let language = repo.language.as_deref();

                // Upsert into org.repositories
                sqlx::query!(
                    r#"
                    INSERT INTO org.repositories (id, github_org, github_repo, default_branch, primary_language)
                    VALUES ($1, $2, $3, $4, $5)
                    ON CONFLICT (github_org, github_repo)
                    DO UPDATE SET
                        default_branch = COALESCE(EXCLUDED.default_branch, org.repositories.default_branch),
                        primary_language = COALESCE(EXCLUDED.primary_language, org.repositories.primary_language)
                    "#,
                    Uuid::now_v7(),
                    owner,
                    repo_name,
                    default_branch,
                    language,
                )
                .execute(pool)
                .await
                .map_err(|e| ps_core::Error::Database(e.to_string()))?;

                targets.push(RepoTarget {
                    owner: owner.clone(),
                    repo: repo_name.clone(),
                });
            }

            debug!(
                org,
                page,
                repos_on_page = result.items.len(),
                "discovered repos"
            );

            match result.next_page {
                Some(next) => page = next,
                None => break,
            }
        }
    }

    info!(total_repos = targets.len(), "repo discovery complete");
    Ok(targets)
}
