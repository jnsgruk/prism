use std::collections::HashMap;

use crate::Error;
use crate::models::TeamType;
use sqlx::PgPool;
use uuid::Uuid;

/// Repository for the `org` schema: people, teams, platform identities,
/// team memberships, and repositories.
#[derive(Clone)]
pub struct OrgRepo {
    pool: PgPool,
}

/// A team row with active member count.
pub struct TeamWithCount {
    pub id: Uuid,
    pub name: String,
    pub org_name: String,
    pub parent_team_id: Option<Uuid>,
    pub lead_id: Option<Uuid>,
    pub lead_name: Option<String>,
    pub github_team_slug: Option<String>,
    pub team_type: TeamType,
    pub member_count: i32,
}

/// A minimal person row (id, name, email, level).
pub struct PersonRow {
    pub id: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub level: Option<String>,
}

/// A platform identity row.
pub struct IdentityRow {
    pub person_id: Uuid,
    pub platform: String,
    pub platform_username: String,
}

/// Input for a directory import record.
pub struct ImportRecord {
    pub name: String,
    pub email: Option<String>,
    pub level: Option<String>,
    pub directory_id: Option<String>,
    pub team: Option<String>,
    pub team_type: Option<TeamType>,
    pub org: Option<String>,
    pub identities: Vec<ImportIdentity>,
    /// Manager name (from directory HTML --manager field).
    pub manager_name: Option<String>,
    /// Nesting depth in the directory HTML (1 = VP, 2 = director/manager, etc.).
    pub depth: Option<u32>,
    /// Whether this person has direct reports in the directory tree.
    pub has_reports: bool,
}

/// A platform identity within an import record.
pub struct ImportIdentity {
    pub platform: String,
    pub username: String,
}

/// Result of a directory import operation.
pub struct ImportResult {
    pub people_imported: i32,
    pub teams_created: i32,
    pub identities_mapped: i32,
    pub warnings: Vec<String>,
}

