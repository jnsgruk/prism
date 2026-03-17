//! Flow metric computations: cycle time, WIP, lead time, flow efficiency,
//! and cross-source throughput.

use std::collections::HashMap;

use ps_core::models::{ContributionState, ContributionType};
use ps_core::repo::metrics::ContributionMetricRow;
use time::Date;

/// Extract hours from completed Jira tickets that have a positive value for
/// the given metric key (e.g. `"cycle_time_hours"` or `"lead_time_hours"`).
fn completed_jira_metric_hours(
    contributions: &[ContributionMetricRow],
    metric_key: &str,
) -> Vec<f64> {
    contributions
        .iter()
        .filter(|c| {
            c.contribution_type == ContributionType::JiraTicket
                && matches!(
                    c.state,
                    Some(ContributionState::Closed | ContributionState::Merged)
                )
        })
        .filter_map(|c| {
            c.metrics
                .get(metric_key)
                .and_then(serde_json::Value::as_f64)
                .filter(|&h| h > 0.0)
        })
        .collect()
}

/// Cross-source throughput: total completed items + per-source breakdown.
pub struct Throughput {
    pub total: i32,
    pub by_source: serde_json::Value,
}

/// Compute average cycle time (hours) across completed Jira tickets.
///
/// Uses the pre-computed `cycle_time_hours` field stored during Jira ingestion.
#[allow(clippy::cast_precision_loss)]
pub fn compute_cycle_time(contributions: &[ContributionMetricRow]) -> Option<f32> {
    let cycle_times = completed_jira_metric_hours(contributions, "cycle_time_hours");

    if cycle_times.is_empty() {
        return None;
    }

    let avg = cycle_times.iter().sum::<f64>() / cycle_times.len() as f64;
    Some(avg as f32)
}

/// Compute average WIP (work in progress) for the period.
///
/// Counts items in non-terminal states: Jira tickets that are `InProgress`
/// and PRs that are `Open`, that were created before the period end and
/// not yet closed.
#[allow(clippy::cast_precision_loss)]
pub fn compute_wip(contributions: &[ContributionMetricRow], period_end: Date) -> Option<f32> {
    let period_end_dt = period_end
        .midnight()
        .assume_utc()
        .checked_add(time::Duration::days(1))?;

    let wip_count = contributions
        .iter()
        .filter(|c| {
            (matches!(c.contribution_type, ContributionType::JiraTicket)
                && c.state == Some(ContributionState::InProgress)
                || matches!(c.contribution_type, ContributionType::PullRequest)
                    && c.state == Some(ContributionState::Open))
                && c.created_at < period_end_dt
                && c.closed_at.is_none_or(|closed| closed >= period_end_dt)
        })
        .count();

    if wip_count == 0 {
        return None;
    }

    Some(wip_count as f32)
}

/// Compute average lead time (hours) across merged PRs and completed Jira tickets.
///
/// - For merged PRs: `closed_at - created_at`
/// - For completed Jira tickets: uses pre-computed `cycle_time_hours`
#[allow(clippy::cast_precision_loss)]
pub fn compute_lead_time(contributions: &[ContributionMetricRow]) -> Option<f32> {
    // Merged PR lead times: closed_at - created_at
    let pr_lead_times = contributions
        .iter()
        .filter(|c| {
            c.contribution_type == ContributionType::PullRequest
                && c.state == Some(ContributionState::Merged)
        })
        .filter_map(|c| {
            let hours = (c.closed_at? - c.created_at).whole_seconds() as f64 / 3600.0;
            (hours > 0.0).then_some(hours)
        });

    // Completed Jira ticket lead times from stored metric
    let jira_lead_times = completed_jira_metric_hours(contributions, "cycle_time_hours");

    let lead_times: Vec<f64> = pr_lead_times.chain(jira_lead_times).collect();

    if lead_times.is_empty() {
        return None;
    }

    let avg = lead_times.iter().sum::<f64>() / lead_times.len() as f64;
    Some(avg as f32)
}

