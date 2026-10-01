use ps_core::models::PeriodType;
use ps_core::repo::Repos;
use ps_core::repo::insights::UpsertSnapshotParams;
use time::{Date, OffsetDateTime};
use tracing::{debug, info};
use uuid::Uuid;

/// Compute insight snapshots for all teams across the given period.
///
/// Returns the number of snapshots computed. Refreshes are serialized across
/// periods and workers, with one team (up to five aggregations) at a time.
pub async fn compute_all_snapshots(
    repos: &Repos,
    period_start: Date,
    period_end: Date,
    period_type: PeriodType,
) -> Result<i32, ps_core::Error> {
    let _refresh_lock = repos.metrics.lock_insight_refresh().await?;
    let _period_lock = repos
        .metrics
        .lock_snapshot_period(period_start, period_type, true)
        .await?;
    let team_ids = repos.org.list_team_ids().await?;

    let computed = i32::try_from(team_ids.len())
        .map_err(|error| ps_core::Error::Internal(error.to_string()))?;
    // Each team already fans out into five queries, some with parallel hashes.
    // Adding team concurrency multiplies database memory rather than just work.
    for team_id in team_ids {
        compute_team_snapshot(repos, team_id, period_start, period_end, period_type).await?;
    }

    info!(computed, %period_type, %period_start, "computed all insight snapshots");

    Ok(computed)
}

/// Compute and upsert an insight snapshot for a single team.
#[allow(clippy::cast_precision_loss)]
async fn compute_team_snapshot(
    repos: &Repos,
    team_id: Uuid,
    period_start: Date,
    period_end: Date,
    period_type: PeriodType,
) -> Result<(), ps_core::Error> {
    let insights = repos.insights.for_snapshot_period(period_end);
    let since = period_start_to_datetime(period_start);

    // All aggregation queries run in parallel — they're read-only.
    let (review_quality, significance, topics, depth_by_sig, coverage) = tokio::try_join!(
        insights.get_review_quality_for_team(team_id, true, since),
        insights.get_significance_for_team(team_id, true, since),
        insights.get_topic_categories_for_team(team_id, true, since),
        insights.get_depth_by_significance_for_team(team_id, true, since),
        insights.get_coverage_for_team(team_id, true, since),
    )?;

    let (total_contributions, enriched_contributions, by_type) = coverage;

    let total_reviews = review_quality.total_reviews;
    let rubber_stamp_pct = if total_reviews > 0 {
        Some(review_quality.depth_1 as f32 / total_reviews as f32 * 100.0)
    } else {
        None
    };
    let deep_review_pct = if total_reviews > 0 {
        Some(
            (review_quality.depth_4 + review_quality.depth_5) as f32 / total_reviews as f32 * 100.0,
        )
    } else {
        None
    };

    let coverage_json = serde_json::json!({
        "total_contributions": total_contributions,
        "enriched_contributions": enriched_contributions,
        "by_type": by_type.iter().map(|t| serde_json::json!({
            "enrichment_type": t.enrichment_type,
            "eligible": t.eligible,
            "enriched": t.enriched,
        })).collect::<Vec<_>>(),
    });

    let source_ids: Vec<_> = repos
        .metrics
        .get_team_contributions(team_id, period_start, period_end)
        .await?
        .into_iter()
        .map(|contribution| contribution.id)
        .collect();
    let raw_insights = serde_json::json!({
        "contribution_ids": source_ids,
        "topic_categories": topics.iter().map(|t| serde_json::json!({
            "category": t.category,
            "count": t.count,
        })).collect::<Vec<_>>(),
    });

    let params = UpsertSnapshotParams {
        team_id,
        period_start,
        period_end,
        period_type: period_type.as_str().to_string(),
        avg_review_depth: if total_reviews > 0 {
            Some(review_quality.avg_depth as f32)
        } else {
            None
        },
        review_count: total_reviews,
        rubber_stamp_pct,
        deep_review_pct,
        depth_distribution: vec![
            review_quality.depth_1,
            review_quality.depth_2,
            review_quality.depth_3,
            review_quality.depth_4,
            review_quality.depth_5,
        ],
        constructive_count: review_quality.constructive,
        neutral_count: review_quality.neutral,
        critical_count: review_quality.critical,
        hostile_count: review_quality.hostile,
        significant_count: significance.significant,
        notable_count: significance.notable,
        routine_count: significance.routine,
        avg_depth_on_significant: if depth_by_sig.significant_review_count > 0 {
            Some(depth_by_sig.avg_depth_significant as f32)
        } else {
            None
        },
        avg_depth_on_notable: if depth_by_sig.notable_review_count > 0 {
            Some(depth_by_sig.avg_depth_notable as f32)
        } else {
            None
        },
        avg_depth_on_routine: if depth_by_sig.routine_review_count > 0 {
            Some(depth_by_sig.avg_depth_routine as f32)
        } else {
            None
        },
        enrichment_coverage: coverage_json,
        raw_insights,
    };

    let snapshot_id = repos.insights.upsert_snapshot(&params).await?;
    repos
        .insights
        .replace_snapshot_sources(snapshot_id, team_id, period_start, period_end)
        .await?;

    debug!(%team_id, %period_type, "computed insight snapshot");
    Ok(())
}

/// Convert a period start date to an `OffsetDateTime` at midnight UTC.
fn period_start_to_datetime(date: Date) -> OffsetDateTime {
    date.midnight().assume_utc()
}
