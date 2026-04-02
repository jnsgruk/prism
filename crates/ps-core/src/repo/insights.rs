use crate::Error;
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

/// Repository for read-only enrichment aggregation queries.
///
/// Consumes `reasoning.enrichments` joined with `activity.contributions`,
/// `org.people`, and `org.team_memberships` to produce insight summaries
/// for teams, individuals, and org-wide views.
#[derive(Clone)]
pub struct InsightsRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Review depth distribution and sentiment counts for a scope.
pub struct ReviewQualityRow {
    pub avg_depth: f64,
    pub total_reviews: i32,
    pub depth_1: i32,
    pub depth_2: i32,
    pub depth_3: i32,
    pub depth_4: i32,
    pub depth_5: i32,
    pub constructive: i32,
    pub neutral: i32,
    pub critical: i32,
    pub hostile: i32,
}

/// A top reviewer by average depth.
pub struct ReviewerDepthRow {
    pub person_id: Uuid,
    pub person_name: String,
    pub review_count: i32,
    pub avg_depth: f64,
}

/// PR significance counts.
pub struct SignificanceRow {
    pub significant: i32,
    pub notable: i32,
    pub routine: i32,
    pub avg_confidence: f64,
}

/// Discourse topic category with count.
pub struct TopicCategoryRow {
    pub category: String,
    pub count: i32,
}

/// A notable/exemplary contribution surfaced by enrichments.
pub struct NotableContributionRow {
    pub contribution_id: Uuid,
    pub title: String,
    pub url: String,
    pub person_name: String,
    pub platform: String,
    pub contribution_type: String,
    pub enrichment_type: String,
    pub value_summary: String,
    pub rationale: String,
    pub confidence: f64,
}

/// Coverage stats for a single enrichment type.
pub struct TypeCoverageRow {
    pub enrichment_type: String,
    pub eligible: i32,
    pub enriched: i32,
}

/// Depth × significance cross-reference.
pub struct DepthBySignificanceRow {
    pub avg_depth_significant: f64,
    pub avg_depth_notable: f64,
    pub avg_depth_routine: f64,
    pub significant_review_count: i32,
    pub notable_review_count: i32,
    pub routine_review_count: i32,
}

/// Reviews received summary for an individual.
pub struct ReviewsReceivedRow {
    pub avg_depth_received: f64,
    pub total_reviews_received: i32,
    pub deep_review_pct: f64,
}

/// Parameters for upserting an insight snapshot.
pub struct UpsertSnapshotParams {
    pub team_id: Uuid,
    pub period_start: Date,
    pub period_end: Date,
    pub period_type: String,
    pub avg_review_depth: Option<f32>,
    pub review_count: i32,
    pub rubber_stamp_pct: Option<f32>,
    pub deep_review_pct: Option<f32>,
    pub depth_distribution: Vec<i32>,
    pub constructive_count: i32,
    pub neutral_count: i32,
    pub critical_count: i32,
    pub hostile_count: i32,
    pub significant_count: i32,
    pub notable_count: i32,
    pub routine_count: i32,
    pub avg_depth_on_significant: Option<f32>,
    pub avg_depth_on_notable: Option<f32>,
    pub avg_depth_on_routine: Option<f32>,
    pub enrichment_coverage: serde_json::Value,
    pub raw_insights: serde_json::Value,
}

/// A stored insight snapshot for a team and period.
pub struct SnapshotRow {
    pub avg_review_depth: Option<f32>,
    pub review_count: i32,
    pub rubber_stamp_pct: Option<f32>,
    pub deep_review_pct: Option<f32>,
    pub significant_count: i32,
    pub notable_count: i32,
    pub routine_count: i32,
}

/// Enrichment-based peer percentile for a single metric.
pub struct EnrichmentPeerPercentile {
    pub metric_name: String,
    pub value: f64,
    pub percentile: f64,
    pub peer_count: i32,
}

