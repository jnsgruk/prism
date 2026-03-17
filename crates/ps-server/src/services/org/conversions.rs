use std::collections::HashMap;

use ps_core::models::TeamType;
use ps_core::repo::org::{IdentityRow, PersonRow, TeamWithCount, github_teams::GitHubTeamRow};
use ps_proto::prism::v1::{
    GitHubTeam as ProtoGitHubTeam, Person, PlatformIdentity, Team, TeamType as ProtoTeamType,
};
use tonic::Status;
use uuid::Uuid;

/// Build `Person` proto messages from person rows + their platform identities.
pub(super) fn build_people(people: Vec<PersonRow>, identities: &[IdentityRow]) -> Vec<Person> {
    // Index identities by person_id for O(N+M) instead of O(N*M) lookup.
    let mut identity_map: HashMap<Uuid, Vec<&IdentityRow>> = HashMap::new();
    for i in identities {
        identity_map.entry(i.person_id).or_default().push(i);
    }

    people
        .into_iter()
        .map(|p| {
            let person_identities: Vec<PlatformIdentity> = identity_map
                .get(&p.id)
                .map(|ids| {
                    ids.iter()
                        .map(|i| PlatformIdentity {
                            platform: i.platform.clone(),
                            username: i.platform_username.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();

            Person {
                id: p.id.to_string(),
                name: p.name,
                email: p.email,
                level: p.level,
                identities: person_identities,
                active: p.active,
                team_name: p.team_name,
                team_id: p.team_id.map(|id| id.to_string()),
            }
        })
        .collect()
}

pub(super) fn team_type_to_proto(tt: TeamType) -> i32 {
    match tt {
        TeamType::Org => ProtoTeamType::Org.into(),
        TeamType::Group => ProtoTeamType::Group.into(),
        TeamType::Team => ProtoTeamType::Team.into(),
        TeamType::Squad => ProtoTeamType::Squad.into(),
    }
}

#[allow(clippy::result_large_err)]
pub(super) fn proto_to_team_type(v: i32) -> Result<TeamType, Status> {
    match ProtoTeamType::try_from(v) {
        Ok(ProtoTeamType::Org) => Ok(TeamType::Org),
        Ok(ProtoTeamType::Group) => Ok(TeamType::Group),
        Ok(ProtoTeamType::Team) => Ok(TeamType::Team),
        Ok(ProtoTeamType::Squad) => Ok(TeamType::Squad),
        _ => Err(Status::invalid_argument("invalid team_type")),
    }
}

pub(super) fn github_team_to_proto(t: GitHubTeamRow) -> ProtoGitHubTeam {
    ProtoGitHubTeam {
        id: t.id.to_string(),
        source_id: t.source_id.to_string(),
        github_org: t.github_org,
        github_team_id: t.github_team_id,
        slug: t.slug,
        name: t.name,
        description: t.description,
        member_count: t.member_count,
        repo_count: t.repo_count,
    }
}

pub(super) fn team_to_proto(t: TeamWithCount) -> Team {
    Team {
        id: t.id.to_string(),
        name: t.name,
        org_name: t.org_name,
        parent_team_id: t.parent_team_id.map(|id| id.to_string()),
        lead_id: t.lead_id.map(|id| id.to_string()),
        member_count: t.member_count,
        team_type: team_type_to_proto(t.team_type),
        total_member_count: 0,
        children: Vec::new(),
        lead_name: t.lead_name,
    }
}

/// Recursively populate a team's children and compute total member counts.
fn populate_team_tree(
    id: &str,
    proto_teams: &mut HashMap<String, Team>,
    children_map: &HashMap<String, Vec<String>>,
) -> Team {
    let child_ids: Vec<String> = children_map.get(id).cloned().unwrap_or_default();

    let children: Vec<Team> = child_ids
        .iter()
        .map(|cid| populate_team_tree(cid, proto_teams, children_map))
        .collect();

    let total: i32 = children.iter().map(|c| c.total_member_count).sum();

    let mut team = proto_teams.remove(id).unwrap_or_default();
    team.total_member_count = team.member_count + total;
    team.children = children;
    team
}

/// Build a tree of teams from a flat list, returning only root nodes.
pub(super) fn build_team_tree(teams: Vec<TeamWithCount>) -> Vec<Team> {
    let mut proto_teams: HashMap<String, Team> = HashMap::new();
    let mut children_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut root_ids: Vec<String> = Vec::new();

    for t in teams {
        let id = t.id.to_string();
        let parent_id = t.parent_team_id.map(|p| p.to_string());
        proto_teams.insert(id.clone(), team_to_proto(t));

        if let Some(pid) = parent_id {
            children_map.entry(pid).or_default().push(id);
        } else {
            root_ids.push(id);
        }
    }

    root_ids
        .iter()
        .map(|id| populate_team_tree(id, &mut proto_teams, &children_map))
        .collect()
}
