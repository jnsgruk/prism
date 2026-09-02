use crate::Error;
use crate::repo::{PageRequest, PageResponse, SortDir, SortParams};
use uuid::Uuid;

use super::{OrgRepo, PersonRow};

/// Parameters for querying people with pagination, sorting, and filtering.
pub struct ListPeopleParams {
    pub active_only: bool,
    pub search: Option<String>,
    pub team_id: Option<Uuid>,
    pub filter: Option<String>,
    pub page: PageRequest,
    pub sort: Option<SortParams>,
}

/// Validate a requested sort field before passing it as a query parameter.
fn validated_sort_field(field: &str) -> &'static str {
    match field {
        "name" => "name",
        "email" => "email",
        "team_name" => "team_name",
        "active" => "active",
        other => {
            tracing::warn!(
                sort_field = other,
                "unrecognised sort field, falling back to name"
            );
            "name"
        }
    }
}

fn search_pattern(search: Option<String>) -> Option<String> {
    search
        .filter(|search| !search.is_empty())
        .map(|search| format!("%{}%", search.replace('%', "\\%").replace('_', "\\_")))
}

fn people_cursor(person: &PersonRow, sort_field: &str) -> (String, String) {
    let sort_value = match sort_field {
        "email" => person.email.clone().unwrap_or_default(),
        "team_name" => person.team_name.clone().unwrap_or_default(),
        "active" => person.active.to_string(),
        _ => person.name.clone(),
    };
    (sort_value, person.id.to_string())
}

impl OrgRepo {
    /// List people with server-side pagination, sorting, and search.
    pub async fn list_people_paginated(
        &self,
        params: ListPeopleParams,
    ) -> Result<PageResponse<PersonRow>, Error> {
        let sort = params.sort.unwrap_or(SortParams {
            column: "name".to_owned(),
            direction: SortDir::Asc,
        });

        let sort_field = validated_sort_field(&sort.column);
        let descending = sort.direction == SortDir::Desc;
        let search_pattern = search_pattern(params.search);
        let filter = match params.filter.as_deref() {
            Some(filter @ ("unassigned" | "inactive")) => Some(filter),
            _ => None,
        };
        let cursor_sort = params
            .page
            .cursor
            .as_ref()
            .map(|cursor| cursor.sort_value.as_str());
        let cursor_id = params.page.cursor.as_ref().map(|cursor| cursor.id.as_str());
        let limit = params.page.limit();

        let (total_count, rows) = tokio::try_join!(
            async {
                sqlx::query_scalar!(
                    r#"
                    SELECT COUNT(*)::bigint AS "count!"
                    FROM org.people p
                    LEFT JOIN org.team_memberships tm ON tm.person_id = p.id
                        AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
                    LEFT JOIN org.teams t ON t.id = tm.team_id
                    WHERE ($1::bool = false OR p.active = true)
                      AND ($2::text IS NULL
                           OR $2 = 'all'
                           OR ($2 = 'unassigned' AND p.active = true AND tm.team_id IS NULL)
                           OR ($2 = 'inactive' AND p.active = false))
                      AND ($3::text IS NULL OR p.name ILIKE $3
                           OR COALESCE(p.email, '') ILIKE $3
                           OR COALESCE(t.name, '') ILIKE $3)
                      AND ($4::uuid IS NULL OR tm.team_id IN (
                          WITH RECURSIVE team_tree AS (
                              SELECT id FROM org.teams WHERE id = $4
                              UNION ALL
                              SELECT child.id FROM org.teams child
                              INNER JOIN team_tree parent ON child.parent_team_id = parent.id
                          )
                          SELECT id FROM team_tree
                      ))
                    "#,
                    params.active_only,
                    filter,
                    search_pattern,
                    params.team_id,
                )
                .fetch_one(&self.pool)
                .await
                .map_err(Error::from)
            },
            async {
                sqlx::query!(
                    r#"
                    SELECT p.id, p.name, p.email, p.level, p.active,
                           tm.team_id AS "team_id?", t.name AS "team_name?"
                    FROM org.people p
                    LEFT JOIN org.team_memberships tm ON tm.person_id = p.id
                        AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
                    LEFT JOIN org.teams t ON t.id = tm.team_id
                    WHERE ($1::bool = false OR p.active = true)
                      AND ($2::text IS NULL
                           OR $2 = 'all'
                           OR ($2 = 'unassigned' AND p.active = true AND tm.team_id IS NULL)
                           OR ($2 = 'inactive' AND p.active = false))
                      AND ($3::text IS NULL OR p.name ILIKE $3
                           OR COALESCE(p.email, '') ILIKE $3
                           OR COALESCE(t.name, '') ILIKE $3)
                      AND ($4::uuid IS NULL OR tm.team_id IN (
                          WITH RECURSIVE team_tree AS (
                              SELECT id FROM org.teams WHERE id = $4
                              UNION ALL
                              SELECT child.id FROM org.teams child
                              INNER JOIN team_tree parent ON child.parent_team_id = parent.id
                          )
                          SELECT id FROM team_tree
                      ))
                      AND ($5::text IS NULL OR (
                          $8::bool = false AND
                          (CASE $6::text
                              WHEN 'email' THEN COALESCE(p.email, '')
                              WHEN 'team_name' THEN COALESCE(t.name, '')
                              WHEN 'active' THEN p.active::text
                              ELSE p.name
                           END, p.id::text) > ($5, $7::text)
                      ) OR (
                          $8::bool = true AND
                          (CASE $6::text
                              WHEN 'email' THEN COALESCE(p.email, '')
                              WHEN 'team_name' THEN COALESCE(t.name, '')
                              WHEN 'active' THEN p.active::text
                              ELSE p.name
                           END, p.id::text) < ($5, $7::text)
                      ))
                    ORDER BY
                      CASE WHEN $8::bool = false THEN CASE $6::text
                          WHEN 'email' THEN COALESCE(p.email, '')
                          WHEN 'team_name' THEN COALESCE(t.name, '')
                          WHEN 'active' THEN p.active::text
                          ELSE p.name
                      END END ASC,
                      CASE WHEN $8::bool = true THEN CASE $6::text
                          WHEN 'email' THEN COALESCE(p.email, '')
                          WHEN 'team_name' THEN COALESCE(t.name, '')
                          WHEN 'active' THEN p.active::text
                          ELSE p.name
                      END END DESC,
                      CASE WHEN $8::bool = false THEN p.id END ASC,
                      CASE WHEN $8::bool = true THEN p.id END DESC
                    LIMIT $9
                    "#,
                    params.active_only,
                    filter,
                    search_pattern,
                    params.team_id,
                    cursor_sort,
                    sort_field,
                    cursor_id,
                    descending,
                    limit,
                )
                .fetch_all(&self.pool)
                .await
                .map_err(Error::from)
            },
        )?;

        let sort_column = sort.column;
        let items: Vec<PersonRow> = rows
            .into_iter()
            .map(|row| PersonRow {
                id: row.id,
                name: row.name,
                email: row.email,
                level: row.level,
                active: row.active,
                team_id: row.team_id,
                team_name: row.team_name,
            })
            .collect();

        Ok(PageResponse::from_items(
            items,
            params.page.page_size,
            total_count,
            |person| people_cursor(person, &sort_column),
        ))
    }

