use super::*;
use crate::models::{ContributionState, ContributionType, Platform};

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
    pub state: Option<&'a str>,
    pub search: Option<&'a str>,
}

/// A detailed contribution row for drill-down display.
pub struct ContributionDetailRow {
    pub id: Uuid,
    pub person_name: String,
    pub platform: Platform,
    pub contribution_type: ContributionType,
    pub platform_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub state: Option<ContributionState>,
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
        let escaped_search = params.search.map(super::super::escape_like);
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
              AND ($4::text IS NULL OR c.contribution_type LIKE $4)
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
}
