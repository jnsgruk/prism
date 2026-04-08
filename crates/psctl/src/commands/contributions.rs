use anyhow::Result;
use ps_proto::canonical::prism::v1::{
    ContributionState, ContributionType, ListPersonContributionsRequest, Platform,
};

use crate::client::Clients;
use crate::format;

pub fn platform_str_to_proto(s: &str) -> i32 {
    Platform::from_user_str(s).into()
}

pub fn proto_platform_display(v: i32) -> &'static str {
    Platform::try_from(v).map_or("unknown", Platform::display_str)
}

pub fn proto_contribution_type_display(v: i32) -> &'static str {
    ContributionType::try_from(v).map_or("unknown", ContributionType::display_str)
}

fn proto_contribution_state_display(v: i32) -> &'static str {
    ContributionState::try_from(v).map_or("\u{2014}", ContributionState::display_str)
}

pub async fn contributions(
    clients: &mut Clients,
    person: &str,
    platform: Option<&str>,
    since: Option<&str>,
) -> Result<()> {
    let response = clients
        .metrics
        .list_person_contributions(ListPersonContributionsRequest {
            person_id: person.to_string(),
            platform: platform.map_or(0, platform_str_to_proto),
            contribution_type: 0,
            since: since.map(ToString::to_string),
            page_size: 50,
            page_index: 0,
            sort_field: None,
            sort_desc: Some(true),
            state: 0,
            search: None,
            platform_instance: None,
            until: None,
        })
        .await?
        .into_inner();

    if response.contributions.is_empty() {
        println!("No contributions found.");
        return Ok(());
    }

    println!(
        "{:<12}  {:<15}  {:<40}  {:<10}  {:<20}",
        "PLATFORM", "TYPE", "TITLE", "STATE", "CREATED"
    );
    println!("{}", "\u{2500}".repeat(100));

    for c in &response.contributions {
        println!(
            "{:<12}  {:<15}  {:<40}  {:<10}  {:<20}",
            proto_platform_display(c.platform),
            proto_contribution_type_display(c.contribution_type),
            truncate(&c.title, 40),
            proto_contribution_state_display(c.state),
            format::timestamp(c.created_at.as_ref()),
        );
    }

    println!(
        "\nShowing {} of {} contributions.",
        response.contributions.len(),
        response.total_count
    );

    Ok(())
}

use crate::format::truncate;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_str_to_proto_all_platforms() {
        // Verify delegation to ps_proto::convert covers all platforms.
        assert_eq!(platform_str_to_proto("github"), i32::from(Platform::Github));
        assert_eq!(platform_str_to_proto("jira"), i32::from(Platform::Jira));
        assert_eq!(
            platform_str_to_proto("discourse"),
            i32::from(Platform::Discourse)
        );
        assert_eq!(
            platform_str_to_proto("launchpad"),
            i32::from(Platform::Launchpad)
        );
        assert_eq!(
            platform_str_to_proto("mattermost"),
            i32::from(Platform::Mattermost)
        );
        assert_eq!(
            platform_str_to_proto("google_drive"),
            i32::from(Platform::GoogleDrive)
        );
        assert_eq!(
            platform_str_to_proto("unknown"),
            i32::from(Platform::Unspecified)
        );
    }

    #[test]
    fn platform_str_to_proto_case_insensitive() {
        assert_eq!(platform_str_to_proto("GitHub"), i32::from(Platform::Github));
        assert_eq!(platform_str_to_proto("JIRA"), i32::from(Platform::Jira));
    }

    #[test]
    fn proto_platform_display_all() {
        assert_eq!(
            proto_platform_display(i32::from(Platform::Github)),
            "github"
        );
        assert_eq!(
            proto_platform_display(i32::from(Platform::Discourse)),
            "discourse"
        );
        assert_eq!(
            proto_platform_display(i32::from(Platform::GoogleDrive)),
            "google_drive"
        );
        assert_eq!(proto_platform_display(999), "unknown");
    }

    #[test]
    fn proto_contribution_type_display_all() {
        assert_eq!(
            proto_contribution_type_display(i32::from(ContributionType::PullRequest)),
            "pull_request"
        );
        assert_eq!(
            proto_contribution_type_display(i32::from(ContributionType::DiscourseLike)),
            "discourse_like"
        );
        assert_eq!(proto_contribution_type_display(999), "unknown");
    }

    #[test]
    fn proto_contribution_state_display_all() {
        assert_eq!(
            proto_contribution_state_display(i32::from(ContributionState::Open)),
            "open"
        );
        assert_eq!(
            proto_contribution_state_display(i32::from(ContributionState::Merged)),
            "merged"
        );
        assert_eq!(
            proto_contribution_state_display(i32::from(ContributionState::Done)),
            "done"
        );
        assert_eq!(
            proto_contribution_state_display(i32::from(ContributionState::Approved)),
            "APPROVED"
        );
        assert_eq!(proto_contribution_state_display(999), "\u{2014}");
    }
}
