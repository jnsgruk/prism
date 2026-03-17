use anyhow::Result;
use ps_proto::prism::v1::ListPersonContributionsRequest;

use crate::client::Clients;
use crate::format;

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
            platform: platform.map(ToString::to_string),
            contribution_type: None,
            since: since.map(ToString::to_string),
            page_size: 50,
            page_index: 0,
            sort_field: None,
            sort_desc: Some(true),
            state: None,
            search: None,
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
            c.platform,
            c.contribution_type,
            truncate(&c.title, 40),
            if c.state.is_empty() {
                "\u{2014}"
            } else {
                &c.state
            },
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
