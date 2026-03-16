use crate::Error;
use crate::repo::{PageRequest, PageResponse, SortDir, SortParams};
use sqlx::FromRow;
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

/// Row type for runtime SQL query.
#[derive(FromRow)]
struct PeopleQueryRow {
    id: Uuid,
    name: String,
    email: Option<String>,
    level: Option<String>,
    active: bool,
    team_id: Option<Uuid>,
    team_name: Option<String>,
}

impl From<PeopleQueryRow> for PersonRow {
    fn from(r: PeopleQueryRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            email: r.email,
            level: r.level,
            active: r.active,
            team_id: r.team_id,
            team_name: r.team_name,
        }
    }
}

/// Map a validated sort field name to its SQL expression.
fn sort_field_to_sql(field: &str) -> &'static str {
    match field {
        "email" => "COALESCE(p.email, '')",
        "team_name" => "COALESCE(t.name, '')",
        "active" => "p.active",
        _ => "p.name",
    }
}

impl OrgRepo {
    /// List people with server-side pagination, sorting, and search.
    ///
    /// Uses runtime SQL (`sqlx::query_as`) because dynamic ORDER BY and
    /// keyset cursor clauses cannot be expressed in compile-time `sqlx::query!`.
    /// Sort fields are validated against `PEOPLE_SORT_FIELDS` to prevent injection.
    pub async fn list_people_paginated(
        &self,
        params: ListPeopleParams,
    ) -> Result<PageResponse<PersonRow>, Error> {
        let sort = params.sort.unwrap_or(SortParams {
            column: "name".to_owned(),
            direction: SortDir::Asc,
        });

        let sort_col = sort_field_to_sql(&sort.column);
        let sort_dir = if sort.direction == SortDir::Desc {
            "DESC"
        } else {
            "ASC"
        };
        let cursor_op = if sort.direction == SortDir::Desc {
            "<"
        } else {
            ">"
        };

        let base_from = r"FROM org.people p
            LEFT JOIN org.team_memberships tm ON tm.person_id = p.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.teams t ON t.id = tm.team_id";

        // Build dynamic WHERE + bind values.
        let mut conditions: Vec<String> = Vec::new();
        let mut binds: Vec<String> = Vec::new();
        let mut idx = 0usize;

        if params.active_only {
            conditions.push("p.active = true".to_owned());
        }

        if let Some(ref f) = params.filter {
            match f.as_str() {
                "unassigned" => {
                    conditions.push("p.active = true".to_owned());
                    conditions.push("tm.team_id IS NULL".to_owned());
                }
                "inactive" => {
                    conditions.push("p.active = false".to_owned());
                }
                _ => {}
            }
        }

        if let Some(ref search) = params.search
            && !search.is_empty()
        {
            idx += 1;
            let pattern = format!("%{}%", search.replace('%', "\\%").replace('_', "\\_"));
            binds.push(pattern);
            conditions.push(format!(
                    "(p.name ILIKE ${idx} OR COALESCE(p.email, '') ILIKE ${idx} OR COALESCE(t.name, '') ILIKE ${idx})"
                ));
        }

        if let Some(tid) = params.team_id {
            idx += 1;
            binds.push(tid.to_string());
            conditions.push(format!("tm.team_id = ${idx}::uuid"));
        }

        if let Some(ref cursor) = params.page.cursor {
            idx += 1;
            binds.push(cursor.sort_value.clone());
            let sort_param = idx;
            idx += 1;
            binds.push(cursor.id.clone());
            let id_param = idx;
            conditions.push(format!(
                "({sort_col}, p.id::text) {cursor_op} (${sort_param}, ${id_param})"
            ));
        }

        let where_sql = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Count (uses same conditions except cursor — cursor binds are at end).
        let count_bind_len = if params.page.cursor.is_some() {
            binds.len() - 2
        } else {
            binds.len()
        };
        let count_conditions: Vec<&str> = conditions
            .iter()
            .take(if params.page.cursor.is_some() {
                conditions.len() - 1
            } else {
                conditions.len()
            })
            .map(String::as_str)
            .collect();
        let count_where = if count_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", count_conditions.join(" AND "))
        };
        let count_sql = format!("SELECT COUNT(*)::bigint {base_from} {count_where}");

        // Data query with optional LIMIT.
        let limit_sql = if let Some(limit) = params.page.limit() {
            idx += 1;
            binds.push(limit.to_string());
            format!("LIMIT ${idx}::bigint")
        } else {
            String::new()
        };

        let data_sql = format!(
            "SELECT p.id, p.name, p.email, p.level, p.active, tm.team_id, t.name AS team_name \
             {base_from} {where_sql} ORDER BY {sort_col} {sort_dir}, p.id {sort_dir} {limit_sql}"
        );

        // Execute count + data in parallel.
        let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
        for val in binds.get(..count_bind_len).unwrap_or(&binds) {
            cq = cq.bind(val);
        }
        let mut dq = sqlx::query_as::<_, PeopleQueryRow>(&data_sql);
        for val in &binds {
            dq = dq.bind(val);
        }
        let (total_count, rows): (i64, Vec<PeopleQueryRow>) = tokio::try_join!(
            async { cq.fetch_one(&self.pool).await.map_err(Error::from) },
            async { dq.fetch_all(&self.pool).await.map_err(Error::from) },
        )?;

        let sort_col_name = sort.column.clone();
        let items: Vec<PersonRow> = rows.into_iter().map(Into::into).collect();

        Ok(PageResponse::from_items(
            items,
            params.page.page_size,
            total_count,
            |p| {
                let sv = match sort_col_name.as_str() {
                    "email" => p.email.clone().unwrap_or_default(),
                    "team_name" => p.team_name.clone().unwrap_or_default(),
                    "active" => p.active.to_string(),
                    _ => p.name.clone(),
                };
                (sv, p.id.to_string())
            },
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
