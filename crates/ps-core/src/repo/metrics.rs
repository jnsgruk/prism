use crate::Error;
use crate::models::{ContributionState, ContributionType, PeriodType, Platform};
use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

/// Repository for the `metrics` schema: pre-computed team snapshots.
#[derive(Clone)]
pub struct MetricsRepo {
    pool: PgPool,
}

/// A row from `metrics.team_snapshots` joined with team name and member count.
pub struct TeamSnapshotRow {
    pub id: Uuid,
    pub team_id: Uuid,
    pub team_name: String,
    pub member_count: i32,
    pub period_start: Date,
    pub period_end: Date,
    pub period_type: PeriodType,
    pub throughput: Option<i32>,
    pub avg_review_turnaround_hours: Option<f32>,
    pub avg_cycle_time_hours: Option<f32>,
    pub wip_avg: Option<f32>,
    pub flow_efficiency: Option<f32>,
    pub lead_time_hours: Option<f32>,
    pub raw_metrics: serde_json::Value,
    pub source_platforms: Vec<String>,
}

/// Raw contribution data needed for metrics computation.
pub struct ContributionMetricRow {
    pub id: Uuid,
    pub person_id: Option<Uuid>,
    pub platform: Platform,
    pub platform_id: String,
    pub contribution_type: ContributionType,
    pub state: Option<ContributionState>,
    pub created_at: time::OffsetDateTime,
    pub closed_at: Option<time::OffsetDateTime>,
    pub metrics: serde_json::Value,
    pub metadata: serde_json::Value,
    pub state_history: Option<serde_json::Value>,
}

