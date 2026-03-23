mod grpc;

use ps_core::repo::Repos;
use ps_core::repo::metrics::{ContributionDetailRow, ContributionFullRow};
use ps_proto::canonical::prism::v1::{
    Contribution, DiscourseInstanceMetrics as ProtoDiscourseInstanceMetrics, PeriodType,
    TeamMetrics,
};
use time::Date;
use time::macros::format_description;
use tonic::Status;

use super::common::to_timestamp;

/// ISO 8601 date-only format (YYYY-MM-DD).
const DATE_FMT: &[time::format_description::BorrowedFormatItem<'_>] =
    format_description!("[year]-[month]-[day]");

pub struct MetricsServiceImpl {
    repos: Repos,
}

impl MetricsServiceImpl {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }
}

#[allow(clippy::result_large_err)]
fn parse_period_type(pt: i32) -> Result<ps_core::models::PeriodType, Status> {
    match PeriodType::try_from(pt) {
        Ok(PeriodType::Week) => Ok(ps_core::models::PeriodType::Week),
        Ok(PeriodType::Month) => Ok(ps_core::models::PeriodType::Month),
        Ok(PeriodType::Quarter) => Ok(ps_core::models::PeriodType::Quarter),
        _ => Err(Status::invalid_argument("invalid period_type")),
    }
}

fn period_type_to_proto(pt: ps_core::models::PeriodType) -> PeriodType {
    match pt {
        ps_core::models::PeriodType::Week => PeriodType::Week,
        ps_core::models::PeriodType::Month => PeriodType::Month,
        ps_core::models::PeriodType::Quarter => PeriodType::Quarter,
    }
}

#[allow(clippy::result_large_err)]
fn parse_date(s: &str) -> Result<Date, Status> {
    Date::parse(s, DATE_FMT).map_err(|_| Status::invalid_argument(format!("invalid date: {s}")))
}

fn format_date(d: Date) -> String {
    d.format(DATE_FMT).unwrap_or_else(|_| d.to_string())
}

/// Extract an f32 metric from `raw_metrics` JSON.
///
/// Truncating `f64 as f32` is acceptable here: metric values (hours,
/// percentages) fit comfortably within f32 range, and proto uses float32.
fn json_f32(v: &serde_json::Value, key: &str) -> f32 {
    v.get(key)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0) as f32
}

/// Extract an i32 metric from `raw_metrics` JSON.
///
/// Truncating `i64 as i32` is acceptable here: metric counts (PRs, posts,
/// likes) are always well within i32 range.
fn json_i32(v: &serde_json::Value, key: &str) -> i32 {
    v.get(key).and_then(serde_json::Value::as_i64).unwrap_or(0) as i32
}

fn snapshot_to_proto(s: ps_core::repo::metrics::TeamSnapshotRow) -> TeamMetrics {
    // Parse per-instance Discourse breakdown from raw_metrics JSON
    let discourse_by_instance = s
        .raw_metrics
        .get("discourse_by_instance")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .map(|(instance, v)| ProtoDiscourseInstanceMetrics {
                    instance: instance.clone(),
                    topics_created: json_i32(v, "topics_created"),
                    posts: json_i32(v, "posts"),
                    replies: json_i32(v, "replies"),
                    likes_given: json_i32(v, "likes_given"),
                    solved_topics: json_i32(v, "solved_topics"),
                })
                .collect()
        })
        .unwrap_or_default();

    TeamMetrics {
        team_id: s.team_id.to_string(),
        team_name: s.team_name,
        period: Some(ps_proto::canonical::prism::v1::Period {
            r#type: period_type_to_proto(s.period_type).into(),
            start: format_date(s.period_start),
            end: format_date(s.period_end),
        }),
        throughput: s.throughput.unwrap_or(0),
        avg_review_turnaround_hours: s.avg_review_turnaround_hours.unwrap_or(0.0),
        member_count: s.member_count,
        review_turnaround_p75_hours: json_f32(&s.raw_metrics, "review_turnaround_p75_hours"),
        review_turnaround_p90_hours: json_f32(&s.raw_metrics, "review_turnaround_p90_hours"),
        review_turnaround_p99_hours: json_f32(&s.raw_metrics, "review_turnaround_p99_hours"),
        raw_metrics: std::collections::HashMap::default(),
        avg_cycle_time_hours: s.avg_cycle_time_hours.unwrap_or(0.0),
        wip_avg: s.wip_avg.unwrap_or(0.0),
        flow_efficiency: s.flow_efficiency.unwrap_or(0.0),
        lead_time_hours: s.lead_time_hours.unwrap_or(0.0),
        source_platforms: s.source_platforms.clone(),
        discourse_topics_created: json_i32(&s.raw_metrics, "discourse_topics_created"),
        discourse_posts: json_i32(&s.raw_metrics, "discourse_posts"),
        discourse_replies: json_i32(&s.raw_metrics, "discourse_replies"),
        discourse_likes_given: json_i32(&s.raw_metrics, "discourse_likes_given"),
        discourse_likes_received: json_i32(&s.raw_metrics, "discourse_likes_received"),
        discourse_solved_topics: json_i32(&s.raw_metrics, "discourse_solved_topics"),
        discourse_active_participants: json_i32(&s.raw_metrics, "discourse_active_participants"),
        discourse_by_instance,
    }
}

