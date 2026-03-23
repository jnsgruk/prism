use anyhow::Result;
use ps_proto::canonical::prism::v1::{
    ContributionState, ContributionType, ListPersonContributionsRequest, Platform,
};

use crate::client::Clients;
use crate::format;

pub fn platform_str_to_proto(s: &str) -> i32 {
    match s {
        "github" => Platform::Github.into(),
        "jira" => Platform::Jira.into(),
        "launchpad" => Platform::Launchpad.into(),
        "mattermost" => Platform::Mattermost.into(),
        s if s.starts_with("discourse") => Platform::Discourse.into(),
        _ => Platform::Unspecified.into(),
    }
}

pub fn proto_platform_display(v: i32) -> &'static str {
    match Platform::try_from(v) {
        Ok(Platform::Github) => "github",
        Ok(Platform::Jira) => "jira",
        Ok(Platform::Discourse) => "discourse",
        Ok(Platform::Launchpad) => "launchpad",
        Ok(Platform::Mattermost) => "mattermost",
        _ => "unknown",
    }
}

pub fn proto_contribution_type_display(v: i32) -> &'static str {
    match ContributionType::try_from(v) {
        Ok(ContributionType::PullRequest) => "pull_request",
        Ok(ContributionType::PrReview) => "pr_review",
        Ok(ContributionType::JiraTicket) => "jira_ticket",
        Ok(ContributionType::DiscourseTopic) => "discourse_topic",
        Ok(ContributionType::DiscoursePost) => "discourse_post",
        Ok(ContributionType::DiscourseLike) => "discourse_like",
        _ => "unknown",
    }
}

fn proto_contribution_state_display(v: i32) -> &'static str {
    match ContributionState::try_from(v) {
        Ok(ContributionState::Open) => "open",
        Ok(ContributionState::Closed) => "closed",
        Ok(ContributionState::Merged) => "merged",
        Ok(ContributionState::InProgress) => "in_progress",
        Ok(ContributionState::Approved) => "APPROVED",
        Ok(ContributionState::ChangesRequested) => "CHANGES_REQUESTED",
        Ok(ContributionState::Commented) => "COMMENTED",
        Ok(ContributionState::Pending) => "PENDING",
        Ok(ContributionState::Dismissed) => "DISMISSED",
        Ok(ContributionState::Done) => "done",
        _ => "\u{2014}",
    }
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