impl InsightsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // -----------------------------------------------------------------------
    // Review quality
    // -----------------------------------------------------------------------

    /// Review depth distribution + sentiment for a team (with optional descendants).
    pub async fn get_review_quality_for_team(
        &self,
        team_id: Uuid,
        include_descendants: bool,
        since: OffsetDateTime,
    ) -> Result<ReviewQualityRow, Error> {
        // When include_descendants is false, we still use the CTE but it
        // only matches the single team_id.
        let row = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
                WHERE $3
            )
            SELECT
                COALESCE(AVG((e.value->>'score')::double precision), 0.0) AS "avg_depth!: f64",
                COUNT(*)::int AS "total_reviews!: i32",
                COUNT(*) FILTER (WHERE (e.value->>'score')::int = 1)::int AS "depth_1!: i32",
                COUNT(*) FILTER (WHERE (e.value->>'score')::int = 2)::int AS "depth_2!: i32",
                COUNT(*) FILTER (WHERE (e.value->>'score')::int = 3)::int AS "depth_3!: i32",
                COUNT(*) FILTER (WHERE (e.value->>'score')::int = 4)::int AS "depth_4!: i32",
                COUNT(*) FILTER (WHERE (e.value->>'score')::int = 5)::int AS "depth_5!: i32",
                -- Sentiment (from separate enrichment rows, counted via subquery)
                0::int AS "constructive!: i32",
                0::int AS "neutral!: i32",
                0::int AS "critical!: i32",
                0::int AS "hostile!: i32"
            FROM reasoning.enrichments e
            JOIN activity.contributions c ON c.id = e.contribution_id
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
            JOIN team_tree tt ON tm.team_id = tt.id
            WHERE e.enrichment_type = 'review_depth'
              AND c.created_at >= $2
              AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            "#,
            team_id,
            since,
            include_descendants,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        // Fetch sentiment counts separately (different enrichment type).
        let sentiment = self
            .get_sentiment_counts_for_team(team_id, include_descendants, since)
            .await?;

        Ok(ReviewQualityRow {
            avg_depth: row.avg_depth,
            total_reviews: row.total_reviews,
            depth_1: row.depth_1,
            depth_2: row.depth_2,
            depth_3: row.depth_3,
            depth_4: row.depth_4,
            depth_5: row.depth_5,
            constructive: sentiment.0,
            neutral: sentiment.1,
            critical: sentiment.2,
            hostile: sentiment.3,
        })
    }

    /// Sentiment counts for a team scope.
    async fn get_sentiment_counts_for_team(
        &self,
        team_id: Uuid,
        include_descendants: bool,
        since: OffsetDateTime,
    ) -> Result<(i32, i32, i32, i32), Error> {
        let row = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
                WHERE $3
            )
            SELECT
                COUNT(*) FILTER (WHERE e.value->>'sentiment' = 'constructive')::int AS "constructive!: i32",
                COUNT(*) FILTER (WHERE e.value->>'sentiment' = 'neutral')::int AS "neutral!: i32",
                COUNT(*) FILTER (WHERE e.value->>'sentiment' = 'critical')::int AS "critical!: i32",
                COUNT(*) FILTER (WHERE e.value->>'sentiment' = 'hostile')::int AS "hostile!: i32"
            FROM reasoning.enrichments e
            JOIN activity.contributions c ON c.id = e.contribution_id
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
            JOIN team_tree tt ON tm.team_id = tt.id
            WHERE e.enrichment_type = 'sentiment'
              AND c.created_at >= $2
              AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            "#,
            team_id,
            since,
            include_descendants,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok((row.constructive, row.neutral, row.critical, row.hostile))
    }

    /// Review quality for an individual (their reviews of others).
    pub async fn get_review_quality_for_person(
        &self,
        person_id: Uuid,
        since: OffsetDateTime,
    ) -> Result<ReviewQualityRow, Error> {
        let row = sqlx::query!(
            r#"
            SELECT
                COALESCE(AVG((e.value->>'score')::double precision), 0.0) AS "avg_depth!: f64",
                COUNT(*)::int AS "total_reviews!: i32",
                COUNT(*) FILTER (WHERE (e.value->>'score')::int = 1)::int AS "depth_1!: i32",
                COUNT(*) FILTER (WHERE (e.value->>'score')::int = 2)::int AS "depth_2!: i32",
                COUNT(*) FILTER (WHERE (e.value->>'score')::int = 3)::int AS "depth_3!: i32",
                COUNT(*) FILTER (WHERE (e.value->>'score')::int = 4)::int AS "depth_4!: i32",
                COUNT(*) FILTER (WHERE (e.value->>'score')::int = 5)::int AS "depth_5!: i32"
            FROM reasoning.enrichments e
            JOIN activity.contributions c ON c.id = e.contribution_id
            WHERE e.enrichment_type = 'review_depth'
              AND c.person_id = $1
              AND c.created_at >= $2
            "#,
            person_id,
            since,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        // Sentiment for person's reviews
        let sentiment = sqlx::query!(
            r#"
            SELECT
                COUNT(*) FILTER (WHERE e.value->>'sentiment' = 'constructive')::int AS "constructive!: i32",
                COUNT(*) FILTER (WHERE e.value->>'sentiment' = 'neutral')::int AS "neutral!: i32",
                COUNT(*) FILTER (WHERE e.value->>'sentiment' = 'critical')::int AS "critical!: i32",
                COUNT(*) FILTER (WHERE e.value->>'sentiment' = 'hostile')::int AS "hostile!: i32"
            FROM reasoning.enrichments e
            JOIN activity.contributions c ON c.id = e.contribution_id
            WHERE e.enrichment_type = 'sentiment'
              AND c.person_id = $1
              AND c.created_at >= $2
            "#,
            person_id,
            since,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(ReviewQualityRow {
            avg_depth: row.avg_depth,
            total_reviews: row.total_reviews,
            depth_1: row.depth_1,
            depth_2: row.depth_2,
            depth_3: row.depth_3,
            depth_4: row.depth_4,
            depth_5: row.depth_5,
            constructive: sentiment.constructive,
            neutral: sentiment.neutral,
            critical: sentiment.critical,
            hostile: sentiment.hostile,
        })
    }

    // -----------------------------------------------------------------------
    // Reviews received (for individual view)
    // -----------------------------------------------------------------------

    /// Summary of review quality received on a person's PRs.
    pub async fn get_reviews_received_for_person(
        &self,
        person_id: Uuid,
        since: OffsetDateTime,
    ) -> Result<ReviewsReceivedRow, Error> {
        // Reviews on this person's PRs: find reviews whose pr_platform_id
        // matches a PR authored by this person.
        let row = sqlx::query!(
            r#"
            SELECT
                COALESCE(AVG((e.value->>'score')::double precision), 0.0) AS "avg_depth!: f64",
                COUNT(*)::int AS "total!: i32",
                COUNT(*) FILTER (WHERE (e.value->>'score')::int >= 4)::int AS "deep!: i32"
            FROM reasoning.enrichments e
            JOIN activity.contributions review ON review.id = e.contribution_id
            JOIN activity.contributions pr
                ON pr.platform = review.platform
                AND pr.platform_id = review.metrics->>'pr_platform_id'
                AND pr.contribution_type = 'pull_request'
            WHERE e.enrichment_type = 'review_depth'
              AND pr.person_id = $1
              AND review.created_at >= $2
            "#,
            person_id,
            since,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        let total = row.total;
        let deep_pct = if total > 0 {
            f64::from(row.deep) / f64::from(total) * 100.0
        } else {
            0.0
        };

        Ok(ReviewsReceivedRow {
            avg_depth_received: row.avg_depth,
            total_reviews_received: total,
            deep_review_pct: deep_pct,
        })
    }

    // -----------------------------------------------------------------------
    // Top reviewers
    // -----------------------------------------------------------------------

    /// Top reviewers by average depth for a team scope.
    pub async fn get_top_reviewers(
        &self,
        team_id: Uuid,
        include_descendants: bool,
        since: OffsetDateTime,
        min_reviews: i64,
        limit: i64,
    ) -> Result<Vec<ReviewerDepthRow>, Error> {
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
                WHERE $3
            )
            SELECT
                p.id AS "person_id!: Uuid",
                p.name AS "person_name!: String",
                COUNT(*)::int AS "review_count!: i32",
                AVG((e.value->>'score')::double precision) AS "avg_depth!: f64"
            FROM reasoning.enrichments e
            JOIN activity.contributions c ON c.id = e.contribution_id
            JOIN org.people p ON p.id = c.person_id
            JOIN org.team_memberships tm ON tm.person_id = p.id
            JOIN team_tree tt ON tm.team_id = tt.id
            WHERE e.enrichment_type = 'review_depth'
              AND c.created_at >= $2
              AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            GROUP BY p.id, p.name
            HAVING COUNT(*) >= $4
            ORDER BY AVG((e.value->>'score')::double precision) DESC
            LIMIT $5
            "#,
            team_id,
            since,
            include_descendants,
            min_reviews,
            limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| ReviewerDepthRow {
                person_id: r.person_id,
                person_name: r.person_name,
                review_count: r.review_count,
                avg_depth: r.avg_depth,
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // Significance
    // -----------------------------------------------------------------------

    /// PR significance counts for a team scope.
    pub async fn get_significance_for_team(
        &self,
        team_id: Uuid,
        include_descendants: bool,
        since: OffsetDateTime,
    ) -> Result<SignificanceRow, Error> {
        let row = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
                WHERE $3
            )
            SELECT
                COUNT(*) FILTER (WHERE e.value->>'significance' = 'significant')::int AS "significant!: i32",
                COUNT(*) FILTER (WHERE e.value->>'significance' = 'notable')::int AS "notable!: i32",
                COUNT(*) FILTER (WHERE e.value->>'significance' = 'routine')::int AS "routine!: i32",
                COALESCE(AVG(e.confidence), 0.0)::double precision AS "avg_confidence!: f64"
            FROM reasoning.enrichments e
            JOIN activity.contributions c ON c.id = e.contribution_id
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
            JOIN team_tree tt ON tm.team_id = tt.id
            WHERE e.enrichment_type = 'significance'
              AND c.created_at >= $2
              AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            "#,
            team_id,
            since,
            include_descendants,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(SignificanceRow {
            significant: row.significant,
            notable: row.notable,
            routine: row.routine,
            avg_confidence: row.avg_confidence,
        })
    }

    /// PR significance counts for an individual.
    pub async fn get_significance_for_person(
        &self,
        person_id: Uuid,
        since: OffsetDateTime,
    ) -> Result<SignificanceRow, Error> {
        let row = sqlx::query!(
            r#"
            SELECT
                COUNT(*) FILTER (WHERE e.value->>'significance' = 'significant')::int AS "significant!: i32",
                COUNT(*) FILTER (WHERE e.value->>'significance' = 'notable')::int AS "notable!: i32",
                COUNT(*) FILTER (WHERE e.value->>'significance' = 'routine')::int AS "routine!: i32",
                COALESCE(AVG(e.confidence), 0.0)::double precision AS "avg_confidence!: f64"
            FROM reasoning.enrichments e
            JOIN activity.contributions c ON c.id = e.contribution_id
            WHERE e.enrichment_type = 'significance'
              AND c.person_id = $1
              AND c.created_at >= $2
            "#,
            person_id,
            since,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(SignificanceRow {
            significant: row.significant,
            notable: row.notable,
            routine: row.routine,
            avg_confidence: row.avg_confidence,
        })
    }

    // -----------------------------------------------------------------------
    // Topic categories
    // -----------------------------------------------------------------------

    /// Discourse topic categories for a team scope.
    pub async fn get_topic_categories_for_team(
        &self,
        team_id: Uuid,
        include_descendants: bool,
        since: OffsetDateTime,
    ) -> Result<Vec<TopicCategoryRow>, Error> {
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
                WHERE $3
            )
            SELECT
                COALESCE(e.value->>'primary_category', 'unknown') AS "category!: String",
                COUNT(*)::int AS "count!: i32"
            FROM reasoning.enrichments e
            JOIN activity.contributions c ON c.id = e.contribution_id
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
            JOIN team_tree tt ON tm.team_id = tt.id
            WHERE e.enrichment_type = 'topic'
              AND c.created_at >= $2
              AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            GROUP BY COALESCE(e.value->>'primary_category', 'unknown')
            ORDER BY COUNT(*) DESC
            "#,
            team_id,
            since,
            include_descendants,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| TopicCategoryRow {
                category: r.category,
                count: r.count,
            })
            .collect())
    }

    /// Discourse topic categories for an individual.
    pub async fn get_topic_categories_for_person(
        &self,
        person_id: Uuid,
        since: OffsetDateTime,
    ) -> Result<Vec<TopicCategoryRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                COALESCE(e.value->>'primary_category', 'unknown') AS "category!: String",
                COUNT(*)::int AS "count!: i32"
            FROM reasoning.enrichments e
            JOIN activity.contributions c ON c.id = e.contribution_id
            WHERE e.enrichment_type = 'topic'
              AND c.person_id = $1
              AND c.created_at >= $2
            GROUP BY COALESCE(e.value->>'primary_category', 'unknown')
            ORDER BY COUNT(*) DESC
            "#,
            person_id,
            since,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| TopicCategoryRow {
                category: r.category,
                count: r.count,
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // Notable contributions
    // -----------------------------------------------------------------------

    /// Highest-signal contributions for a team scope.
    pub async fn get_notable_contributions_for_team(
        &self,
        team_id: Uuid,
        include_descendants: bool,
        since: OffsetDateTime,
        limit: i32,
    ) -> Result<Vec<NotableContributionRow>, Error> {
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
                WHERE $3
            ),
            scored AS (
                SELECT
                    c.id AS contribution_id,
                    COALESCE(c.title, '') AS title,
                    COALESCE(c.url, '') AS url,
                    COALESCE(p.name, '') AS person_name,
                    c.platform::text AS platform,
                    c.contribution_type::text AS contribution_type,
                    e.enrichment_type,
                    e.value,
                    COALESCE(e.confidence, 0.0)::double precision AS confidence,
                    CASE
                        WHEN e.enrichment_type = 'review_depth'
                            AND (e.value->>'score')::int = 5 THEN 100
                        WHEN e.enrichment_type = 'significance'
                            AND e.value->>'significance' = 'significant' THEN 90
                        WHEN e.enrichment_type = 'review_depth'
                            AND (e.value->>'score')::int = 4 THEN 80
                        WHEN e.enrichment_type = 'significance'
                            AND e.value->>'significance' = 'notable' THEN 70
                        ELSE 0
                    END AS signal_score
                FROM reasoning.enrichments e
                JOIN activity.contributions c ON c.id = e.contribution_id
                LEFT JOIN org.people p ON p.id = c.person_id
                JOIN org.team_memberships tm ON tm.person_id = c.person_id
                JOIN team_tree tt ON tm.team_id = tt.id
                WHERE c.created_at >= $2
                  AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
                  AND e.enrichment_type IN ('review_depth', 'significance')
            )
            SELECT
                contribution_id AS "contribution_id!: Uuid",
                title AS "title!: String",
                url AS "url!: String",
                person_name AS "person_name!: String",
                platform AS "platform!: String",
                contribution_type AS "contribution_type!: String",
                enrichment_type AS "enrichment_type!: String",
                CASE
                    WHEN enrichment_type = 'review_depth'
                        THEN 'Score ' || (value->>'score') || ' — ' || COALESCE(value->>'rationale', '')
                    WHEN enrichment_type = 'significance'
                        THEN COALESCE(value->>'significance', '') || ' — ' || COALESCE(value->>'rationale', '')
                    ELSE ''
                END AS "value_summary!: String",
                COALESCE(value->>'rationale', '') AS "rationale!: String",
                confidence AS "confidence!: f64"
            FROM scored
            WHERE signal_score > 0
            ORDER BY signal_score DESC, confidence DESC
            LIMIT $4
            "#,
            team_id,
            since,
            include_descendants,
            i64::from(limit),
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| NotableContributionRow {
                contribution_id: r.contribution_id,
                title: r.title,
                url: r.url,
                person_name: r.person_name,
                platform: r.platform,
                contribution_type: r.contribution_type,
                enrichment_type: r.enrichment_type,
                value_summary: r.value_summary,
                rationale: r.rationale,
                confidence: r.confidence,
            })
            .collect())
    }

    /// Notable contributions for an individual.
    pub async fn get_notable_contributions_for_person(
        &self,
        person_id: Uuid,
        since: OffsetDateTime,
        limit: i32,
    ) -> Result<Vec<NotableContributionRow>, Error> {
        let rows = sqlx::query!(
            r#"
            WITH scored AS (
                SELECT
                    c.id AS contribution_id,
                    COALESCE(c.title, '') AS title,
                    COALESCE(c.url, '') AS url,
                    COALESCE(p.name, '') AS person_name,
                    c.platform::text AS platform,
                    c.contribution_type::text AS contribution_type,
                    e.enrichment_type,
                    e.value,
                    COALESCE(e.confidence, 0.0)::double precision AS confidence,
                    CASE
                        WHEN e.enrichment_type = 'review_depth'
                            AND (e.value->>'score')::int = 5 THEN 100
                        WHEN e.enrichment_type = 'significance'
                            AND e.value->>'significance' = 'significant' THEN 90
                        WHEN e.enrichment_type = 'review_depth'
                            AND (e.value->>'score')::int = 4 THEN 80
                        WHEN e.enrichment_type = 'significance'
                            AND e.value->>'significance' = 'notable' THEN 70
                        ELSE 0
                    END AS signal_score
                FROM reasoning.enrichments e
                JOIN activity.contributions c ON c.id = e.contribution_id
                LEFT JOIN org.people p ON p.id = c.person_id
                WHERE c.person_id = $1
                  AND c.created_at >= $2
                  AND e.enrichment_type IN ('review_depth', 'significance')
            )
            SELECT
                contribution_id AS "contribution_id!: Uuid",
                title AS "title!: String",
                url AS "url!: String",
                person_name AS "person_name!: String",
                platform AS "platform!: String",
                contribution_type AS "contribution_type!: String",
                enrichment_type AS "enrichment_type!: String",
                CASE
                    WHEN enrichment_type = 'review_depth'
                        THEN 'Score ' || (value->>'score') || ' — ' || COALESCE(value->>'rationale', '')
                    WHEN enrichment_type = 'significance'
                        THEN COALESCE(value->>'significance', '') || ' — ' || COALESCE(value->>'rationale', '')
                    ELSE ''
                END AS "value_summary!: String",
                COALESCE(value->>'rationale', '') AS "rationale!: String",
                confidence AS "confidence!: f64"
            FROM scored
            WHERE signal_score > 0
            ORDER BY signal_score DESC, confidence DESC
            LIMIT $3
            "#,
            person_id,
            since,
            i64::from(limit),
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| NotableContributionRow {
                contribution_id: r.contribution_id,
                title: r.title,
                url: r.url,
                person_name: r.person_name,
                platform: r.platform,
                contribution_type: r.contribution_type,
                enrichment_type: r.enrichment_type,
                value_summary: r.value_summary,
                rationale: r.rationale,
                confidence: r.confidence,
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // Enrichment coverage
    // -----------------------------------------------------------------------

    /// Coverage stats per enrichment type for a team scope.
    pub async fn get_coverage_for_team(
        &self,
        team_id: Uuid,
        include_descendants: bool,
        since: OffsetDateTime,
    ) -> Result<(i32, i32, Vec<TypeCoverageRow>), Error> {
        // Total contributions in scope
        let total = sqlx::query_scalar!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
                WHERE $3
            )
            SELECT COUNT(DISTINCT c.id)::int AS "count!: i32"
            FROM activity.contributions c
            JOIN org.team_memberships tm ON tm.person_id = c.person_id
            JOIN team_tree tt ON tm.team_id = tt.id
            WHERE c.created_at >= $2
              AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            "#,
            team_id,
            since,
            include_descendants,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        // Per-type coverage
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
                WHERE $3
            ),
            team_contributions AS (
                SELECT DISTINCT c.id, c.contribution_type
                FROM activity.contributions c
                JOIN org.team_memberships tm ON tm.person_id = c.person_id
                JOIN team_tree tt ON tm.team_id = tt.id
                WHERE c.created_at >= $2
                  AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            ),
            type_eligible AS (
                SELECT
                    et.enrichment_type,
                    COUNT(*)::int AS eligible
                FROM team_contributions tc
                CROSS JOIN (VALUES ('review_depth'), ('sentiment'), ('significance'), ('topic')) AS et(enrichment_type)
                WHERE (et.enrichment_type IN ('review_depth', 'sentiment') AND tc.contribution_type = 'pr_review')
                   OR (et.enrichment_type = 'significance' AND tc.contribution_type = 'pull_request')
                   OR (et.enrichment_type = 'topic' AND tc.contribution_type = 'discourse_topic')
                GROUP BY et.enrichment_type
            ),
            type_enriched AS (
                SELECT
                    e.enrichment_type,
                    COUNT(*)::int AS enriched
                FROM reasoning.enrichments e
                JOIN team_contributions tc ON tc.id = e.contribution_id
                GROUP BY e.enrichment_type
            )
            SELECT
                te.enrichment_type AS "enrichment_type!: String",
                te.eligible AS "eligible!: i32",
                COALESCE(ten.enriched, 0) AS "enriched!: i32"
            FROM type_eligible te
            LEFT JOIN type_enriched ten ON ten.enrichment_type = te.enrichment_type
            ORDER BY te.enrichment_type
            "#,
            team_id,
            since,
            include_descendants,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let enriched_total: i32 = rows.iter().map(|r| r.enriched).sum();
        let by_type = rows
            .into_iter()
            .map(|r| TypeCoverageRow {
                enrichment_type: r.enrichment_type,
                eligible: r.eligible,
                enriched: r.enriched,
            })
            .collect();

        Ok((total, enriched_total, by_type))
    }

    /// Coverage stats for an individual.
    pub async fn get_coverage_for_person(
        &self,
        person_id: Uuid,
        since: OffsetDateTime,
    ) -> Result<(i32, i32, Vec<TypeCoverageRow>), Error> {
        let total = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*)::int AS "count!: i32"
            FROM activity.contributions c
            WHERE c.person_id = $1
              AND c.created_at >= $2
            "#,
            person_id,
            since,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        let rows = sqlx::query!(
            r#"
            WITH person_contributions AS (
                SELECT id, contribution_type
                FROM activity.contributions
                WHERE person_id = $1 AND created_at >= $2
            ),
            type_eligible AS (
                SELECT
                    et.enrichment_type,
                    COUNT(*)::int AS eligible
                FROM person_contributions pc
                CROSS JOIN (VALUES ('review_depth'), ('sentiment'), ('significance'), ('topic')) AS et(enrichment_type)
                WHERE (et.enrichment_type IN ('review_depth', 'sentiment') AND pc.contribution_type = 'pr_review')
                   OR (et.enrichment_type = 'significance' AND pc.contribution_type = 'pull_request')
                   OR (et.enrichment_type = 'topic' AND pc.contribution_type = 'discourse_topic')
                GROUP BY et.enrichment_type
            ),
            type_enriched AS (
                SELECT
                    e.enrichment_type,
                    COUNT(*)::int AS enriched
                FROM reasoning.enrichments e
                JOIN person_contributions pc ON pc.id = e.contribution_id
                GROUP BY e.enrichment_type
            )
            SELECT
                te.enrichment_type AS "enrichment_type!: String",
                te.eligible AS "eligible!: i32",
                COALESCE(ten.enriched, 0) AS "enriched!: i32"
            FROM type_eligible te
            LEFT JOIN type_enriched ten ON ten.enrichment_type = te.enrichment_type
            ORDER BY te.enrichment_type
            "#,
            person_id,
            since,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        let enriched_total: i32 = rows.iter().map(|r| r.enriched).sum();
        let by_type = rows
            .into_iter()
            .map(|r| TypeCoverageRow {
                enrichment_type: r.enrichment_type,
                eligible: r.eligible,
                enriched: r.enriched,
            })
            .collect();

        Ok((total, enriched_total, by_type))
    }

    // -----------------------------------------------------------------------
    // Depth × significance cross-reference
    // -----------------------------------------------------------------------

    /// Average review depth grouped by PR significance classification.
    pub async fn get_depth_by_significance_for_team(
        &self,
        team_id: Uuid,
        include_descendants: bool,
        since: OffsetDateTime,
    ) -> Result<DepthBySignificanceRow, Error> {
        let row = sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $1
                UNION ALL
                SELECT t.id FROM org.teams t
                JOIN team_tree tt ON t.parent_team_id = tt.id
                WHERE $3
            ),
            -- PRs with significance enrichments
            sig_prs AS (
                SELECT
                    c.platform,
                    c.platform_id,
                    e.value->>'significance' AS sig_label
                FROM reasoning.enrichments e
                JOIN activity.contributions c ON c.id = e.contribution_id
                JOIN org.team_memberships tm ON tm.person_id = c.person_id
                JOIN team_tree tt ON tm.team_id = tt.id
                WHERE e.enrichment_type = 'significance'
                  AND c.created_at >= $2
                  AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            ),
            -- Reviews of those PRs, with depth scores
            review_depths AS (
                SELECT
                    sp.sig_label,
                    (e.value->>'score')::double precision AS depth_score
                FROM sig_prs sp
                JOIN activity.contributions review
                    ON review.platform = sp.platform
                    AND review.metrics->>'pr_platform_id' = sp.platform_id
                    AND review.contribution_type = 'pr_review'
                JOIN reasoning.enrichments e
                    ON e.contribution_id = review.id
                    AND e.enrichment_type = 'review_depth'
            )
            SELECT
                COALESCE(AVG(depth_score) FILTER (WHERE sig_label = 'significant'), 0.0) AS "avg_significant!: f64",
                COALESCE(AVG(depth_score) FILTER (WHERE sig_label = 'notable'), 0.0) AS "avg_notable!: f64",
                COALESCE(AVG(depth_score) FILTER (WHERE sig_label = 'routine'), 0.0) AS "avg_routine!: f64",
                COUNT(*) FILTER (WHERE sig_label = 'significant')::int AS "count_significant!: i32",
                COUNT(*) FILTER (WHERE sig_label = 'notable')::int AS "count_notable!: i32",
                COUNT(*) FILTER (WHERE sig_label = 'routine')::int AS "count_routine!: i32"
            FROM review_depths
            "#,
            team_id,
            since,
            include_descendants,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(DepthBySignificanceRow {
            avg_depth_significant: row.avg_significant,
            avg_depth_notable: row.avg_notable,
            avg_depth_routine: row.avg_routine,
            significant_review_count: row.count_significant,
            notable_review_count: row.count_notable,
            routine_review_count: row.count_routine,
        })
    }

    // -----------------------------------------------------------------------
    // Snapshot upsert
    // -----------------------------------------------------------------------

    /// Upsert an insight snapshot for a team/period.
    ///
    /// Uses `ON CONFLICT` on the `(team_id, period_start, period_type)` unique
    /// constraint, matching the metrics snapshot pattern.
    pub async fn upsert_snapshot(&self, p: &UpsertSnapshotParams) -> Result<Uuid, Error> {
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO reasoning.insight_snapshots (
                team_id, period_start, period_end, period_type,
                avg_review_depth, review_count, rubber_stamp_pct, deep_review_pct,
                depth_distribution,
                constructive_count, neutral_count, critical_count, hostile_count,
                significant_count, notable_count, routine_count,
                avg_depth_on_significant, avg_depth_on_notable, avg_depth_on_routine,
                enrichment_coverage, raw_insights,
                computed_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8,
                $9,
                $10, $11, $12, $13,
                $14, $15, $16,
                $17, $18, $19,
                $20, $21,
                now()
            )
            ON CONFLICT (team_id, period_start, period_type) DO UPDATE SET
                period_end = EXCLUDED.period_end,
                avg_review_depth = EXCLUDED.avg_review_depth,
                review_count = EXCLUDED.review_count,
                rubber_stamp_pct = EXCLUDED.rubber_stamp_pct,
                deep_review_pct = EXCLUDED.deep_review_pct,
                depth_distribution = EXCLUDED.depth_distribution,
                constructive_count = EXCLUDED.constructive_count,
                neutral_count = EXCLUDED.neutral_count,
                critical_count = EXCLUDED.critical_count,
                hostile_count = EXCLUDED.hostile_count,
                significant_count = EXCLUDED.significant_count,
                notable_count = EXCLUDED.notable_count,
                routine_count = EXCLUDED.routine_count,
                avg_depth_on_significant = EXCLUDED.avg_depth_on_significant,
                avg_depth_on_notable = EXCLUDED.avg_depth_on_notable,
                avg_depth_on_routine = EXCLUDED.avg_depth_on_routine,
                enrichment_coverage = EXCLUDED.enrichment_coverage,
                raw_insights = EXCLUDED.raw_insights,
                computed_at = now()
            RETURNING id
            "#,
            p.team_id,
            p.period_start,
            p.period_end,
            &p.period_type,
            p.avg_review_depth,
            p.review_count,
            p.rubber_stamp_pct,
            p.deep_review_pct,
            &p.depth_distribution,
            p.constructive_count,
            p.neutral_count,
            p.critical_count,
            p.hostile_count,
            p.significant_count,
            p.notable_count,
            p.routine_count,
            p.avg_depth_on_significant,
            p.avg_depth_on_notable,
            p.avg_depth_on_routine,
            &p.enrichment_coverage,
            &p.raw_insights,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(id)
    }

    /// Compute enrichment-based peer percentiles for a person relative to same-level peers.
    ///
    /// Returns review depth and rubber-stamp rate percentiles among peers
    /// at the same level who have at least 5 review depth enrichments in the period.
    pub async fn compute_enrichment_peer_percentiles(
        &self,
        person_id: Uuid,
        level: &str,
        since: OffsetDateTime,
    ) -> Result<Vec<EnrichmentPeerPercentile>, Error> {
        let rows = sqlx::query!(
            r#"
            WITH peer_review_stats AS (
                SELECT
                    c.person_id,
                    AVG((e.value->>'score')::double precision) AS avg_depth,
                    COUNT(*)::int AS review_count,
                    (COUNT(*) FILTER (WHERE (e.value->>'score')::int = 1)::double precision
                     / NULLIF(COUNT(*), 0)::double precision * 100) AS rubber_stamp_pct
                FROM reasoning.enrichments e
                JOIN activity.contributions c ON c.id = e.contribution_id
                JOIN org.people p ON p.id = c.person_id
                WHERE e.enrichment_type = 'review_depth'
                  AND c.created_at >= $1
                  AND p.level = $2
                  AND c.person_id IS NOT NULL
                GROUP BY c.person_id
                HAVING COUNT(*) >= 5
            ),
            person_stats AS (
                SELECT avg_depth, rubber_stamp_pct
                FROM peer_review_stats
                WHERE person_id = $3
            ),
            depth_rank AS (
                SELECT
                    COUNT(*)::int AS peer_count,
                    COUNT(*) FILTER (WHERE avg_depth <= (SELECT avg_depth FROM person_stats))::int AS depth_rank,
                    COUNT(*) FILTER (WHERE rubber_stamp_pct >= (SELECT rubber_stamp_pct FROM person_stats))::int AS rubber_stamp_rank
                FROM peer_review_stats
            )
            SELECT
                ps.avg_depth AS "avg_depth!",
                ps.rubber_stamp_pct AS "rubber_stamp_pct!",
                dr.peer_count AS "peer_count!",
                dr.depth_rank AS "depth_rank!",
                dr.rubber_stamp_rank AS "rubber_stamp_rank!"
            FROM person_stats ps, depth_rank dr
            "#,
            since,
            level,
            person_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        let Some(row) = rows else {
            return Ok(vec![]);
        };

        let peer_count = row.peer_count;
        if peer_count == 0 {
            return Ok(vec![]);
        }

        let depth_percentile = f64::from(row.depth_rank) / f64::from(peer_count);
        let rubber_stamp_percentile = f64::from(row.rubber_stamp_rank) / f64::from(peer_count);

        Ok(vec![
            EnrichmentPeerPercentile {
                metric_name: "review_depth".to_string(),
                value: row.avg_depth,
                percentile: depth_percentile,
                peer_count,
            },
            EnrichmentPeerPercentile {
                metric_name: "rubber_stamp_rate".to_string(),
                value: row.rubber_stamp_pct,
                percentile: rubber_stamp_percentile,
                peer_count,
            },
        ])
    }

    /// Fetch the most recent snapshot for a team before the given period start
    /// with the same period type. Returns `None` if no prior snapshot exists.
    pub async fn get_previous_snapshot(
        &self,
        team_id: Uuid,
        period_start: Date,
        period_type: &str,
    ) -> Result<Option<SnapshotRow>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT
                avg_review_depth AS "avg_review_depth: f32",
                review_count AS "review_count!",
                rubber_stamp_pct AS "rubber_stamp_pct: f32",
                deep_review_pct AS "deep_review_pct: f32",
                significant_count AS "significant_count!",
                notable_count AS "notable_count!",
                routine_count AS "routine_count!"
            FROM reasoning.insight_snapshots
            WHERE team_id = $1
              AND period_type = $2
              AND period_start < $3
            ORDER BY period_start DESC
            LIMIT 1
            "#,
            team_id,
            period_type,
            period_start,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row.map(|r| SnapshotRow {
            avg_review_depth: r.avg_review_depth,
            review_count: r.review_count,
            rubber_stamp_pct: r.rubber_stamp_pct,
            deep_review_pct: r.deep_review_pct,
            significant_count: r.significant_count,
            notable_count: r.notable_count,
            routine_count: r.routine_count,
        }))
    }
}