/// Convert a `ContributionFullRow` to a proto `Contribution` with all detail fields.
fn contribution_full_to_proto(r: ContributionFullRow) -> Contribution {
    let repo = if let Some(s) = r.metadata.get("repo").and_then(|v| v.as_str()) {
        s.to_string()
    } else {
        match r.platform_id.splitn(3, '/').collect::<Vec<_>>().as_slice() {
            [owner, repo, ..] => format!("{owner}/{repo}"),
            _ => String::new(),
        }
    };
    let is_discourse = r.platform.is_discourse();
    let review_count = if is_discourse {
        json_i32(&r.metrics, "post_count")
    } else {
        json_i32(&r.metrics, "review_count")
    };
    let category = if is_discourse {
        r.metrics
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };

    let labels = r
        .metrics
        .get("labels")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let head_ref = r
        .metrics
        .get("head_ref")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let base_ref = r
        .metrics
        .get("base_ref")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let draft = r
        .metrics
        .get("draft")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    Contribution {
        id: r.id.to_string(),
        person_name: r.person_name,
        person_id: r.person_id.map(|id| id.to_string()).unwrap_or_default(),
        platform: r.platform.to_string(),
        contribution_type: r.contribution_type.to_string(),
        platform_id: r.platform_id,
        title: r.title.unwrap_or_default(),
        url: r.url.unwrap_or_default(),
        state: r.state.map(|s| s.to_string()).unwrap_or_default(),
        created_at: Some(to_timestamp(r.created_at)),
        updated_at: r.updated_at.map(to_timestamp),
        closed_at: r.closed_at.map(to_timestamp),
        content: r.content.unwrap_or_default(),
        additions: json_i32(&r.metrics, "additions"),
        deletions: json_i32(&r.metrics, "deletions"),
        changed_files: json_i32(&r.metrics, "changed_files"),
        review_count,
        review_hours: json_f32(&r.metrics, "review_hours"),
        repo,
        category,
        labels,
        head_ref,
        base_ref,
        draft,
    }
}

/// Convert a `ContributionDetailRow` to a proto `Contribution`.
///
/// Used by both `list_team_contributions` and `list_person_contributions`.
fn contribution_detail_to_proto(r: ContributionDetailRow) -> Contribution {
    let repo = if let Some(s) = r.metadata.get("repo").and_then(|v| v.as_str()) {
        s.to_string()
    } else {
        // Fallback: extract "owner/repo" from platform_id (e.g. "owner/repo/pull/123")
        match r.platform_id.splitn(3, '/').collect::<Vec<_>>().as_slice() {
            [owner, repo, ..] => format!("{owner}/{repo}"),
            _ => String::new(),
        }
    };
    // For discourse contributions, map post_count to review_count
    // and extract category from metrics.
    let is_discourse = r.platform.is_discourse();
    let review_count = if is_discourse {
        json_i32(&r.metrics, "post_count")
    } else {
        json_i32(&r.metrics, "review_count")
    };
    let category = if is_discourse {
        r.metrics
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };

    Contribution {
        id: r.id.to_string(),
        person_name: r.person_name,
        platform: r.platform.to_string(),
        contribution_type: r.contribution_type.to_string(),
        platform_id: r.platform_id,
        title: r.title.unwrap_or_default(),
        url: r.url.unwrap_or_default(),
        state: r.state.map(|s| s.to_string()).unwrap_or_default(),
        created_at: Some(to_timestamp(r.created_at)),
        closed_at: r.closed_at.map(to_timestamp),
        additions: json_i32(&r.metrics, "additions"),
        deletions: json_i32(&r.metrics, "deletions"),
        changed_files: json_i32(&r.metrics, "changed_files"),
        review_count,
        review_hours: json_f32(&r.metrics, "review_hours"),
        repo,
        category,
        // Detail-only fields — empty in list RPCs.
        content: String::new(),
        updated_at: None,
        person_id: String::new(),
        labels: Vec::new(),
        head_ref: String::new(),
        base_ref: String::new(),
        draft: false,
    }
}
