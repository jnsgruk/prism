use super::*;

impl InsightsRepo {
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
}
