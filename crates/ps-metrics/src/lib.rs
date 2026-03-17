mod flow;

use std::cmp::Ordering;
use std::collections::HashMap;

use futures::stream::{self, TryStreamExt};
use ps_core::models::{ContributionType, PeriodType};
use ps_core::repo::Repos;
use ps_core::repo::metrics::{ContributionMetricRow, SnapshotInput};
use time::Date;
use tracing::info;
use uuid::Uuid;

pub use flow::{Throughput, compute_cross_source_throughput};

/// Review turnaround distribution: average + percentiles.
#[derive(Debug, Clone, Copy)]
pub struct ReviewTurnaround {
    pub avg: f32,
    pub p75: f32,
    pub p90: f32,
    pub p99: f32,
}

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
    period_type: PeriodType,
) -> Result<(), ps_core::Error> {
    let contributions = repos
        .metrics
        .get_team_contributions(team_id, period_start, period_end)
        .await?;

    let cross_throughput = flow::compute_cross_source_throughput(&contributions);
    let review = compute_review_turnaround(&contributions);
    let avg_cycle_time_hours = flow::compute_cycle_time(&contributions);
    let wip_avg = flow::compute_wip(&contributions, period_end);
    let lead_time_hours = flow::compute_lead_time(&contributions);
    let flow_efficiency = flow::compute_flow_efficiency(&contributions);

    let mut raw_metrics = serde_json::json!({
        "throughput_by_source": cross_throughput.by_source,
    });
    if let Some(r) = &review
        && let Some(obj) = raw_metrics.as_object_mut()
    {
        obj.insert(
            "review_turnaround_p75_hours".into(),
            serde_json::json!(r.p75),
        );
        obj.insert(
            "review_turnaround_p90_hours".into(),
            serde_json::json!(r.p90),
        );
        obj.insert(
            "review_turnaround_p99_hours".into(),
            serde_json::json!(r.p99),
        );
    }

    let snapshot_id = repos
        .metrics
        .upsert_snapshot(&SnapshotInput {
            id: Uuid::now_v7(),
            team_id,
            period_start,
            period_end,
            period_type,
            throughput: cross_throughput.total,
            avg_review_turnaround_hours: review.as_ref().map(|r| r.avg),
            avg_cycle_time_hours,
            wip_avg,
            flow_efficiency,
            lead_time_hours,
            raw_metrics,
        })
        .await?;

    // Populate snapshot_sources for traceability
    let contribution_ids: Vec<Uuid> = contributions.iter().map(|c| c.id).collect();
    repos.metrics.delete_snapshot_sources(snapshot_id).await?;
    repos
        .metrics
        .insert_snapshot_sources(snapshot_id, &contribution_ids)
        .await?;

    info!(
        %team_id,
        %period_start,
        %period_end,
        %period_type,
        throughput = cross_throughput.total,
        avg_review_turnaround_hours = ?review.as_ref().map(|r| r.avg),
        ?avg_cycle_time_hours,
        ?wip_avg,
        ?lead_time_hours,
        ?flow_efficiency,
        "computed team snapshot"
    );

    Ok(())
}

/// Compute snapshots for all teams for a given period.
pub async fn compute_all_snapshots(
    repos: &Repos,
    period_start: Date,
    period_end: Date,
    period_type: PeriodType,
) -> Result<i32, ps_core::Error> {
    let team_ids = repos.org.list_team_ids().await?;

    stream::iter(team_ids.iter().map(Ok))
        .try_for_each_concurrent(4, |&team_id| async move {
            compute_team_snapshot(repos, team_id, period_start, period_end, period_type).await
        })
        .await?;

    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let computed = team_ids.len() as i32;
    info!(computed, %period_type, %period_start, "computed all team snapshots");
    Ok(computed)
}

