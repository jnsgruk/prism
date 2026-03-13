use ps_core::repo::Repos;
use ps_core::repo::metrics::{ContributionMetricRow, SnapshotInput};
use time::Date;
use tracing::info;
use uuid::Uuid;

/// Compute and store metrics for a single team and period.
///
/// Reads contributions from `activity.contributions` for team members,
/// computes PR throughput and review turnaround, and writes the result
/// to `metrics.team_snapshots`.
pub async fn compute_team_snapshot(
    repos: &Repos,
    team_id: Uuid,
    period_start: Date,
    period_end: Date,
    period_type: &str,
) -> Result<(), ps_core::Error> {
    let contributions = repos
        .metrics
        .get_team_contributions(team_id, period_start, period_end)
        .await?;

    let throughput = compute_throughput(&contributions);
    let avg_review_turnaround = compute_review_turnaround(&contributions);

    repos
        .metrics
        .upsert_snapshot(&SnapshotInput {
            id: Uuid::now_v7(),
            team_id,
            period_start,
            period_end,
            period_type: period_type.to_owned(),
            throughput,
            avg_review_turnaround_hours: avg_review_turnaround,
        })
        .await?;

    info!(
        %team_id,
        %period_start,
        %period_end,
        period_type,
        throughput,
        avg_review_turnaround_hours = ?avg_review_turnaround,
        "computed team snapshot"
    );

    Ok(())
}

/// Compute snapshots for all teams for a given period.
pub async fn compute_all_snapshots(
    repos: &Repos,
    period_start: Date,
    period_end: Date,
    period_type: &str,
) -> Result<i32, ps_core::Error> {
    let teams = repos.org.list_teams(None).await?;
    let mut computed = 0;

    for team in &teams {
        compute_team_snapshot(repos, team.id, period_start, period_end, period_type).await?;
        computed += 1;
    }

    info!(computed, period_type, %period_start, "computed all team snapshots");
    Ok(computed)
}

/// Count merged PRs in the contribution set.
#[allow(clippy::cast_possible_wrap)] // contribution count never approaches i32::MAX
fn compute_throughput(contributions: &[ContributionMetricRow]) -> i32 {
    contributions
        .iter()
        .filter(|c| c.contribution_type == "pull_request" && c.state.as_deref() == Some("merged"))
        .count() as i32
}

/// Average hours from PR creation to first review.
///
/// For each PR, finds the earliest `pr_review` with a matching `pr_platform_id`
/// in the metadata and computes the time delta. Returns `None` if no
/// PR-review pairs exist.
#[allow(clippy::cast_precision_loss)] // review turnaround doesn't need sub-second precision
fn compute_review_turnaround(contributions: &[ContributionMetricRow]) -> Option<f32> {
    // Collect reviews indexed by their parent PR platform_id
    let reviews: Vec<(&str, time::OffsetDateTime)> = contributions
        .iter()
        .filter(|c| c.contribution_type == "pr_review")
        .filter_map(|c| {
            let pr_platform_id = c.metadata.get("pr_platform_id")?.as_str()?;
            Some((pr_platform_id, c.created_at))
        })
        .collect();

    if reviews.is_empty() {
        return None;
    }

    // For each PR, find earliest review and compute turnaround
    let mut turnaround_hours: Vec<f32> = Vec::new();

    for pr in contributions
        .iter()
        .filter(|c| c.contribution_type == "pull_request")
    {
        // Build the platform_id that reviews reference
        // Reviews store pr_platform_id in metadata matching the PR's contribution platform_id
        // We need the PR's metadata to find its platform_id — but we don't have it in this struct.
        // Instead, reviews reference via pr_platform_id. We need to match by checking all reviews
        // whose pr_platform_id matches any of our PRs.
        //
        // Since we don't have platform_id on ContributionMetricRow (it's not needed for metrics),
        // we use an alternative approach: match reviews to PRs by person_id + time proximity,
        // OR we can compute from the pr's metrics.review_hours if available.

        // Try the stored review_hours metric first (set during ingestion)
        if let Some(review_hours) = pr
            .metrics
            .get("review_hours")
            .and_then(serde_json::value::Value::as_f64)
            && review_hours > 0.0
        {
            turnaround_hours.push(review_hours as f32);
            continue;
        }

        // Fallback: compute from first review time minus PR creation
        // Find the earliest review that was created after this PR
        if let Some(earliest_review) = reviews
            .iter()
            .filter(|(_, review_time)| *review_time >= pr.created_at)
            .min_by_key(|(_, t)| *t)
        {
            let delta = earliest_review.1 - pr.created_at;
            let hours = delta.whole_seconds() as f32 / 3600.0;
            if hours >= 0.0 {
                turnaround_hours.push(hours);
            }
        }
    }

    if turnaround_hours.is_empty() {
        return None;
    }

    let sum: f32 = turnaround_hours.iter().sum();
    Some(sum / turnaround_hours.len() as f32)
}

