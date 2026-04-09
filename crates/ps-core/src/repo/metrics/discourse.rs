use super::*;

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
              AND ($4::text IS NULL OR c.platform ILIKE $4)
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
              AND ($4::text IS NULL OR c.platform ILIKE $4)
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
              AND ($4::text IS NULL OR c.platform ILIKE $4)
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