impl MetricsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get a team snapshot for a specific period.
    pub async fn get_team_snapshot(
        &self,
        team_id: Uuid,
        period_start: Date,
        period_type: PeriodType,
    ) -> Result<Option<TeamSnapshotRow>, Error> {
        let period_type_str = period_type.as_str();
        let row = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT ch.id FROM org.teams ch
                JOIN team_tree tt ON ch.parent_team_id = tt.id
            )
            SELECT ts.id, ts.team_id, t.name AS team_name,
                   (SELECT COUNT(DISTINCT tm.person_id)::int
                    FROM org.team_memberships tm
                    JOIN team_tree tt ON tm.team_id = tt.id
                    WHERE tm.end_date IS NULL OR tm.end_date > CURRENT_DATE) AS "member_count!",
                   ts.period_start, ts.period_end, ts.period_type,
                   ts.throughput, ts.avg_review_turnaround_hours,
                   ts.avg_cycle_time_hours, ts.wip_avg,
                   ts.flow_efficiency, ts.lead_time_hours,
                   ts.raw_metrics AS "raw_metrics!"
            FROM metrics.team_snapshots ts
            JOIN org.teams t ON t.id = ts.team_id
            WHERE ts.team_id = $1
              AND ts.period_start = $2
              AND ts.period_type = $3
            "#,
            team_id,
            period_start,
            period_type_str,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        let Some(snapshot) = row else {
            return Ok(None);
        };

        let source_platforms = self.get_snapshot_source_platforms(snapshot.id).await?;

        Ok(Some(TeamSnapshotRow {
            id: snapshot.id,
            team_id: snapshot.team_id,
            team_name: snapshot.team_name,
            member_count: snapshot.member_count,
            period_start: snapshot.period_start,
            period_end: snapshot.period_end,
            period_type: PeriodType::from_str_opt(&snapshot.period_type).unwrap_or(period_type),
            throughput: snapshot.throughput,
            avg_review_turnaround_hours: snapshot.avg_review_turnaround_hours,
            avg_cycle_time_hours: snapshot.avg_cycle_time_hours,
            wip_avg: snapshot.wip_avg,
            flow_efficiency: snapshot.flow_efficiency,
            lead_time_hours: snapshot.lead_time_hours,
            raw_metrics: snapshot.raw_metrics,
            source_platforms,
        }))
    }

    /// Get snapshots for multiple teams for a specific period.
    pub async fn compare_team_snapshots(
        &self,
        team_ids: &[Uuid],
        period_start: Date,
        period_type: PeriodType,
    ) -> Result<Vec<TeamSnapshotRow>, Error> {
        let period_type_str = period_type.as_str();
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id, id AS root_id FROM org.teams WHERE id = ANY($1)
                UNION ALL
                SELECT ch.id, tt.root_id
                FROM org.teams ch
                JOIN team_tree tt ON ch.parent_team_id = tt.id
            ),
            member_counts AS (
                SELECT tt.root_id AS team_id,
                       COUNT(DISTINCT tm.person_id)::int AS count
                FROM team_tree tt
                JOIN org.team_memberships tm ON tm.team_id = tt.id
                WHERE tm.end_date IS NULL OR tm.end_date > CURRENT_DATE
                GROUP BY tt.root_id
            )
            SELECT ts.id, ts.team_id, t.name AS team_name,
                   COALESCE(mc.count, 0) AS "member_count!",
                   ts.period_start, ts.period_end, ts.period_type,
                   ts.throughput, ts.avg_review_turnaround_hours,
                   ts.avg_cycle_time_hours, ts.wip_avg,
                   ts.flow_efficiency, ts.lead_time_hours,
                   ts.raw_metrics AS "raw_metrics!"
            FROM metrics.team_snapshots ts
            JOIN org.teams t ON t.id = ts.team_id
            LEFT JOIN member_counts mc ON mc.team_id = ts.team_id
            WHERE ts.team_id = ANY($1)
              AND ts.period_start = $2
              AND ts.period_type = $3
            ORDER BY t.name
            "#,
            team_ids,
            period_start,
            period_type_str,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let snapshot_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
        let source_map = self
            .get_bulk_snapshot_source_platforms(&snapshot_ids)
            .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let source_platforms = source_map.get(&r.id).cloned().unwrap_or_default();
                TeamSnapshotRow {
                    id: r.id,
                    team_id: r.team_id,
                    team_name: r.team_name,
                    member_count: r.member_count,
                    period_start: r.period_start,
                    period_end: r.period_end,
                    period_type: PeriodType::from_str_opt(&r.period_type).unwrap_or(period_type),
                    throughput: r.throughput,
                    avg_review_turnaround_hours: r.avg_review_turnaround_hours,
                    avg_cycle_time_hours: r.avg_cycle_time_hours,
                    wip_avg: r.wip_avg,
                    flow_efficiency: r.flow_efficiency,
                    lead_time_hours: r.lead_time_hours,
                    raw_metrics: r.raw_metrics,
                    source_platforms,
                }
            })
            .collect())
    }

    /// List periods that have snapshot data.
    pub async fn list_periods(&self) -> Result<Vec<PeriodRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT period_start, period_end, period_type
            FROM metrics.team_snapshots
            ORDER BY period_type, period_start DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                Some(PeriodRow {
                    start: r.period_start,
                    end: r.period_end,
                    period_type: PeriodType::from_str_opt(&r.period_type)?,
                })
            })
            .collect())
    }

    /// Upsert a team snapshot (used by metrics computation).
    ///
    /// Returns the actual snapshot ID (which may differ from `snap.id` if an
    /// existing row was updated via the ON CONFLICT clause).
    pub async fn upsert_snapshot(&self, snap: &SnapshotInput) -> Result<Uuid, Error> {
        let id = snap.id;
        let team_id = snap.team_id;
        let period_start = snap.period_start;
        let period_end = snap.period_end;
        let period_type = snap.period_type.as_str();
        let throughput = snap.throughput;
        let avg_review_turnaround_hours = snap.avg_review_turnaround_hours;
        let avg_cycle_time_hours = snap.avg_cycle_time_hours;
        let wip_avg = snap.wip_avg;
        let flow_efficiency = snap.flow_efficiency;
        let lead_time_hours = snap.lead_time_hours;
        let raw_metrics = &snap.raw_metrics;
        let row = sqlx::query_scalar!(
            r#"
            INSERT INTO metrics.team_snapshots (
                id, team_id, period_start, period_end, period_type,
                throughput, avg_review_turnaround_hours,
                avg_cycle_time_hours, wip_avg, flow_efficiency, lead_time_hours,
                raw_metrics, computed_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now())
            ON CONFLICT (team_id, period_start, period_type)
            DO UPDATE SET
                period_end = EXCLUDED.period_end,
                throughput = EXCLUDED.throughput,
                avg_review_turnaround_hours = EXCLUDED.avg_review_turnaround_hours,
                avg_cycle_time_hours = EXCLUDED.avg_cycle_time_hours,
                wip_avg = EXCLUDED.wip_avg,
                flow_efficiency = EXCLUDED.flow_efficiency,
                lead_time_hours = EXCLUDED.lead_time_hours,
                raw_metrics = EXCLUDED.raw_metrics,
                computed_at = now()
            RETURNING id
            "#,
            id,
            team_id,
            period_start,
            period_end,
            period_type,
            throughput,
            avg_review_turnaround_hours,
            avg_cycle_time_hours,
            wip_avg,
            flow_efficiency,
            lead_time_hours,
            raw_metrics,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row)
    }

    /// Get contributions for a team's members within a date range.
    ///
    /// Uses a recursive CTE to include members of all descendant teams so that
    /// parent/group-level metrics aggregate correctly.
    pub async fn get_team_contributions(
        &self,
        team_id: Uuid,
        period_start: Date,
        period_end: Date,
    ) -> Result<Vec<ContributionMetricRow>, Error> {
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
            )
            SELECT DISTINCT c.id, c.person_id, c.platform, c.platform_id,
                   c.contribution_type, c.state,
                   c.created_at, c.closed_at,
                   c.metrics, c.metadata, c.state_history
            FROM activity.contributions c
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
            JOIN team_tree tt ON tm.team_id = tt.id
            WHERE (tm.end_date IS NULL OR tm.end_date > $3::date)
              AND tm.start_date <= $3::date
              AND c.created_at >= $2::date::timestamptz
              AND c.created_at < ($3::date + INTERVAL '1 day')::timestamptz
            "#,
            team_id,
            period_start,
            period_end,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                Some(ContributionMetricRow {
                    id: r.id,
                    person_id: r.person_id,
                    platform: r.platform.parse().ok()?,
                    platform_id: r.platform_id,
                    contribution_type: r.contribution_type.parse().ok()?,
                    state: r.state.as_deref().and_then(ContributionState::from_str_opt),
                    created_at: r.created_at,
                    closed_at: r.closed_at,
                    metrics: r.metrics,
                    metadata: r.metadata,
                    state_history: r.state_history,
                })
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Discourse activity queries (leaf-team drilldown)
// ---------------------------------------------------------------------------

