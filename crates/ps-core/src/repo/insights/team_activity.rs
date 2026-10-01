use super::*;

impl InsightsRepo {
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
            JOIN (SELECT DISTINCT tm.person_id FROM org.team_memberships tm
                JOIN team_tree tt ON tm.team_id = tt.id
                WHERE (tm.end_date IS NULL OR tm.end_date > COALESCE($4::date, CURRENT_DATE))
                    AND tm.start_date <= COALESCE($4::date, CURRENT_DATE)) members
                ON members.person_id = c.person_id
            WHERE e.enrichment_type = 'significance'
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

        Ok(SignificanceRow {
            significant: row.significant,
            notable: row.notable,
            routine: row.routine,
            avg_confidence: row.avg_confidence,
        })
    }

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
            JOIN (SELECT DISTINCT tm.person_id FROM org.team_memberships tm
                JOIN team_tree tt ON tm.team_id = tt.id
                WHERE (tm.end_date IS NULL OR tm.end_date > COALESCE($4::date, CURRENT_DATE))
                    AND tm.start_date <= COALESCE($4::date, CURRENT_DATE)) members
                ON members.person_id = c.person_id
            WHERE e.enrichment_type = 'topic'
              AND c.created_at >= $2
              AND ($4::date IS NULL OR c.created_at < ($4::date + INTERVAL '1 day')::timestamptz)
            GROUP BY COALESCE(e.value->>'primary_category', 'unknown')
            ORDER BY COUNT(*) DESC
            "#,
            team_id,
            since,
            include_descendants,
            self.snapshot_period_end,
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
            JOIN (SELECT DISTINCT tm.person_id FROM org.team_memberships tm
                JOIN team_tree tt ON tm.team_id = tt.id
                WHERE (tm.end_date IS NULL OR tm.end_date > COALESCE($4::date, CURRENT_DATE))
                    AND tm.start_date <= COALESCE($4::date, CURRENT_DATE)) members
                ON members.person_id = c.person_id
            WHERE c.created_at >= $2
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
                JOIN (SELECT DISTINCT tm.person_id FROM org.team_memberships tm
                    JOIN team_tree tt ON tm.team_id = tt.id
                    WHERE (tm.end_date IS NULL OR tm.end_date > COALESCE($4::date, CURRENT_DATE))
                        AND tm.start_date <= COALESCE($4::date, CURRENT_DATE)) members
                    ON members.person_id = c.person_id
                WHERE c.created_at >= $2
              AND ($4::date IS NULL OR c.created_at < ($4::date + INTERVAL '1 day')::timestamptz)
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
            self.snapshot_period_end,
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
}