/// Hours from PR creation to first review: average + percentiles.
///
/// For each PR, finds the earliest `pr_review` with a matching `pr_platform_id`
/// in the metadata and computes the time delta. Returns `None` if no
/// PR-review pairs exist.
#[allow(clippy::cast_precision_loss)] // review turnaround doesn't need sub-second precision
fn compute_review_turnaround(contributions: &[ContributionMetricRow]) -> Option<ReviewTurnaround> {
    // Index reviews by their parent PR platform_id for O(1) lookup per PR.
    let mut reviews_by_pr: HashMap<&str, Vec<time::OffsetDateTime>> = HashMap::new();
    for c in contributions
        .iter()
        .filter(|c| c.contribution_type == ContributionType::PrReview)
    {
        if let Some(pr_platform_id) = c.metadata.get("pr_platform_id").and_then(|v| v.as_str()) {
            reviews_by_pr
                .entry(pr_platform_id)
                .or_default()
                .push(c.created_at);
        }
    }

    if reviews_by_pr.is_empty() {
        return None;
    }

    // For each PR, find earliest review and compute turnaround
    let mut turnaround_hours: Vec<f32> = Vec::new();

    for pr in contributions
        .iter()
        .filter(|c| c.contribution_type == ContributionType::PullRequest)
    {
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

        // Fallback: find the earliest review for THIS PR (matching platform_id)
        // that was created after the PR itself.
        if let Some(&earliest) = reviews_by_pr
            .get(pr.platform_id.as_str())
            .and_then(|times| times.iter().filter(|&&t| t >= pr.created_at).min())
        {
            let delta = earliest - pr.created_at;
            let hours = delta.whole_seconds() as f32 / 3600.0;
            if hours >= 0.0 {
                turnaround_hours.push(hours);
            }
        }
    }

    if turnaround_hours.is_empty() {
        return None;
    }

    turnaround_hours.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    let avg = turnaround_hours.iter().sum::<f32>() / turnaround_hours.len() as f32;
    let p75 = percentile(&turnaround_hours, 75.0);
    let p90 = percentile(&turnaround_hours, 90.0);
    let p99 = percentile(&turnaround_hours, 99.0);

    Some(ReviewTurnaround { avg, p75, p90, p99 })
}

/// Nearest-rank percentile on a pre-sorted slice.
#[allow(clippy::cast_precision_loss)]
fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() as f32 - 1.0)).ceil() as usize;
    sorted
        .get(idx.min(sorted.len() - 1))
        .copied()
        .unwrap_or(0.0)
}