/// Category distribution for Discourse activity.
pub struct DiscourseCategoryRow {
    pub category: String,
    pub topic_count: i32,
    pub post_count: i32,
}

/// Daily Discourse activity data point.
pub struct DiscourseActivityRow {
    pub date: Date,
    pub topics: i32,
    pub posts: i32,
    pub likes: i32,
}

/// Top contributor for Discourse activity.
pub struct DiscourseContributorRow {
    pub person_id: Uuid,
    pub name: String,
    pub topics: i32,
    pub posts: i32,
    pub likes_received: i32,
}

impl MetricsRepo {
    /// Category distribution of Discourse topics and posts for a team's members.
    pub async fn get_discourse_category_distribution(
        &self,
        team_id: Uuid,
        period_start: Date,
        period_end: Date,
        instance: Option<&str>,
    ) -> Result<Vec<DiscourseCategoryRow>, Error> {
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
            )
            SELECT
                COALESCE(c.metrics->>'category', c.metadata->>'category', 'Uncategorized') AS "category!",
                SUM(CASE WHEN c.contribution_type = 'discourse_topic' THEN 1 ELSE 0 END)::int AS "topic_count!",
                SUM(CASE WHEN c.contribution_type = 'discourse_post' THEN 1 ELSE 0 END)::int AS "post_count!"
            FROM activity.contributions c
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
            JOIN team_tree tt ON tm.team_id = tt.id
            WHERE (tm.end_date IS NULL OR tm.end_date > $3::date)
              AND tm.start_date <= $3::date
              AND c.created_at >= $2::date::timestamptz
              AND c.created_at < ($3::date + INTERVAL '1 day')::timestamptz
              AND c.contribution_type IN ('discourse_topic', 'discourse_post')
              AND ($4::text IS NULL OR c.platform = $4)
            GROUP BY COALESCE(c.metrics->>'category', c.metadata->>'category', 'Uncategorized')
            ORDER BY "post_count!" DESC
            LIMIT 20
            "#,
            team_id,
            period_start,
            period_end,
            instance,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| DiscourseCategoryRow {
                category: r.category,
                topic_count: r.topic_count,
                post_count: r.post_count,
            })
            .collect())
    }

    /// Daily activity trend for Discourse contributions.
    pub async fn get_discourse_activity_trend(
        &self,
        team_id: Uuid,
        period_start: Date,
        period_end: Date,
        instance: Option<&str>,
    ) -> Result<Vec<DiscourseActivityRow>, Error> {
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
            )
            SELECT
                c.created_at::date AS "date!",
                SUM(CASE WHEN c.contribution_type = 'discourse_topic' THEN 1 ELSE 0 END)::int AS "topics!",
                SUM(CASE WHEN c.contribution_type = 'discourse_post' THEN 1 ELSE 0 END)::int AS "posts!",
                SUM(CASE WHEN c.contribution_type = 'discourse_like' THEN 1 ELSE 0 END)::int AS "likes!"
            FROM activity.contributions c
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
            JOIN team_tree tt ON tm.team_id = tt.id
            WHERE (tm.end_date IS NULL OR tm.end_date > $3::date)
              AND tm.start_date <= $3::date
              AND c.created_at >= $2::date::timestamptz
              AND c.created_at < ($3::date + INTERVAL '1 day')::timestamptz
              AND c.contribution_type IN ('discourse_topic', 'discourse_post', 'discourse_like')
              AND ($4::text IS NULL OR c.platform = $4)
            GROUP BY c.created_at::date
            ORDER BY "date!" ASC
            "#,
            team_id,
            period_start,
            period_end,
            instance,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| DiscourseActivityRow {
                date: r.date,
                topics: r.topics,
                posts: r.posts,
                likes: r.likes,
            })
            .collect())
    }

    /// Top Discourse contributors for a team.
    pub async fn get_discourse_top_contributors(
        &self,
        team_id: Uuid,
        period_start: Date,
        period_end: Date,
        instance: Option<&str>,
    ) -> Result<Vec<DiscourseContributorRow>, Error> {
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
            )
            SELECT
                p.id AS "person_id!",
                p.name AS "name!",
                SUM(CASE WHEN c.contribution_type = 'discourse_topic' THEN 1 ELSE 0 END)::int AS "topics!",
                SUM(CASE WHEN c.contribution_type = 'discourse_post' THEN 1 ELSE 0 END)::int AS "posts!",
                SUM(CASE WHEN c.contribution_type = 'discourse_post'
                    THEN COALESCE((c.metrics->>'likes')::int, 0) ELSE 0 END)::int AS "likes_received!"
            FROM activity.contributions c
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
            JOIN team_tree tt ON tm.team_id = tt.id
            JOIN org.people p ON p.id = c.person_id
            WHERE (tm.end_date IS NULL OR tm.end_date > $3::date)
              AND tm.start_date <= $3::date
              AND c.created_at >= $2::date::timestamptz
              AND c.created_at < ($3::date + INTERVAL '1 day')::timestamptz
              AND c.contribution_type IN ('discourse_topic', 'discourse_post')
              AND ($4::text IS NULL OR c.platform = $4)
            GROUP BY p.id, p.name
            ORDER BY (SUM(CASE WHEN c.contribution_type IN ('discourse_topic', 'discourse_post') THEN 1 ELSE 0 END)) DESC
            LIMIT 50
            "#,
            team_id,
            period_start,
            period_end,
            instance,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| DiscourseContributorRow {
                person_id: r.person_id,
                name: r.name,
                topics: r.topics,
                posts: r.posts,
                likes_received: r.likes_received,
            })
            .collect())
    }
}