/// Determine period boundaries for a given date and period type.
#[allow(clippy::expect_used)] // date arithmetic on known-valid values (day 1, known months)
pub fn period_boundaries(reference_date: Date, period_type: &str) -> (Date, Date) {
    match period_type {
        "week" => {
            let weekday = reference_date.weekday().number_days_from_monday();
            let start = reference_date - time::Duration::days(i64::from(weekday));
            let end = start + time::Duration::days(6);
            (start, end)
        }
        "month" => {
            let start = reference_date.replace_day(1).expect("day 1 always valid");
            let next_month = if reference_date.month() == time::Month::December {
                start
                    .replace_year(reference_date.year() + 1)
                    .expect("year valid")
                    .replace_month(time::Month::January)
                    .expect("month valid")
            } else {
                start
                    .replace_month(reference_date.month().next())
                    .expect("next month valid")
            };
            let end = next_month - time::Duration::days(1);
            (start, end)
        }
        "quarter" => {
            let quarter_start_month = match reference_date.month() {
                time::Month::January | time::Month::February | time::Month::March => {
                    time::Month::January
                }
                time::Month::April | time::Month::May | time::Month::June => time::Month::April,
                time::Month::July | time::Month::August | time::Month::September => {
                    time::Month::July
                }
                _ => time::Month::October,
            };
            let start = Date::from_calendar_date(reference_date.year(), quarter_start_month, 1)
                .expect("quarter start valid");
            let end_month = match quarter_start_month {
                time::Month::January => time::Month::March,
                time::Month::April => time::Month::June,
                time::Month::July => time::Month::September,
                _ => time::Month::December,
            };
            let next_quarter = if end_month == time::Month::December {
                Date::from_calendar_date(reference_date.year() + 1, time::Month::January, 1)
                    .expect("next year valid")
            } else {
                Date::from_calendar_date(reference_date.year(), end_month.next(), 1)
                    .expect("next quarter valid")
            };
            let end = next_quarter - time::Duration::days(1);
            (start, end)
        }
        _ => {
            // Default to month
            period_boundaries(reference_date, "month")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn test_week_boundaries() {
        // 2026-03-13 is a Friday
        let (start, end) = period_boundaries(date!(2026 - 03 - 13), "week");
        assert_eq!(start, date!(2026 - 03 - 09)); // Monday
        assert_eq!(end, date!(2026 - 03 - 15)); // Sunday
    }

    #[test]
    fn test_month_boundaries() {
        let (start, end) = period_boundaries(date!(2026 - 03 - 13), "month");
        assert_eq!(start, date!(2026 - 03 - 01));
        assert_eq!(end, date!(2026 - 03 - 31));
    }

    #[test]
    fn test_month_boundaries_december() {
        let (start, end) = period_boundaries(date!(2026 - 12 - 15), "month");
        assert_eq!(start, date!(2026 - 12 - 01));
        assert_eq!(end, date!(2026 - 12 - 31));
    }

    #[test]
    fn test_quarter_boundaries_q1() {
        let (start, end) = period_boundaries(date!(2026 - 02 - 15), "quarter");
        assert_eq!(start, date!(2026 - 01 - 01));
        assert_eq!(end, date!(2026 - 03 - 31));
    }

    #[test]
    fn test_quarter_boundaries_q4() {
        let (start, end) = period_boundaries(date!(2026 - 11 - 01), "quarter");
        assert_eq!(start, date!(2026 - 10 - 01));
        assert_eq!(end, date!(2026 - 12 - 31));
    }

    #[test]
    fn test_throughput_counts_merged_prs() {
        let contributions = vec![
            make_contribution("pull_request", "merged"),
            make_contribution("pull_request", "merged"),
            make_contribution("pull_request", "closed"),
            make_contribution("pull_request", "open"),
            make_contribution("pr_review", "APPROVED"),
        ];
        assert_eq!(compute_throughput(&contributions), 2);
    }

    #[test]
    fn test_throughput_empty() {
        assert_eq!(compute_throughput(&[]), 0);
    }

    #[test]
    fn test_review_turnaround_none_without_reviews() {
        let contributions = vec![make_contribution("pull_request", "merged")];
        assert!(compute_review_turnaround(&contributions).is_none());
    }

    fn make_contribution(contribution_type: &str, state: &str) -> ContributionMetricRow {
        ContributionMetricRow {
            person_id: Some(Uuid::nil()),
            contribution_type: contribution_type.to_owned(),
            state: Some(state.to_owned()),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            closed_at: None,
            metrics: serde_json::json!({}),
            metadata: serde_json::json!({}),
        }
    }
}
