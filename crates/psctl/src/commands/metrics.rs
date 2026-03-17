use anyhow::{Result, bail};
use ps_proto::prism::v1::{
    GetTeamMetricsRequest, GetTeamTreeRequest, ListPeriodsRequest, PeriodType,
};

use crate::client::Clients;

/// Resolve a team name to its UUID by searching the team tree.
async fn resolve_team_id(clients: &mut Clients, name: &str) -> Result<String> {
    // Try parsing as UUID first
    if uuid::Uuid::parse_str(name).is_ok() {
        return Ok(name.to_string());
    }

    let tree = clients
        .org
        .get_team_tree(GetTeamTreeRequest {})
        .await?
        .into_inner();

    fn find_team(teams: &[ps_proto::prism::v1::Team], name: &str) -> Option<String> {
        for team in teams {
            if team.name.eq_ignore_ascii_case(name) {
                return Some(team.id.clone());
            }
            if let Some(id) = find_team(&team.children, name) {
                return Some(id);
            }
        }
        None
    }

    find_team(&tree.roots, name).ok_or_else(|| anyhow::anyhow!("team not found: {name}"))
}

fn parse_period_type(s: &str) -> Result<PeriodType> {
    match s.to_lowercase().as_str() {
        "week" | "w" => Ok(PeriodType::Week),
        "month" | "m" => Ok(PeriodType::Month),
        "quarter" | "q" => Ok(PeriodType::Quarter),
        _ => bail!("invalid period type: {s} (expected week, month, or quarter)"),
    }
}

pub async fn metrics(clients: &mut Clients, team_name: &str, period: &str) -> Result<()> {
    let team_id = resolve_team_id(clients, team_name).await?;
    let period_type = parse_period_type(period)?;

    // Get the most recent period of the requested type from the server
    let periods = clients
        .metrics
        .list_periods(ListPeriodsRequest {})
        .await?
        .into_inner();

    let target_period = periods
        .periods
        .iter()
        .find(|p| p.r#type == i32::from(period_type))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no {period} periods with data found"))?;

    let response = clients
        .metrics
        .get_team_metrics(GetTeamMetricsRequest {
            team_id,
            period: Some(target_period),
        })
        .await?
        .into_inner();

    let Some(m) = response.metrics else {
        println!("No metrics available.");
        return Ok(());
    };

    println!("Team: {} ({})", m.team_name, m.team_id);
    if let Some(p) = &m.period {
        println!("Period: {} to {}", p.start, p.end);
    }
    println!("Members: {}", m.member_count);
    println!();
    println!("{:<30} {:>12}", "METRIC", "VALUE");
    println!("{}", "─".repeat(44));
    println!("{:<30} {:>12}", "Throughput", m.throughput);
    println!(
        "{:<30} {:>12}",
        "Avg review turnaround (h)",
        format_f32(m.avg_review_turnaround_hours)
    );
    println!(
        "{:<30} {:>12}",
        "Review P75 (h)",
        format_f32(m.review_turnaround_p75_hours)
    );
    println!(
        "{:<30} {:>12}",
        "Review P90 (h)",
        format_f32(m.review_turnaround_p90_hours)
    );
    println!(
        "{:<30} {:>12}",
        "Avg cycle time (h)",
        format_f32(m.avg_cycle_time_hours)
    );
    println!("{:<30} {:>12}", "WIP avg", format_f32(m.wip_avg));
    println!(
        "{:<30} {:>12}",
        "Flow efficiency",
        if m.flow_efficiency > 0.0 {
            format!("{:.0}%", m.flow_efficiency * 100.0)
        } else {
            "—".to_string()
        }
    );
    println!(
        "{:<30} {:>12}",
        "Lead time (h)",
        format_f32(m.lead_time_hours)
    );

    if !m.source_platforms.is_empty() {
        println!();
        println!("Sources: {}", m.source_platforms.join(", "));
    }

    Ok(())
}

fn format_f32(v: f32) -> String {
    if v > 0.0 {
        format!("{v:.1}")
    } else {
        "—".to_string()
    }
}