    /// List people ordered by name, optionally filtering by active status.
    /// Kept for backward compatibility with callers that don't need pagination.
    pub async fn list_people(&self, active_only: bool) -> Result<Vec<PersonRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT p.id, p.name, p.email, p.level, p.active,
                   tm.team_id AS "team_id?", t.name AS "team_name?"
            FROM org.people p
            LEFT JOIN org.team_memberships tm ON tm.person_id = p.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.teams t ON t.id = tm.team_id
            WHERE ($1::bool = false OR p.active = true)
            ORDER BY p.name
            "#,
            active_only,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|p| PersonRow {
                id: p.id,
                name: p.name,
                email: p.email,
                level: p.level,
                active: p.active,
                team_id: p.team_id,
                team_name: p.team_name,
            })
            .collect())
    }

    /// Get a single person with their current team info.
    pub async fn get_person(&self, id: Uuid) -> Result<Option<PersonRow>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT p.id, p.name, p.email, p.level, p.active,
                   tm.team_id AS "team_id?", t.name AS "team_name?"
            FROM org.people p
            LEFT JOIN org.team_memberships tm ON tm.person_id = p.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.teams t ON t.id = tm.team_id
            WHERE p.id = $1
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row.map(|p| PersonRow {
            id: p.id,
            name: p.name,
            email: p.email,
            level: p.level,
            active: p.active,
            team_id: p.team_id,
            team_name: p.team_name,
        }))
    }

    /// Update a person's fields (COALESCE pattern — only non-NULL values change).
    pub async fn update_person(
        &self,
        id: Uuid,
        name: Option<&str>,
        email: Option<&str>,
        level: Option<&str>,
    ) -> Result<PersonRow, Error> {
        sqlx::query!(
            r#"
            UPDATE org.people
            SET name = COALESCE($2, name),
                email = COALESCE($3, email),
                level = COALESCE($4, level),
                updated_at = now()
            WHERE id = $1
            "#,
            id,
            name,
            email,
            level,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        self.get_person(id)
            .await?
            .ok_or_else(|| Error::Internal("person not found after update".to_owned()))
    }

    /// Deactivate a person: set `active = false` and end all active memberships.
    pub async fn deactivate_person(&self, id: Uuid) -> Result<(), Error> {
        let mut tx = self.pool.begin().await.map_err(Error::from)?;

        sqlx::query!(
            "UPDATE org.people SET active = false, updated_at = now() WHERE id = $1",
            id,
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;

        sqlx::query!(
            r#"
            UPDATE org.team_memberships
            SET end_date = CURRENT_DATE
            WHERE person_id = $1 AND (end_date IS NULL OR end_date > CURRENT_DATE)
            "#,
            id,
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::from)?;

        tx.commit().await.map_err(Error::from)?;

        Ok(())
    }

    /// Reactivate a person: set `active = true`. Does not restore memberships.
    pub async fn reactivate_person(&self, id: Uuid) -> Result<(), Error> {
        sqlx::query!(
            "UPDATE org.people SET active = true, updated_at = now() WHERE id = $1",
            id,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }
}
