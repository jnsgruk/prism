use std::collections::HashSet;

use ps_core::models::TeamType;
use serde::Deserialize;
use tonic::Status;

use crate::directory::parse_directory_html;

/// Detect file format and parse into `DirectoryRecord` entries.
///
/// For HTML files, this also computes the team hierarchy from the directory
/// nesting structure: depth-1 people are group leaders, depth-2 people with
/// reports are team leaders, depth-3+ people with reports are squad leaders.
#[allow(clippy::result_large_err)]
pub(crate) fn parse_file_content(content: &str) -> Result<Vec<DirectoryRecord>, Status> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('<') || trimmed.starts_with("<!") {
        parse_html_to_records(content)
    } else {
        serde_json::from_str(content)
            .map_err(|e| Status::invalid_argument(format!("invalid JSON: {e}")))
    }
}

/// Parse HTML directory into `DirectoryRecord` entries with hierarchy information.
///
/// Determines which people are team/squad leaders by checking whether they have
/// reports (i.e. someone else lists them as their manager).
#[allow(clippy::result_large_err)]
fn parse_html_to_records(content: &str) -> Result<Vec<DirectoryRecord>, Status> {
    let people = parse_directory_html(content);
    if people.is_empty() {
        return Err(Status::invalid_argument(
            "no valid entries found in HTML directory file",
        ));
    }

    // Build set of people who have reports (someone names them as manager).
    let managers: HashSet<String> = people
        .iter()
        .filter_map(|p| p.manager_name.clone())
        .collect();

    Ok(people
        .into_iter()
        .map(|p| {
            let has_reports = managers.contains(&p.display_name);

            // Determine the team name this person belongs to or leads.
            // - depth 1 + has_reports → group leader, team = group name
            // - depth 2 + has_reports → team leader, team = "<name>'s Team"
            // - depth 3+ + has_reports → squad leader, team = "<name>'s Squad"
            // - depth 2+ without reports → IC, team = "<manager>'s Team" or Squad
            let (team, team_type) = derive_team_assignment(
                &p.display_name,
                p.depth,
                has_reports,
                p.group.as_ref(),
                p.manager_name.as_ref(),
            );

            let mut identities = vec![
                DirectoryIdentity {
                    platform: "github".to_owned(),
                    username: p.github_username,
                },
                DirectoryIdentity {
                    platform: "launchpad".to_owned(),
                    username: p.launchpad_username,
                },
            ];
            if let Some(mm) = p.mattermost_username {
                identities.push(DirectoryIdentity {
                    platform: "mattermost".to_owned(),
                    username: mm,
                });
            }
            DirectoryRecord {
                name: p.display_name,
                email: Some(p.email),
                level: p.title,
                directory_id: None,
                team,
                team_type,
                org: Some("Canonical".to_owned()),
                identities,
                manager_name: p.manager_name,
                depth: Some(p.depth),
                has_reports,
                group: p.group,
            }
        })
        .collect())
}

/// Derive the team name and type for a person based on directory nesting.
fn derive_team_assignment(
    name: &str,
    depth: u32,
    has_reports: bool,
    group: Option<&String>,
    manager_name: Option<&String>,
) -> (Option<String>, Option<TeamType>) {
    match (depth, has_reports) {
        // VP / group leader or depth-2 IC — assign to group
        (1, _) | (2, false) => (group.cloned(), Some(TeamType::Group)),
        // Depth-2 with reports — team leader, auto-name from their name
        (2, true) => (Some(format!("{name}'s Team")), Some(TeamType::Team)),
        // Depth 3+ with reports — squad leader
        (_, true) => (Some(format!("{name}'s Squad")), Some(TeamType::Squad)),
        // Depth 3+ IC — assign to their manager's team/squad
        (d, false) if d >= 3 => manager_name.map_or_else(
            || (group.cloned(), Some(TeamType::Group)),
            |mgr| {
                if d == 3 {
                    // Manager is depth 2 → team
                    (Some(format!("{mgr}'s Team")), Some(TeamType::Team))
                } else {
                    // Manager is depth 3+ → squad
                    (Some(format!("{mgr}'s Squad")), Some(TeamType::Squad))
                }
            },
        ),
        _ => (None, None),
    }
}

/// A single record in a directory import file.
#[derive(Deserialize)]
pub(crate) struct DirectoryRecord {
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub directory_id: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub team_type: Option<TeamType>,
    #[serde(default)]
    pub org: Option<String>,
    #[serde(default)]
    pub identities: Vec<DirectoryIdentity>,
    #[serde(default)]
    pub manager_name: Option<String>,
    #[serde(default)]
    pub depth: Option<u32>,
    #[serde(default)]
    pub has_reports: bool,
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct DirectoryIdentity {
    pub platform: String,
    pub username: String,
}