/// Determine period boundaries for a given date and period type.
#[allow(clippy::expect_used)] // date arithmetic on known-valid values (day 1, known months)
pub fn period_boundaries(reference_date: Date, period_type: PeriodType) -> (Date, Date) {
    match period_type {
        PeriodType::Week => {
            let weekday = reference_date.weekday().number_days_from_monday();
            let start = reference_date - time::Duration::days(i64::from(weekday));
            let end = start + time::Duration::days(6);
            (start, end)
        }
        PeriodType::Month => {
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
        PeriodType::Quarter => {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ps_core::models::{ContributionState, Platform};
    use time::macros::date;

    #[test]
    fn test_week_boundaries() {
        // 2026-03-13 is a Friday
        let (start, end) = period_boundaries(date!(2026 - 03 - 13), PeriodType::Week);
        assert_eq!(start, date!(2026 - 03 - 09)); // Monday
        assert_eq!(end, date!(2026 - 03 - 15)); // Sunday
    }

    #[test]
    fn test_month_boundaries() {
        let (start, end) = period_boundaries(date!(2026 - 03 - 13), PeriodType::Month);
        assert_eq!(start, date!(2026 - 03 - 01));
        assert_eq!(end, date!(2026 - 03 - 31));
    }

    #[test]
    fn test_month_boundaries_december() {
        let (start, end) = period_boundaries(date!(2026 - 12 - 15), PeriodType::Month);
        assert_eq!(start, date!(2026 - 12 - 01));
        assert_eq!(end, date!(2026 - 12 - 31));
    }

    #[test]
    fn test_quarter_boundaries_q1() {
        let (start, end) = period_boundaries(date!(2026 - 02 - 15), PeriodType::Quarter);
        assert_eq!(start, date!(2026 - 01 - 01));
        assert_eq!(end, date!(2026 - 03 - 31));
    }

    #[test]
    fn test_quarter_boundaries_q4() {
        let (start, end) = period_boundaries(date!(2026 - 11 - 01), PeriodType::Quarter);
        assert_eq!(start, date!(2026 - 10 - 01));
        assert_eq!(end, date!(2026 - 12 - 31));
    }

    #[test]
    fn test_throughput_counts_merged_prs() {
        let contributions = vec![
            make_contribution(
                ContributionType::PullRequest,
                Some(ContributionState::Merged),
            ),
            make_contribution(
                ContributionType::PullRequest,
                Some(ContributionState::Merged),
            ),
            make_contribution(
                ContributionType::PullRequest,
                Some(ContributionState::Closed),
            ),
            make_contribution(ContributionType::PullRequest, Some(ContributionState::Open)),
            make_contribution(
                ContributionType::PrReview,
                Some(ContributionState::Approved),
            ),
        ];
        // Only merged PRs count as throughput
        assert_eq!(
            flow::compute_cross_source_throughput(&contributions).total,
            2
        );
    }

    #[test]
    fn test_throughput_empty() {
        assert_eq!(flow::compute_cross_source_throughput(&[]).total, 0);
    }

    #[test]
    fn test_review_turnaround_none_without_reviews() {
        let contributions = vec![make_contribution(
            ContributionType::PullRequest,
            Some(ContributionState::Merged),
        )];
        assert!(compute_review_turnaround(&contributions).is_none());
    }

    #[test]
    fn test_percentile_basic() {
        // 10 values: 1..=10
        let data: Vec<f32> = (1..=10).map(|i| i as f32).collect();
        assert!((percentile(&data, 75.0) - 8.0).abs() < f32::EPSILON);
        assert!((percentile(&data, 90.0) - 10.0).abs() < f32::EPSILON);
        assert!((percentile(&data, 99.0) - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_percentile_single() {
        let data = vec![5.0];
        assert!((percentile(&data, 75.0) - 5.0).abs() < f32::EPSILON);
        assert!((percentile(&data, 99.0) - 5.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_percentile_empty() {
        assert!((percentile(&[], 75.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn test_review_turnaround_no_cross_attribution() {
        // PR A was created at T=0, PR B at T=10h
        // Review for PR B was created at T=1h (matching pr_platform_id = "B")
        // Without proper filtering, PR A would incorrectly match Review-for-B
        let pr_a = ContributionMetricRow {
            id: Uuid::now_v7(),
            person_id: Some(Uuid::nil()),
            platform: Platform::Github,
            platform_id: "PR_A".into(),
            contribution_type: ContributionType::PullRequest,
            state: Some(ContributionState::Merged),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            closed_at: None,
            metrics: serde_json::json!({}),
            metadata: serde_json::json!({}),
            state_history: None,
        };
        let pr_b = ContributionMetricRow {
            id: Uuid::now_v7(),
            person_id: Some(Uuid::nil()),
            platform: Platform::Github,
            platform_id: "PR_B".into(),
            contribution_type: ContributionType::PullRequest,
            state: Some(ContributionState::Merged),
            created_at: time::OffsetDateTime::UNIX_EPOCH + time::Duration::hours(10),
            closed_at: None,
            metrics: serde_json::json!({}),
            metadata: serde_json::json!({}),
            state_history: None,
        };
        let review_for_b = ContributionMetricRow {
            id: Uuid::now_v7(),
            person_id: Some(Uuid::nil()),
            platform: Platform::Github,
            platform_id: "REVIEW_1".into(),
            contribution_type: ContributionType::PrReview,
            state: Some(ContributionState::Approved),
            created_at: time::OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1),
            closed_at: None,
            metrics: serde_json::json!({}),
            metadata: serde_json::json!({"pr_platform_id": "PR_B"}),
            state_history: None,
        };

        let contributions = vec![pr_a, pr_b, review_for_b];
        let result = compute_review_turnaround(&contributions);

        // Should not find a turnaround for PR A (no review matches PR_A).
        // Should not find a turnaround for PR B (review is before PR B's creation).
        // So no turnaround data should be computed.
        assert!(result.is_none());
    }

    fn make_contribution(
        contribution_type: ContributionType,
        state: Option<ContributionState>,
    ) -> ContributionMetricRow {
        ContributionMetricRow {
            id: Uuid::now_v7(),
            person_id: Some(Uuid::nil()),
            platform: Platform::Github,
            platform_id: Uuid::now_v7().to_string(),
            contribution_type,
            state,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            closed_at: None,
            metrics: serde_json::json!({}),
            metadata: serde_json::json!({}),
            state_history: None,
        }
    }
}