/// Parameters for listing team contributions with filtering and pagination.
pub struct ListContributionsParams<'a> {
    pub team_id: Uuid,
    pub period_start: Date,
    pub period_end: Date,
    pub contribution_type: Option<&'a str>,
    pub state: Option<&'a str>,
    pub search: Option<&'a str>,
    pub sort_field: Option<&'a str>,
    pub sort_desc: bool,
    pub page_size: i32,
    pub offset: i32,
    pub platform: Option<&'a str>,
}

/// Parameters for listing a person's contributions with filtering and pagination.
pub struct ListPersonContributionsParams<'a> {
    pub person_id: Uuid,
    pub platform: Option<&'a str>,
    pub contribution_type: Option<&'a str>,
    pub since: Option<Date>,
    pub sort_field: Option<&'a str>,
    pub sort_desc: bool,
    pub page_size: i32,
    pub offset: i32,
}

/// Activity summary for a person grouped by platform.
pub struct PersonActivityRow {
    pub platform: String,
    pub contribution_count: i32,
    pub avg_review_hours: Option<f64>,
    pub avg_cycle_time_hours: Option<f64>,
}

/// Peer percentile data for a metric.
pub struct PeerPercentileRow {
    pub metric_name: String,
    pub value: f64,
    pub percentile: f64,
    pub peer_count: i32,
}