/// Compute flow efficiency from Jira ticket state histories.
///
/// Flow efficiency = active time / total cycle time, where active time is
/// time spent in "in progress" / "review" / "development" status categories.
#[allow(clippy::cast_precision_loss)]
pub fn compute_flow_efficiency(contributions: &[ContributionMetricRow]) -> Option<f32> {
    let mut efficiencies: Vec<f64> = Vec::new();

    for c in contributions
        .iter()
        .filter(|c| c.contribution_type == ContributionType::JiraTicket)
        .filter(|c| {
            matches!(
                c.state,
                Some(ContributionState::Closed | ContributionState::Merged)
            )
        })
    {
        let Some(history) = c.state_history.as_ref().and_then(|v| v.as_array()) else {
            continue;
        };
        if history.len() < 2 {
            continue;
        }

        let total_hours = c
            .metrics
            .get("cycle_time_hours")
            .and_then(serde_json::Value::as_f64)
            .filter(|&h| h > 0.0);

        let Some(total_hours) = total_hours else {
            continue;
        };

        // Sum time spent in active states
        let mut active_seconds: f64 = 0.0;
        for pair in history.windows(2) {
            let (Some(current), Some(next)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            let state = current.get("state").and_then(|s| s.as_str()).unwrap_or("");
            let at_str = current.get("at").and_then(|s| s.as_str());
            let next_at_str = next.get("at").and_then(|s| s.as_str());

            let (Some(at_str), Some(next_at_str)) = (at_str, next_at_str) else {
                continue;
            };

            if is_active_state(state)
                && let (Ok(at), Ok(next_at)) = (
                    time::OffsetDateTime::parse(
                        at_str,
                        &time::format_description::well_known::Rfc3339,
                    ),
                    time::OffsetDateTime::parse(
                        next_at_str,
                        &time::format_description::well_known::Rfc3339,
                    ),
                )
            {
                active_seconds += (next_at - at).whole_seconds() as f64;
            }
        }

        let active_hours = active_seconds / 3600.0;
        if active_hours > 0.0 && active_hours <= total_hours {
            efficiencies.push(active_hours / total_hours);
        }
    }

    if efficiencies.is_empty() {
        return None;
    }

    let avg = efficiencies.iter().sum::<f64>() / efficiencies.len() as f64;
    Some(avg as f32)
}

/// Returns true if a Jira status name indicates active work.
fn is_active_state(state: &str) -> bool {
    let lower = state.to_lowercase();
    lower.contains("progress")
        || lower.contains("review")
        || lower.contains("development")
        || lower.contains("coding")
        || lower.contains("testing")
}

/// Count all completed items across sources with per-source breakdown.
///
/// Completed means:
/// - PRs: state == Merged
/// - Jira tickets: state == Closed
/// - Discourse topics: always counted (creation is the contribution)
#[allow(clippy::cast_possible_wrap)]
pub fn compute_cross_source_throughput(contributions: &[ContributionMetricRow]) -> Throughput {
    let mut by_source: HashMap<String, i32> = HashMap::new();
    let mut total = 0;

    for c in contributions {
        let counted = match c.contribution_type {
            ContributionType::PullRequest => c.state == Some(ContributionState::Merged),
            ContributionType::JiraTicket => matches!(
                c.state,
                Some(ContributionState::Closed | ContributionState::Merged)
            ),
            ContributionType::DiscourseTopic => true,
            _ => false,
        };

        if counted {
            total += 1;
            let source_key = c.platform.to_string();
            *by_source.entry(source_key).or_default() += 1;
        }
    }

    let by_source_json = serde_json::json!(by_source);
    Throughput {
        total,
        by_source: by_source_json,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ps_core::models::Platform;
    use ps_core::repo::metrics::ContributionMetricRow;
    use time::macros::datetime;
    use uuid::Uuid;

    fn make_jira_ticket(
        state: ContributionState,
        cycle_time_hours: Option<f64>,
        state_history: Option<serde_json::Value>,
    ) -> ContributionMetricRow {
        let mut metrics = serde_json::json!({});
        if let Some(hours) = cycle_time_hours {
            metrics["cycle_time_hours"] = serde_json::json!(hours);
        }
        ContributionMetricRow {
            id: Uuid::now_v7(),
            person_id: Some(Uuid::nil()),
            platform: Platform::Jira,
            platform_id: Uuid::now_v7().to_string(),
            contribution_type: ContributionType::JiraTicket,
            state: Some(state),
            created_at: datetime!(2026-03-01 0:00 UTC),
            closed_at: Some(datetime!(2026-03-05 0:00 UTC)),
            metrics,
            metadata: serde_json::json!({}),
            state_history,
        }
    }

    fn make_pr(
        state: ContributionState,
        created_at: time::OffsetDateTime,
        closed_at: Option<time::OffsetDateTime>,
    ) -> ContributionMetricRow {
        ContributionMetricRow {
            id: Uuid::now_v7(),
            person_id: Some(Uuid::nil()),
            platform: Platform::Github,
            platform_id: Uuid::now_v7().to_string(),
            contribution_type: ContributionType::PullRequest,
            state: Some(state),
            created_at,
            closed_at,
            metrics: serde_json::json!({}),
            metadata: serde_json::json!({}),
            state_history: None,
        }
    }

    // ---- cycle time ----

    #[test]
    fn cycle_time_averages_jira_tickets() {
        let contributions = vec![
            make_jira_ticket(ContributionState::Closed, Some(24.0), None),
            make_jira_ticket(ContributionState::Closed, Some(48.0), None),
        ];
        let result = compute_cycle_time(&contributions).unwrap();
        assert!((result - 36.0).abs() < 0.01);
    }

    #[test]
    fn cycle_time_none_without_jira() {
        let contributions = vec![make_pr(
            ContributionState::Merged,
            datetime!(2026-03-01 0:00 UTC),
            Some(datetime!(2026-03-02 0:00 UTC)),
        )];
        assert!(compute_cycle_time(&contributions).is_none());
    }

    #[test]
    fn cycle_time_skips_zero_values() {
        let contributions = vec![
            make_jira_ticket(ContributionState::Closed, Some(0.0), None),
            make_jira_ticket(ContributionState::Closed, Some(24.0), None),
        ];
        let result = compute_cycle_time(&contributions).unwrap();
        assert!((result - 24.0).abs() < 0.01);
    }

    // ---- WIP ----

    #[test]
    fn wip_counts_in_progress_items() {
        let contributions = vec![
            ContributionMetricRow {
                state: Some(ContributionState::InProgress),
                closed_at: None,
                ..make_jira_ticket(ContributionState::InProgress, None, None)
            },
            make_pr(
                ContributionState::Open,
                datetime!(2026-03-01 0:00 UTC),
                None,
            ),
            // Closed ticket should not count
            make_jira_ticket(ContributionState::Closed, Some(24.0), None),
        ];
        let result = compute_wip(&contributions, time::macros::date!(2026 - 03 - 15)).unwrap();
        assert!((result - 2.0).abs() < 0.01);
    }

    #[test]
    fn wip_none_when_all_closed() {
        let contributions = vec![make_jira_ticket(
            ContributionState::Closed,
            Some(24.0),
            None,
        )];
        assert!(compute_wip(&contributions, time::macros::date!(2026 - 03 - 15)).is_none());
    }

    // ---- lead time ----

    #[test]
    fn lead_time_from_prs_and_jira() {
        let contributions = vec![
            make_pr(
                ContributionState::Merged,
                datetime!(2026-03-01 0:00 UTC),
                Some(datetime!(2026-03-02 0:00 UTC)), // 24h
            ),
            make_jira_ticket(ContributionState::Closed, Some(48.0), None), // 48h
        ];
        let result = compute_lead_time(&contributions).unwrap();
        assert!((result - 36.0).abs() < 0.01); // (24+48)/2
    }

    #[test]
    fn lead_time_none_without_completed() {
        let contributions = vec![make_pr(
            ContributionState::Open,
            datetime!(2026-03-01 0:00 UTC),
            None,
        )];
        assert!(compute_lead_time(&contributions).is_none());
    }

    // ---- flow efficiency ----

    #[test]
    fn flow_efficiency_from_state_history() {
        let history = serde_json::json!([
            {"state": "To Do", "at": "2026-03-01T00:00:00Z"},
            {"state": "In Progress", "at": "2026-03-02T16:00:00Z"},
            {"state": "Done", "at": "2026-03-05T04:00:00Z"}
        ]);
        // Total cycle time: 100 hours (set in metrics).
        // Active time (In Progress → Done): Mar 2 16:00 to Mar 5 04:00 = 60 hours.
        // Efficiency: 60/100 = 0.6
        let contributions = vec![make_jira_ticket(
            ContributionState::Closed,
            Some(100.0),
            Some(history),
        )];
        let result = compute_flow_efficiency(&contributions).unwrap();
        assert!((result - 0.6).abs() < 0.01);
    }

    #[test]
    fn flow_efficiency_none_without_state_history() {
        let contributions = vec![make_jira_ticket(
            ContributionState::Closed,
            Some(48.0),
            None,
        )];
        assert!(compute_flow_efficiency(&contributions).is_none());
    }

    // ---- cross-source throughput ----

    #[test]
    fn throughput_counts_all_completed() {
        let contributions = vec![
            make_pr(
                ContributionState::Merged,
                datetime!(2026-03-01 0:00 UTC),
                Some(datetime!(2026-03-02 0:00 UTC)),
            ),
            make_pr(
                ContributionState::Open,
                datetime!(2026-03-01 0:00 UTC),
                None,
            ), // not counted
            make_jira_ticket(ContributionState::Closed, Some(24.0), None),
            ContributionMetricRow {
                platform: Platform::Discourse("ubuntu".into()),
                contribution_type: ContributionType::DiscourseTopic,
                ..make_jira_ticket(ContributionState::Open, None, None)
            },
        ];
        let result = compute_cross_source_throughput(&contributions);
        assert_eq!(result.total, 3);
        assert_eq!(result.by_source["github"], 1);
        assert_eq!(result.by_source["jira"], 1);
        assert_eq!(result.by_source["discourse-ubuntu"], 1);
    }

    #[test]
    fn throughput_empty() {
        let result = compute_cross_source_throughput(&[]);
        assert_eq!(result.total, 0);
    }
}
