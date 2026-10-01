//! Replace insight provenance alongside every successful period recomputation.

use time::Date;
use uuid::Uuid;

use super::InsightsRepo;
use crate::Error;

impl InsightsRepo {
    pub async fn replace_snapshot_sources(
        &self,
        snapshot_id: Uuid,
        team_id: Uuid,
        period_start: Date,
        period_end: Date,
    ) -> Result<(), Error> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query!(
            "DELETE FROM reasoning.insight_snapshot_sources WHERE snapshot_id = $1",
            snapshot_id,
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query!(
            r#"
            WITH RECURSIVE team_tree AS (
                SELECT id FROM org.teams WHERE id = $2
                UNION ALL
                SELECT team.id FROM org.teams team
                JOIN team_tree parent ON team.parent_team_id = parent.id
            ),
            contributions AS (
                SELECT c.id, c.platform, c.platform_id
                FROM activity.contributions c
                WHERE c.created_at >= $3::date::timestamptz
                    AND c.created_at < ($4::date + INTERVAL '1 day')::timestamptz
                    AND EXISTS (
                        SELECT 1 FROM org.team_memberships member
                        JOIN team_tree team ON team.id = member.team_id
                        WHERE member.person_id = c.person_id
                            AND member.start_date <= $4
                            AND (member.end_date IS NULL OR member.end_date > $4)
                    )
            ),
            sources AS (
                SELECT enrichment.id
                FROM reasoning.enrichments enrichment
                JOIN contributions c ON c.id = enrichment.contribution_id
                UNION
                SELECT depth.id
                FROM contributions pr
                JOIN reasoning.enrichments significance
                    ON significance.contribution_id = pr.id
                    AND significance.enrichment_type = 'significance'
                JOIN activity.contributions review ON review.platform = pr.platform
                    AND COALESCE(review.metadata->>'pr_platform_id', review.metrics->>'pr_platform_id') = pr.platform_id
                    AND review.contribution_type = 'pr_review'
                JOIN reasoning.enrichments depth ON depth.contribution_id = review.id
                    AND depth.enrichment_type = 'review_depth'
                WHERE review.created_at >= $3::date::timestamptz
                    AND review.created_at < ($4::date + INTERVAL '1 day')::timestamptz
            )
            INSERT INTO reasoning.insight_snapshot_sources (snapshot_id, enrichment_id)
            SELECT $1, id FROM sources
            ON CONFLICT DO NOTHING
            "#,
            snapshot_id,
            team_id,
            period_start,
            period_end,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}
