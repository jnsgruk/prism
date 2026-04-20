use super::*;
use crate::backup::{IndividualProfileRow, SnapshotSourceRow};
use crate::models::{ContributionType, Platform};

use super::contributions::{ContributionDetailRow, ListPersonContributionsParams};

/// Activity summary for a person grouped by platform.
pub struct PersonActivityRow {
    pub platform: String,
    pub contribution_count: i32,
    pub pull_request_count: i32,
    pub pr_review_count: i32,
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
        let until = params.until;
        let sort_field = params.sort_field;
        let sort_desc = params.sort_desc;
        let page_size = params.page_size;
        let offset = params.offset;
        let state = params.state;
        let escaped_search = params.search.map(super::super::escape_like);
        let search = escaped_search.as_deref();

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
              AND ($2::text IS NULL OR c.platform ILIKE $2 OR c.platform ILIKE $2 || '-%')
              AND ($3::text IS NULL OR c.contribution_type LIKE $3)
              AND ($4::date IS NULL OR c.created_at >= $4::date::timestamptz)
              AND ($9::text IS NULL OR c.state = $9)
              AND ($10::text IS NULL OR (
                  c.title ILIKE '%' || $10 || '%'
                  OR c.metadata->>'repo' ILIKE '%' || $10 || '%'
              ))
              AND ($11::date IS NULL OR c.created_at < ($11::date + INTERVAL '1 day')::timestamptz)
            ORDER BY
              CASE WHEN $7 = 'platform' AND NOT $8 THEN c.platform END ASC NULLS LAST,
              CASE WHEN $7 = 'platform' AND $8 THEN c.platform END DESC NULLS LAST,
              CASE WHEN $7 = 'state' AND NOT $8 THEN c.state END ASC NULLS LAST,
              CASE WHEN $7 = 'state' AND $8 THEN c.state END DESC NULLS LAST,
              CASE WHEN $7 = 'repo' AND NOT $8 THEN c.metadata->>'repo' END ASC NULLS LAST,
              CASE WHEN $7 = 'repo' AND $8 THEN c.metadata->>'repo' END DESC NULLS LAST,
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
            state,
            search,
            until,
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
                    platform: r.platform.parse().unwrap_or(Platform::Github),
                    contribution_type: r
                        .contribution_type
                        .parse()
                        .unwrap_or(ContributionType::PullRequest),
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
                SUM(CASE WHEN c.contribution_type = 'pull_request' THEN 1 ELSE 0 END)::int AS "pull_request_count!",
                SUM(CASE WHEN c.contribution_type = 'pr_review' THEN 1 ELSE 0 END)::int AS "pr_review_count!",
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
                pull_request_count: r.pull_request_count,
                pr_review_count: r.pr_review_count,
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

    // -----------------------------------------------------------------------
    // Backup export/import — metrics.individual_profiles
    // -----------------------------------------------------------------------

    /// Count individual profiles (for backup manifest).
    pub async fn count_individual_profiles(&self) -> Result<i64, Error> {
        sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!: i64" FROM metrics.individual_profiles"#)
            .fetch_one(&self.pool)
            .await
            .map_err(Error::from)
    }

