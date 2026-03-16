use ps_core::ingestion::RepoTarget;
use ps_core::repo::org::OrgRepo;
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
    org_repo: &OrgRepo,
    exclude_archived: bool,
    exclude_repos: &[String],
) -> Result<Vec<RepoTarget>, ps_core::Error> {
    let mut targets = Vec::new();
    // Collect per-page batches for bulk upsert
    let mut ids = Vec::new();
    let mut gh_orgs = Vec::new();
    let mut gh_repos = Vec::new();
    let mut branches = Vec::new();
    let mut languages = Vec::new();

    for org in orgs {
        let mut page = 1u32;
        loop {
            let result = client
                .list_org_repos(org, page, 100)
                .await
                .map_err(|e| ps_core::Error::Internal(format!("GitHub API error: {e}")))?;

            for repo in &result.items {
                if exclude_archived && repo.archived.unwrap_or(false) {
                    continue;
                }
                if exclude_repos.contains(&repo.name) {
                    continue;
                }

                let owner = &repo.owner.login;
                let repo_name = &repo.name;

                ids.push(Uuid::now_v7());
                gh_orgs.push(owner.clone());
                gh_repos.push(repo_name.clone());
                branches.push(repo.default_branch.clone());
                languages.push(repo.language.clone());

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

    // Batch upsert all discovered repos in a single query
    if !targets.is_empty() {
        let org_refs: Vec<&str> = gh_orgs.iter().map(String::as_str).collect();
        let repo_refs: Vec<&str> = gh_repos.iter().map(String::as_str).collect();
        let branch_refs: Vec<Option<&str>> = branches.iter().map(|b| b.as_deref()).collect();
        let lang_refs: Vec<Option<&str>> = languages.iter().map(|l| l.as_deref()).collect();
        org_repo
            .bulk_upsert_repositories(&ids, &org_refs, &repo_refs, &branch_refs, &lang_refs)
            .await?;
    }

    info!(total_repos = targets.len(), "repo discovery complete");
    Ok(targets)
}
