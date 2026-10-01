use super::*;

impl InsightsRepo {
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
}