    /// Export all individual profiles for backup.
    pub async fn export_individual_profiles(&self) -> Result<Vec<IndividualProfileRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, person_id, period_start, period_end, period_type,
                   activity_summary, peer_comparison, computed_at
            FROM metrics.individual_profiles
            ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| IndividualProfileRow {
                id: r.id,
                person_id: r.person_id,
                period_start: r.period_start.to_string(),
                period_end: r.period_end.to_string(),
                period_type: r.period_type,
                activity_summary: r.activity_summary,
                peer_comparison: r.peer_comparison,
                computed_at: r.computed_at,
            })
            .collect())
    }

    /// Import individual profiles from backup. Upserts on
    /// `(person_id, period_start, period_type)`.
    pub async fn import_individual_profiles(
        &self,
        rows: &[IndividualProfileRow],
    ) -> Result<i64, Error> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mut count: i64 = 0;
        for chunk in rows.chunks(500) {
            let ids: Vec<Uuid> = chunk.iter().map(|r| r.id).collect();
            let person_ids: Vec<Uuid> = chunk.iter().map(|r| r.person_id).collect();
            let period_starts: Vec<Date> = chunk
                .iter()
                .map(|r| {
                    Date::parse(
                        &r.period_start,
                        &time::format_description::well_known::Iso8601::DEFAULT,
                    )
                    .unwrap_or(Date::MIN)
                })
                .collect();
            let period_ends: Vec<Date> = chunk
                .iter()
                .map(|r| {
                    Date::parse(
                        &r.period_end,
                        &time::format_description::well_known::Iso8601::DEFAULT,
                    )
                    .unwrap_or(Date::MIN)
                })
                .collect();
            let period_types: Vec<&str> = chunk.iter().map(|r| r.period_type.as_str()).collect();
            let activity_summaries: Vec<&serde_json::Value> =
                chunk.iter().map(|r| &r.activity_summary).collect();
            let peer_comparisons: Vec<&serde_json::Value> =
                chunk.iter().map(|r| &r.peer_comparison).collect();
            let computed_ats: Vec<time::OffsetDateTime> =
                chunk.iter().map(|r| r.computed_at).collect();

            sqlx::query!(
                r#"
                INSERT INTO metrics.individual_profiles
                    (id, person_id, period_start, period_end, period_type,
                     activity_summary, peer_comparison, computed_at)
                SELECT
                    unnest($1::uuid[]),
                    unnest($2::uuid[]),
                    unnest($3::date[]),
                    unnest($4::date[]),
                    unnest($5::text[]),
                    unnest($6::jsonb[]),
                    unnest($7::jsonb[]),
                    unnest($8::timestamptz[])
                ON CONFLICT (person_id, period_start, period_type) DO UPDATE
                    SET activity_summary = EXCLUDED.activity_summary,
                        peer_comparison  = EXCLUDED.peer_comparison,
                        computed_at      = EXCLUDED.computed_at
                "#,
                &ids,
                &person_ids,
                &period_starts,
                &period_ends,
                &period_types as &[&str],
                &activity_summaries as &[&serde_json::Value],
                &peer_comparisons as &[&serde_json::Value],
                &computed_ats,
            )
            .execute(&self.pool)
            .await
            .map_err(Error::from)?;

            count += i64::try_from(chunk.len()).unwrap_or(i64::MAX);
        }
        Ok(count)
    }

    // -----------------------------------------------------------------------
    // Backup export/import — metrics.snapshot_sources
    // -----------------------------------------------------------------------

    /// Count snapshot source rows (for backup manifest).
    pub async fn count_snapshot_sources(&self) -> Result<i64, Error> {
        sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!: i64" FROM metrics.snapshot_sources"#)
            .fetch_one(&self.pool)
            .await
            .map_err(Error::from)
    }

    /// Export all snapshot source rows for backup.
    pub async fn export_snapshot_sources(&self) -> Result<Vec<SnapshotSourceRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT snapshot_id, contribution_id
            FROM metrics.snapshot_sources
            ORDER BY snapshot_id, contribution_id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| SnapshotSourceRow {
                snapshot_id: r.snapshot_id,
                contribution_id: r.contribution_id,
            })
            .collect())
    }

    /// Import snapshot sources from backup. Uses `ON CONFLICT DO NOTHING`.
    pub async fn import_snapshot_sources(&self, rows: &[SnapshotSourceRow]) -> Result<i64, Error> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mut count: i64 = 0;
        for chunk in rows.chunks(1000) {
            let snapshot_ids: Vec<Uuid> = chunk.iter().map(|r| r.snapshot_id).collect();
            let contribution_ids: Vec<Uuid> = chunk.iter().map(|r| r.contribution_id).collect();

            sqlx::query!(
                r#"
                INSERT INTO metrics.snapshot_sources (snapshot_id, contribution_id)
                SELECT unnest($1::uuid[]), unnest($2::uuid[])
                ON CONFLICT DO NOTHING
                "#,
                &snapshot_ids,
                &contribution_ids,
            )
            .execute(&self.pool)
            .await
            .map_err(Error::from)?;

            count += i64::try_from(chunk.len()).unwrap_or(i64::MAX);
        }
        Ok(count)
    }
}