/// A detailed contribution row for drill-down display.
pub struct ContributionDetailRow {
    pub id: Uuid,
    pub person_name: String,
    pub platform: crate::models::Platform,
    pub contribution_type: crate::models::ContributionType,
    pub platform_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub state: Option<crate::models::ContributionState>,
    pub created_at: time::OffsetDateTime,
    pub closed_at: Option<time::OffsetDateTime>,
    pub metrics: serde_json::Value,
    pub metadata: serde_json::Value,
    pub total_count: i64,
}

impl MetricsRepo {
    /// List individual contributions for a team's members, with filtering and pagination.
    ///
    /// Uses the same recursive team-tree CTE as `get_team_contributions` but returns
    /// full contribution details for drill-down display.
    pub async fn list_team_contributions(
        &self,
        params: &ListContributionsParams<'_>,
    ) -> Result<(Vec<ContributionDetailRow>, i64), Error> {
        let team_id = params.team_id;
        let period_start = params.period_start;
        let period_end = params.period_end;
        let contribution_type = params.contribution_type;
        let state = params.state;
        let sort_field = params.sort_field;
        let sort_desc = params.sort_desc;
        let page_size = params.page_size;
        let offset = params.offset;
        let escaped_search = params.search.map(super::escape_like);
        let search = escaped_search.as_deref();
        let platform = params.platform;
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
            )
            SELECT c.id, p.name AS person_name, c.platform, c.contribution_type,
                   c.platform_id, c.title, c.url, c.state,
                   c.created_at, c.closed_at,
                   c.metrics, c.metadata,
                   COUNT(*) OVER() AS "total_count!"
            FROM activity.contributions c
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
                AND tm.team_id IN (SELECT id FROM team_tree)
                AND (tm.end_date IS NULL OR tm.end_date > $3::date)
                AND tm.start_date <= $3::date
            JOIN org.people p ON p.id = c.person_id
            WHERE c.created_at >= $2::date::timestamptz
              AND c.created_at < ($3::date + INTERVAL '1 day')::timestamptz
              AND ($4::text IS NULL OR c.contribution_type = $4)
              AND ($5::text IS NULL OR c.state = $5)
              AND ($8::text IS NULL OR (
                  c.title ILIKE '%' || $8 || '%'
                  OR p.name ILIKE '%' || $8 || '%'
                  OR c.metadata->>'repo' ILIKE '%' || $8 || '%'
              ))
              AND ($11::text IS NULL OR c.platform = $11)
            ORDER BY
              CASE WHEN $9 = 'person_name' AND NOT $10 THEN p.name END ASC NULLS LAST,
              CASE WHEN $9 = 'person_name' AND $10 THEN p.name END DESC NULLS LAST,
              CASE WHEN $9 = 'state' AND NOT $10 THEN c.state END ASC NULLS LAST,
              CASE WHEN $9 = 'state' AND $10 THEN c.state END DESC NULLS LAST,
              CASE WHEN $9 = 'repo' AND NOT $10 THEN c.metadata->>'repo' END ASC NULLS LAST,
              CASE WHEN $9 = 'repo' AND $10 THEN c.metadata->>'repo' END DESC NULLS LAST,
              CASE WHEN COALESCE($9, 'created_at') = 'created_at' AND NOT $10 THEN c.created_at END ASC,
              CASE WHEN COALESCE($9, 'created_at') = 'created_at' AND $10 THEN c.created_at END DESC
            LIMIT $6 OFFSET $7
            "#,
            team_id,
            period_start,
            period_end,
            contribution_type,
            state,
            page_size as i64,
            offset as i64,
            search,
            sort_field,
            sort_desc,
            platform,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let total_count = rows.first().map_or(0, |r| r.total_count);

