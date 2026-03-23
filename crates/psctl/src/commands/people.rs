use anyhow::Result;
use ps_proto::canonical::prism::v1::{
    ListPeopleRequest, PaginationRequest, PersonFilter, Platform, SortOrder,
};

use crate::client::Clients;

pub async fn people(clients: &mut Clients, team: Option<&str>, unresolved: bool) -> Result<()> {
    let filter: i32 = if unresolved {
        PersonFilter::Unassigned.into()
    } else {
        PersonFilter::Unspecified.into()
    };

    let response = clients
        .org
        .list_people(ListPeopleRequest {
            active_only: Some(true),
            search: None,
            team_id: team.map(ToString::to_string),
            filter,
            pagination: Some(PaginationRequest {
                page_size: 200,
                ..PaginationRequest::default()
            }),
            sort: Some(SortOrder {
                field: "name".to_string(),
                ..SortOrder::default()
            }),
        })
        .await?
        .into_inner();

    if response.people.is_empty() {
        println!("No people found.");
        return Ok(());
    }

    println!(
        "{:<36}  {:<25}  {:<15}  {:<20}  IDENTITIES",
        "ID", "NAME", "LEVEL", "TEAM"
    );
    println!("{}", "\u{2500}".repeat(110));

    for person in &response.people {
        let identities: Vec<String> = person
            .identities
            .iter()
            .map(|i| {
                let platform_name = match Platform::try_from(i.platform) {
                    Ok(Platform::Github) => "github",
                    Ok(Platform::Jira) => "jira",
                    Ok(Platform::Discourse) => "discourse",
                    Ok(Platform::Launchpad) => "launchpad",
                    Ok(Platform::Mattermost) => "mattermost",
                    _ => "unknown",
                };
                format!("{platform_name}:{}", i.username)
            })
            .collect();

        println!(
            "{:<36}  {:<25}  {:<15}  {:<20}  {}",
            person.id,
            truncate(&person.name, 25),
            person.level.as_deref().unwrap_or("\u{2014}"),
            person.team_name.as_deref().unwrap_or("\u{2014}"),
            identities.join(", "),
        );
    }

    if let Some(pag) = &response.pagination {
        println!(
            "\nShowing {} of {} people.",
            response.people.len(),
            pag.total_count
        );
    }

    Ok(())
}

use crate::format::truncate;
