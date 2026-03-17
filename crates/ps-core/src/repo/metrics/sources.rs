use super::*;
use crate::models::{ContributionState, ContributionType, Platform};
use std::collections::HashMap;

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
    pub(super) async fn get_snapshot_source_platforms(
        &self,
        snapshot_id: Uuid,
    ) -> Result<Vec<String>, Error> {
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
    pub(super) async fn get_bulk_snapshot_source_platforms(
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
}