        Ok((
            rows.into_iter()
                .map(|r| ContributionDetailRow {
                    id: r.id,
                    person_name: r.person_name,
                    platform: r
                        .platform
                        .parse()
                        .unwrap_or(crate::models::Platform::Github),
                    contribution_type: r
                        .contribution_type
                        .parse()
                        .unwrap_or(crate::models::ContributionType::PullRequest),
                    platform_id: r.platform_id,
                    title: r.title,
                    url: r.url,
                    state: r.state.and_then(|s| s.parse().ok()),
                    created_at: r.created_at,
                    closed_at: r.closed_at,
                    metrics: r.metrics,
                    metadata: r.metadata,
                    total_count,
                })
                .collect(),
            total_count,
        ))
    }
}

/// A distinct period that has snapshot data.
pub struct PeriodRow {
    pub start: Date,
    pub end: Date,
    pub period_type: PeriodType,
}

use std::collections::HashMap;

impl MetricsRepo {
    /// Record which contributions fed into a snapshot (traceability).
    pub async fn insert_snapshot_sources(
        &self,
        snapshot_id: Uuid,
        contribution_ids: &[Uuid],
    ) -> Result<(), Error> {
        if contribution_ids.is_empty() {
            return Ok(());
        }
        sqlx::query!(
            r#"
            INSERT INTO metrics.snapshot_sources (snapshot_id, contribution_id)
            SELECT $1, UNNEST($2::uuid[])
            ON CONFLICT DO NOTHING
            "#,
            snapshot_id,
            contribution_ids,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;
        Ok(())
    }

    /// Remove existing snapshot source links (for re-computation).
    pub async fn delete_snapshot_sources(&self, snapshot_id: Uuid) -> Result<(), Error> {
        sqlx::query!(
            "DELETE FROM metrics.snapshot_sources WHERE snapshot_id = $1",
            snapshot_id,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;
        Ok(())
    }

    /// Get distinct platforms contributing to a snapshot.
    async fn get_snapshot_source_platforms(&self, snapshot_id: Uuid) -> Result<Vec<String>, Error> {
        let rows = sqlx::query_scalar!(
            r#"
            SELECT DISTINCT c.platform AS "platform!"
            FROM metrics.snapshot_sources ss
            JOIN activity.contributions c ON c.id = ss.contribution_id
            WHERE ss.snapshot_id = $1
            ORDER BY c.platform
            "#,
            snapshot_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;
        Ok(rows)
    }

    /// Get distinct platforms for multiple snapshots in bulk.
    async fn get_bulk_snapshot_source_platforms(
        &self,
        snapshot_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<String>>, Error> {
        if snapshot_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query!(
            r#"
            SELECT ss.snapshot_id, c.platform
            FROM metrics.snapshot_sources ss
            JOIN activity.contributions c ON c.id = ss.contribution_id
            WHERE ss.snapshot_id = ANY($1)
            GROUP BY ss.snapshot_id, c.platform
            ORDER BY ss.snapshot_id, c.platform
            "#,
            snapshot_ids,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let mut map: HashMap<Uuid, Vec<String>> = HashMap::new();
        for r in rows {
            map.entry(r.snapshot_id).or_default().push(r.platform);
        }
        Ok(map)
    }

    /// Get the last N snapshots for a team and period type (for trend data).
    pub async fn get_snapshot_history(
        &self,
        team_id: Uuid,
        period_type: PeriodType,
        limit: i32,
    ) -> Result<Vec<TeamSnapshotRow>, Error> {
        let period_type_str = period_type.as_str();
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT ch.id FROM org.teams ch
                JOIN team_tree tt ON ch.parent_team_id = tt.id
            )
            SELECT ts.id, ts.team_id, t.name AS team_name,
                   (SELECT COUNT(DISTINCT tm.person_id)::int
                    FROM org.team_memberships tm
                    JOIN team_tree tt ON tm.team_id = tt.id
                    WHERE tm.end_date IS NULL OR tm.end_date > CURRENT_DATE) AS "member_count!",
                   ts.period_start, ts.period_end, ts.period_type,
                   ts.throughput, ts.avg_review_turnaround_hours,
                   ts.avg_cycle_time_hours, ts.wip_avg,
                   ts.flow_efficiency, ts.lead_time_hours,
                   ts.raw_metrics AS "raw_metrics!"
            FROM metrics.team_snapshots ts
            JOIN org.teams t ON t.id = ts.team_id
            WHERE ts.team_id = $1
              AND ts.period_type = $2
            ORDER BY ts.period_start DESC
            LIMIT $3
            "#,
            team_id,
            period_type_str,
            i64::from(limit),
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| TeamSnapshotRow {
                id: r.id,
                team_id: r.team_id,
                team_name: r.team_name,
                member_count: r.member_count,
                period_start: r.period_start,
                period_end: r.period_end,
                period_type: PeriodType::from_str_opt(&r.period_type).unwrap_or(period_type),
                throughput: r.throughput,
                avg_review_turnaround_hours: r.avg_review_turnaround_hours,
                avg_cycle_time_hours: r.avg_cycle_time_hours,
                wip_avg: r.wip_avg,
                flow_efficiency: r.flow_efficiency,
                lead_time_hours: r.lead_time_hours,
                raw_metrics: r.raw_metrics,
                source_platforms: Vec::new(), // not needed for trend data
            })
            .collect())
    }
}

/// Input for upserting a team snapshot.
pub struct SnapshotInput {
    pub id: Uuid,
    pub team_id: Uuid,
    pub period_start: Date,
    pub period_end: Date,
    pub period_type: PeriodType,
    pub throughput: i32,
    pub avg_review_turnaround_hours: Option<f32>,
    pub avg_cycle_time_hours: Option<f32>,
    pub wip_avg: Option<f32>,
    pub flow_efficiency: Option<f32>,
    pub lead_time_hours: Option<f32>,
    pub raw_metrics: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Individual profile queries
// ---------------------------------------------------------------------------

impl MetricsRepo {
    /// List contributions for a specific person, with filtering and pagination.
    pub async fn list_person_contributions(
        &self,
        params: &ListPersonContributionsParams<'_>,
    ) -> Result<(Vec<ContributionDetailRow>, i64), Error> {
        let person_id = params.person_id;
        let platform = params.platform;
        let contribution_type = params.contribution_type;
        let since = params.since;
        let sort_field = params.sort_field;
        let sort_desc = params.sort_desc;
        let page_size = params.page_size;
        let offset = params.offset;

        let rows = sqlx::query!(
            r#"
            SELECT c.id, p.name AS person_name, c.platform, c.contribution_type,
                   c.platform_id, c.title, c.url, c.state,
                   c.created_at, c.closed_at,
                   c.metrics, c.metadata,
                   COUNT(*) OVER() AS "total_count!"
            FROM activity.contributions c
            JOIN org.people p ON p.id = c.person_id
            WHERE c.person_id = $1
              AND ($2::text IS NULL OR c.platform = $2)
              AND ($3::text IS NULL OR c.contribution_type = $3)
              AND ($4::date IS NULL OR c.created_at >= $4::date::timestamptz)
            ORDER BY
              CASE WHEN $7 = 'platform' AND NOT $8 THEN c.platform END ASC NULLS LAST,
              CASE WHEN $7 = 'platform' AND $8 THEN c.platform END DESC NULLS LAST,
              CASE WHEN $7 = 'state' AND NOT $8 THEN c.state END ASC NULLS LAST,
              CASE WHEN $7 = 'state' AND $8 THEN c.state END DESC NULLS LAST,
              CASE WHEN COALESCE($7, 'created_at') = 'created_at' AND NOT $8 THEN c.created_at END ASC,
              CASE WHEN COALESCE($7, 'created_at') = 'created_at' AND $8 THEN c.created_at END DESC
            LIMIT $5 OFFSET $6
            "#,
            person_id,
            platform,
            contribution_type,
            since,
            page_size as i64,
            offset as i64,
            sort_field,
            sort_desc,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let total_count = rows.first().map_or(0, |r| r.total_count);

        Ok((
            rows.into_iter()
                .map(|r| ContributionDetailRow {
                    id: r.id,
                    person_name: r.person_name,
                    platform: r
                        .platform
                        .parse()
                        .unwrap_or(crate::models::Platform::Github),
                    contribution_type: r
                        .contribution_type
                        .parse()
                        .unwrap_or(crate::models::ContributionType::PullRequest),
                    platform_id: r.platform_id,
                    title: r.title,
                    url: r.url,
                    state: r.state.and_then(|s| s.parse().ok()),
                    created_at: r.created_at,
                    closed_at: r.closed_at,
                    metrics: r.metrics,
                    metadata: r.metadata,
                    total_count,
                })
                .collect(),
            total_count,
        ))
    }

    /// Get activity summary for a person, grouped by platform.
    pub async fn get_person_activity_summary(
        &self,
        person_id: Uuid,
        period_start: Date,
        period_end: Date,
    ) -> Result<Vec<PersonActivityRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                c.platform AS "platform!",
                COUNT(*)::int AS "contribution_count!",
                AVG(CASE WHEN c.metrics ? 'review_hours'
                    THEN (c.metrics->>'review_hours')::float8 END) AS avg_review_hours,
                AVG(CASE WHEN c.metrics ? 'cycle_time_hours'
                    THEN (c.metrics->>'cycle_time_hours')::float8 END) AS avg_cycle_time_hours
            FROM activity.contributions c
            WHERE c.person_id = $1
              AND c.created_at >= $2::date::timestamptz
              AND c.created_at < ($3::date + INTERVAL '1 day')::timestamptz
            GROUP BY c.platform
            ORDER BY "contribution_count!" DESC
            "#,
            person_id,
            period_start,
            period_end,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| PersonActivityRow {
                platform: r.platform,
                contribution_count: r.contribution_count,
                avg_review_hours: r.avg_review_hours,
                avg_cycle_time_hours: r.avg_cycle_time_hours,
            })
            .collect())
    }

    /// Compute peer percentiles for a person's throughput relative to same-level peers.
    ///
    /// Returns the person's contribution count and their percentile rank among
    /// people with the same `level` who have contributions in the same period.
    pub async fn compute_peer_percentiles(
        &self,
        person_id: Uuid,
        level: &str,
        period_start: Date,
        period_end: Date,
    ) -> Result<Option<(i64, f64, i32)>, Error> {
        // Get contribution counts for all people at this level in this period
        let row = sqlx::query!(
            r#"
            WITH peer_counts AS (
                SELECT c.person_id, COUNT(*)::bigint AS cnt
                FROM activity.contributions c
                JOIN org.people p ON p.id = c.person_id
                WHERE p.level = $1
                  AND c.created_at >= $2::date::timestamptz
                  AND c.created_at < ($3::date + INTERVAL '1 day')::timestamptz
                  AND c.person_id IS NOT NULL
                GROUP BY c.person_id
            )
            SELECT
                (SELECT cnt FROM peer_counts WHERE person_id = $4) AS "person_count?",
                (SELECT COUNT(*)::int FROM peer_counts) AS "peer_count!",
                (SELECT COUNT(*)::int FROM peer_counts
                 WHERE cnt <= (SELECT cnt FROM peer_counts WHERE person_id = $4)) AS "rank!"
            "#,
            level,
            period_start,
            period_end,
            person_id,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        let Some(person_count) = row.person_count else {
            return Ok(None);
        };

        let peer_count = row.peer_count;
        let percentile = if peer_count > 0 {
            f64::from(row.rank) / f64::from(peer_count)
        } else {
            0.0
        };

        Ok(Some((person_count, percentile, peer_count)))
    }
}
