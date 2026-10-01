use super::*;

impl InsightsRepo {
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
            JOIN (SELECT DISTINCT tm.person_id FROM org.team_memberships tm
                JOIN team_tree tt ON tm.team_id = tt.id
                WHERE (tm.end_date IS NULL OR tm.end_date > COALESCE($4::date, CURRENT_DATE))
                    AND tm.start_date <= COALESCE($4::date, CURRENT_DATE)) members
                ON members.person_id = c.person_id
            WHERE e.enrichment_type = 'review_depth'
              AND c.created_at >= $2
              AND ($4::date IS NULL OR c.created_at < ($4::date + INTERVAL '1 day')::timestamptz)
            "#,
            team_id,
            since,
            include_descendants,
            self.snapshot_period_end,
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
            JOIN (SELECT DISTINCT tm.person_id FROM org.team_memberships tm
                JOIN team_tree tt ON tm.team_id = tt.id
                WHERE (tm.end_date IS NULL OR tm.end_date > COALESCE($4::date, CURRENT_DATE))
                    AND tm.start_date <= COALESCE($4::date, CURRENT_DATE)) members
                ON members.person_id = c.person_id
            WHERE e.enrichment_type = 'sentiment'
              AND c.created_at >= $2
              AND ($4::date IS NULL OR c.created_at < ($4::date + INTERVAL '1 day')::timestamptz)
            "#,
            team_id,
            since,
            include_descendants,
            self.snapshot_period_end,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok((row.constructive, row.neutral, row.critical, row.hostile))
    }

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
                JOIN (SELECT DISTINCT tm.person_id FROM org.team_memberships tm
                    JOIN team_tree tt ON tm.team_id = tt.id
                    WHERE (tm.end_date IS NULL OR tm.end_date > COALESCE($4::date, CURRENT_DATE))
                        AND tm.start_date <= COALESCE($4::date, CURRENT_DATE)) members
                    ON members.person_id = c.person_id
                WHERE e.enrichment_type = 'significance'
                  AND c.created_at >= $2
              AND ($4::date IS NULL OR c.created_at < ($4::date + INTERVAL '1 day')::timestamptz)
            ),
            -- Reviews of those PRs, with depth scores
            review_depths AS (
                SELECT
                    sp.sig_label,
                    (e.value->>'score')::double precision AS depth_score
                FROM sig_prs sp
                JOIN activity.contributions review
                    ON review.platform = sp.platform
                    AND COALESCE(review.metadata->>'pr_platform_id', review.metrics->>'pr_platform_id') = sp.platform_id
                    AND review.contribution_type = 'pr_review'
                JOIN reasoning.enrichments e
                    ON e.contribution_id = review.id
                    AND e.enrichment_type = 'review_depth'
                WHERE review.created_at >= $2
                    AND ($4::date IS NULL OR review.created_at < ($4::date + INTERVAL '1 day')::timestamptz)
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
            self.snapshot_period_end,
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
}