impl OrgRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// List teams with active member counts, optionally filtered by parent and/or type.
    pub async fn list_teams(
        &self,
        parent_filter: Option<Uuid>,
        type_filter: Option<TeamType>,
    ) -> Result<Vec<TeamWithCount>, Error> {
        let type_str = type_filter.map(|t: TeamType| t.to_string());
        let rows = sqlx::query!(
            r#"
            SELECT t.id, t.name, t.org_name, t.parent_team_id, t.lead_id,
                   t.github_team_slug,
                   t.team_type AS "team_type: TeamType",
                   p.name AS lead_name,
                   COUNT(tm.id)::int AS "member_count!"
            FROM org.teams t
            LEFT JOIN org.team_memberships tm ON tm.team_id = t.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.people p ON p.id = t.lead_id
            WHERE ($1::uuid IS NULL OR t.parent_team_id = $1)
              AND ($2::text IS NULL OR t.team_type::text = $2)
            GROUP BY t.id, p.name
            ORDER BY t.name
            "#,
            parent_filter,
            type_str,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|t| TeamWithCount {
                id: t.id,
                name: t.name,
                org_name: t.org_name,
                parent_team_id: t.parent_team_id,
                lead_id: t.lead_id,
                lead_name: Some(t.lead_name),
                github_team_slug: t.github_team_slug,
                team_type: t.team_type,
                member_count: t.member_count,
            })
            .collect())
    }

    /// Get a single team with its active member count.
    pub async fn get_team(&self, id: Uuid) -> Result<Option<TeamWithCount>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT t.id, t.name, t.org_name, t.parent_team_id, t.lead_id,
                   t.github_team_slug,
                   t.team_type AS "team_type: TeamType",
                   p.name AS lead_name,
                   COUNT(tm.id)::int AS "member_count!"
            FROM org.teams t
            LEFT JOIN org.team_memberships tm ON tm.team_id = t.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.people p ON p.id = t.lead_id
            WHERE t.id = $1
            GROUP BY t.id, p.name
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(row.map(|t| TeamWithCount {
            id: t.id,
            name: t.name,
            org_name: t.org_name,
            parent_team_id: t.parent_team_id,
            lead_id: t.lead_id,
            lead_name: Some(t.lead_name),
            github_team_slug: t.github_team_slug,
            team_type: t.team_type,
            member_count: t.member_count,
        }))
    }

    /// Get active members of a team.
    pub async fn get_team_members(&self, team_id: Uuid) -> Result<Vec<PersonRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT p.id, p.name, p.email, p.level
            FROM org.people p
            JOIN org.team_memberships tm ON tm.person_id = p.id
            WHERE tm.team_id = $1
              AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            ORDER BY p.name
            "#,
            team_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|p| PersonRow {
                id: p.id,
                name: p.name,
                email: p.email,
                level: p.level,
            })
            .collect())
    }

    /// List all people ordered by name.
    pub async fn list_people(&self) -> Result<Vec<PersonRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, name, email, level
            FROM org.people
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|p| PersonRow {
                id: p.id,
                name: p.name,
                email: p.email,
                level: p.level,
            })
            .collect())
    }

    /// Get platform identities for a set of person IDs.
    pub async fn get_identities_for_people(
        &self,
        person_ids: &[Uuid],
    ) -> Result<Vec<IdentityRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT person_id, platform, platform_username
            FROM org.platform_identities
            WHERE person_id = ANY($1)
            "#,
            person_ids,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|i| IdentityRow {
                person_id: i.person_id,
                platform: i.platform,
                platform_username: i.platform_username,
            })
            .collect())
    }

    /// Batch-resolve platform usernames to person IDs.
    pub async fn batch_resolve_person_ids(
        &self,
        platform: &str,
        usernames: &[String],
    ) -> Result<HashMap<String, Uuid>, Error> {
        if usernames.is_empty() {
            return Ok(HashMap::new());
        }

        let rows = sqlx::query!(
            r#"
            SELECT platform_username, person_id
            FROM org.platform_identities
            WHERE platform = $1
              AND platform_username = ANY($2)
            "#,
            platform,
            usernames,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| (r.platform_username, r.person_id))
            .collect())
    }

    /// Upsert a repository record.
    pub async fn upsert_repository(
        &self,
        id: Uuid,
        github_org: &str,
        github_repo: &str,
        default_branch: Option<&str>,
        primary_language: Option<&str>,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO org.repositories (id, github_org, github_repo, default_branch, primary_language)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (github_org, github_repo)
            DO UPDATE SET
                default_branch = COALESCE(EXCLUDED.default_branch, org.repositories.default_branch),
                primary_language = COALESCE(EXCLUDED.primary_language, org.repositories.primary_language)
            "#,
            id,
            github_org,
            github_repo,
            default_branch,
            primary_language,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Import directory records within a transaction.
    #[allow(clippy::too_many_lines)]
    pub async fn import_records(&self, records: &[ImportRecord]) -> Result<ImportResult, Error> {
        let mut people_imported = 0i32;
        let mut teams_created = 0i32;
        let mut identities_mapped = 0i32;
        let mut warnings = Vec::new();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        for record in records {
            if record.name.is_empty() {
                warnings.push(format!(
                    "skipping record with empty name (directory_id: {:?})",
                    record.directory_id
                ));
                continue;
            }

            let person_id = Uuid::now_v7();

            // Upsert person by directory_id if present, otherwise insert
            let resolved_id = if let Some(dir_id) = &record.directory_id {
                let existing = sqlx::query_scalar!(
                    "SELECT id FROM org.people WHERE directory_id = $1",
                    dir_id,
                )
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| Error::Database(e.to_string()))?;

                if let Some(existing_id) = existing {
                    sqlx::query!(
                        r#"
                        UPDATE org.people
                        SET name = $1, email = $2, level = $3, updated_at = now()
                        WHERE id = $4
                        "#,
                        record.name,
                        record.email,
                        record.level,
                        existing_id,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| Error::Database(e.to_string()))?;

                    existing_id
                } else {
                    sqlx::query!(
                        r#"
                        INSERT INTO org.people (id, name, email, level, directory_id)
                        VALUES ($1, $2, $3, $4, $5)
                        "#,
                        person_id,
                        record.name,
                        record.email,
                        record.level,
                        dir_id,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| Error::Database(e.to_string()))?;

                    people_imported += 1;
                    person_id
                }
            } else {
                sqlx::query!(
                    r#"
                    INSERT INTO org.people (id, name, email, level)
                    VALUES ($1, $2, $3, $4)
                    "#,
                    person_id,
                    record.name,
                    record.email,
                    record.level,
                )
                .execute(&mut *tx)
                .await
                .map_err(|e| Error::Database(e.to_string()))?;

                people_imported += 1;
                person_id
            };

            // Create team if specified and doesn't exist
            if let Some(team_name) = &record.team {
                let org_name = record.org.as_deref().unwrap_or("default");

                let team_id = sqlx::query_scalar!(
                    "SELECT id FROM org.teams WHERE name = $1 AND org_name = $2",
                    team_name,
                    org_name,
                )
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| Error::Database(e.to_string()))?;

                let team_id = if let Some(id) = team_id {
                    id
                } else {
                    let new_id = Uuid::now_v7();
                    let tt = record.team_type.unwrap_or(TeamType::Group);
                    sqlx::query!(
                        r#"
                        INSERT INTO org.teams (id, name, org_name, team_type)
                        VALUES ($1, $2, $3, $4::org.team_type)
                        "#,
                        new_id,
                        team_name,
                        org_name,
                        tt as TeamType,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| Error::Database(e.to_string()))?;

                    teams_created += 1;
                    new_id
                };

                // Create membership if not already active
                let has_membership = sqlx::query_scalar!(
                    r#"
                    SELECT EXISTS(
                        SELECT 1 FROM org.team_memberships
                        WHERE person_id = $1 AND team_id = $2
                          AND (end_date IS NULL OR end_date > CURRENT_DATE)
                    ) AS "exists!"
                    "#,
                    resolved_id,
                    team_id,
                )
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| Error::Database(e.to_string()))?;

                if !has_membership {
                    let membership_id = Uuid::now_v7();
                    sqlx::query!(
                        r#"
                        INSERT INTO org.team_memberships (id, person_id, team_id, start_date)
                        VALUES ($1, $2, $3, CURRENT_DATE)
                        "#,
                        membership_id,
                        resolved_id,
                        team_id,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| Error::Database(e.to_string()))?;
                }
            }

            // Map platform identities
            for identity in &record.identities {
                if identity.platform.is_empty() || identity.username.is_empty() {
                    warnings.push(format!("skipping empty identity for {}", record.name));
                    continue;
                }

                let existing = sqlx::query_scalar!(
                    r#"
                    SELECT id FROM org.platform_identities
                    WHERE platform = $1 AND platform_username = $2
                    "#,
                    identity.platform,
                    identity.username,
                )
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| Error::Database(e.to_string()))?;

                if existing.is_some() {
                    sqlx::query!(
                        r#"
                        UPDATE org.platform_identities
                        SET person_id = $1
                        WHERE platform = $2 AND platform_username = $3
                        "#,
                        resolved_id,
                        identity.platform,
                        identity.username,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| Error::Database(e.to_string()))?;
                } else {
                    let identity_id = Uuid::now_v7();
                    sqlx::query!(
                        r#"
                        INSERT INTO org.platform_identities (id, person_id, platform, platform_username)
                        VALUES ($1, $2, $3, $4)
                        "#,
                        identity_id,
                        resolved_id,
                        identity.platform,
                        identity.username,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| Error::Database(e.to_string()))?;

                    identities_mapped += 1;
                }
            }
        }

        tx.commit()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(ImportResult {
            people_imported,
            teams_created,
            identities_mapped,
            warnings,
        })
    }

    /// Get all teams (flat list) for building a tree in memory.
    pub async fn get_all_teams(&self) -> Result<Vec<TeamWithCount>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT t.id, t.name, t.org_name, t.parent_team_id, t.lead_id,
                   t.github_team_slug,
                   t.team_type AS "team_type: TeamType",
                   p.name AS lead_name,
                   COUNT(tm.id)::int AS "member_count!"
            FROM org.teams t
            LEFT JOIN org.team_memberships tm ON tm.team_id = t.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            LEFT JOIN org.people p ON p.id = t.lead_id
            GROUP BY t.id, p.name
            ORDER BY t.name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|t| TeamWithCount {
                id: t.id,
                name: t.name,
                org_name: t.org_name,
                parent_team_id: t.parent_team_id,
                lead_id: t.lead_id,
                lead_name: Some(t.lead_name),
                github_team_slug: t.github_team_slug,
                team_type: t.team_type,
                member_count: t.member_count,
            })
            .collect())
    }

    /// Create a new team.
    pub async fn create_team(
        &self,
        name: &str,
        org_name: &str,
        team_type: TeamType,
        parent_team_id: Option<Uuid>,
        lead_id: Option<Uuid>,
        github_team_slug: Option<&str>,
    ) -> Result<TeamWithCount, Error> {
        let id = Uuid::now_v7();
        sqlx::query!(
            r#"
            INSERT INTO org.teams (id, name, org_name, team_type, parent_team_id, lead_id, github_team_slug)
            VALUES ($1, $2, $3, $4::org.team_type, $5, $6, $7)
            "#,
            id,
            name,
            org_name,
            team_type as TeamType,
            parent_team_id,
            lead_id,
            github_team_slug,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        self.get_team(id)
            .await?
            .ok_or_else(|| Error::Database("failed to read back created team".to_owned()))
    }

    /// Update an existing team.
    pub async fn update_team(
        &self,
        id: Uuid,
        name: Option<&str>,
        parent_team_id: Option<Uuid>,
        lead_id: Option<Uuid>,
        github_team_slug: Option<&str>,
    ) -> Result<TeamWithCount, Error> {
        sqlx::query!(
            r#"
            UPDATE org.teams
            SET name = COALESCE($2, name),
                parent_team_id = COALESCE($3, parent_team_id),
                lead_id = COALESCE($4, lead_id),
                github_team_slug = COALESCE($5, github_team_slug)
            WHERE id = $1
            "#,
            id,
            name,
            parent_team_id,
            lead_id,
            github_team_slug,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        self.get_team(id)
            .await?
            .ok_or_else(|| Error::Database("team not found after update".to_owned()))
    }

    /// Delete a team. Fails if it has children or active members.
    pub async fn delete_team(&self, id: Uuid) -> Result<(), Error> {
        let has_children = sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM org.teams WHERE parent_team_id = $1) AS "exists!""#,
            id,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        if has_children {
            return Err(Error::Validation(
                "cannot delete team with child teams — remove or reparent children first"
                    .to_owned(),
            ));
        }

        let has_members = sqlx::query_scalar!(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM org.team_memberships
                WHERE team_id = $1 AND (end_date IS NULL OR end_date > CURRENT_DATE)
            ) AS "exists!"
            "#,
            id,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        if has_members {
            return Err(Error::Validation(
                "cannot delete team with active members — reassign members first".to_owned(),
            ));
        }

        sqlx::query!("DELETE FROM org.teams WHERE id = $1", id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Count people (for backup manifest).
    pub async fn count_people(&self) -> Result<i64, Error> {
        sqlx::query_scalar!("SELECT COUNT(*) FROM org.people")
            .fetch_one(&self.pool)
            .await
            .map(|c| c.unwrap_or(0))
            .map_err(|e| Error::Database(e.to_string()))
    }

    /// Count teams (for backup manifest).
    pub async fn count_teams(&self) -> Result<i64, Error> {
        sqlx::query_scalar!("SELECT COUNT(*) FROM org.teams")
            .fetch_one(&self.pool)
            .await
            .map(|c| c.unwrap_or(0))
            .map_err(|e| Error::Database(e.to_string()))
    }

    /// Export all people as JSON rows (for backup).
    pub async fn export_people(&self) -> Result<Vec<serde_json::Value>, Error> {
        let people = sqlx::query!(
            "SELECT id, name, email, level, directory_id, created_at, updated_at FROM org.people"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(people
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "email": p.email,
                    "level": p.level,
                    "directory_id": p.directory_id,
                    "created_at": p.created_at.to_string(),
                    "updated_at": p.updated_at.to_string(),
                })
            })
            .collect())
    }

    /// Export all teams as JSON rows (for backup).
    pub async fn export_teams(&self) -> Result<Vec<serde_json::Value>, Error> {
        let teams = sqlx::query!(
            r#"SELECT id, name, org_name, parent_team_id, lead_id, github_team_slug,
                      team_type AS "team_type: TeamType", created_at
               FROM org.teams"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(teams
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "name": t.name,
                    "org_name": t.org_name,
                    "parent_team_id": t.parent_team_id,
                    "lead_id": t.lead_id,
                    "github_team_slug": t.github_team_slug,
                    "team_type": t.team_type.to_string(),
                    "created_at": t.created_at.to_string(),
                })
            })
            .collect())
    }
}
