use ps_proto::prism::v1::org_service_server::OrgService;
use ps_proto::prism::v1::{
    GetTeamRequest, GetTeamResponse, ImportDirectoryRequest, ImportDirectoryResponse,
    ListPeopleRequest, ListPeopleResponse, ListTeamsRequest, ListTeamsResponse, Person,
    PlatformIdentity, Team,
};
use serde::Deserialize;
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use tracing::info;
use uuid::Uuid;

use crate::directory::parse_directory_html;

use super::common::{db_err, require_auth};

/// A person row from the database (shared between `get_team` and `list_people`).
struct PersonRow {
    id: Uuid,
    name: String,
    email: Option<String>,
    level: Option<String>,
}

/// An identity row from the database.
struct IdentityRow {
    person_id: Uuid,
    platform: String,
    platform_username: String,
}

/// Build `Person` proto messages from person rows + their platform identities.
fn build_people(people: Vec<PersonRow>, identities: &[IdentityRow]) -> Vec<Person> {
    people
        .into_iter()
        .map(|p| {
            let person_identities: Vec<PlatformIdentity> = identities
                .iter()
                .filter(|i| i.person_id == p.id)
                .map(|i| PlatformIdentity {
                    platform: i.platform.clone(),
                    username: i.platform_username.clone(),
                })
                .collect();

            Person {
                id: p.id.to_string(),
                name: p.name,
                email: p.email,
                level: p.level,
                identities: person_identities,
            }
        })
        .collect()
}

pub struct OrgServiceImpl {
    pool: PgPool,
}

impl OrgServiceImpl {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[tonic::async_trait]
#[allow(clippy::too_many_lines)] // sqlx query macros need inline usage for offline type inference
impl OrgService for OrgServiceImpl {
    async fn list_teams(
        &self,
        request: Request<ListTeamsRequest>,
    ) -> Result<Response<ListTeamsResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let parent_filter: Option<Uuid> = req
            .parent_team_id
            .map(|id| id.parse::<Uuid>())
            .transpose()
            .map_err(|_| Status::invalid_argument("invalid parent_team_id"))?;

        // Use a single query with optional parent filter
        let teams = sqlx::query!(
            r#"
            SELECT t.id, t.name, t.org_name, t.parent_team_id, t.lead_id,
                   t.github_team_slug,
                   COUNT(tm.id)::int AS "member_count!"
            FROM org.teams t
            LEFT JOIN org.team_memberships tm ON tm.team_id = t.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            WHERE ($1::uuid IS NULL OR t.parent_team_id = $1)
            GROUP BY t.id
            ORDER BY t.name
            "#,
            parent_filter,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let teams = teams
            .into_iter()
            .map(|t| Team {
                id: t.id.to_string(),
                name: t.name,
                org_name: t.org_name,
                parent_team_id: t.parent_team_id.map(|id| id.to_string()),
                lead_id: t.lead_id.map(|id| id.to_string()),
                github_team_slug: t.github_team_slug,
                member_count: t.member_count,
            })
            .collect();

        Ok(Response::new(ListTeamsResponse { teams }))
    }

    async fn get_team(
        &self,
        request: Request<GetTeamRequest>,
    ) -> Result<Response<GetTeamResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let team_id: Uuid = req
            .team_id
            .parse()
            .map_err(|_| Status::invalid_argument("invalid team_id"))?;

        let team = sqlx::query!(
            r#"
            SELECT t.id, t.name, t.org_name, t.parent_team_id, t.lead_id,
                   t.github_team_slug,
                   COUNT(tm.id)::int AS "member_count!"
            FROM org.teams t
            LEFT JOIN org.team_memberships tm ON tm.team_id = t.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
            WHERE t.id = $1
            GROUP BY t.id
            "#,
            team_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("team not found"))?;

        // Fetch active members with their identities
        let members_rows = sqlx::query!(
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
        .map_err(db_err)?;

        let person_ids: Vec<Uuid> = members_rows.iter().map(|r| r.id).collect();

        let identity_rows = sqlx::query!(
            r#"
            SELECT person_id, platform, platform_username
            FROM org.platform_identities
            WHERE person_id = ANY($1)
            "#,
            &person_ids,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let identities: Vec<IdentityRow> = identity_rows
            .into_iter()
            .map(|i| IdentityRow {
                person_id: i.person_id,
                platform: i.platform,
                platform_username: i.platform_username,
            })
            .collect();

        let member_people: Vec<PersonRow> = members_rows
            .into_iter()
            .map(|p| PersonRow {
                id: p.id,
                name: p.name,
                email: p.email,
                level: p.level,
            })
            .collect();

        let members = build_people(member_people, &identities);

        let team_proto = Team {
            id: team.id.to_string(),
            name: team.name,
            org_name: team.org_name,
            parent_team_id: team.parent_team_id.map(|id| id.to_string()),
            lead_id: team.lead_id.map(|id| id.to_string()),
            github_team_slug: team.github_team_slug,
            member_count: team.member_count,
        };

        Ok(Response::new(GetTeamResponse {
            team: Some(team_proto),
            members,
        }))
    }

    async fn list_people(
        &self,
        request: Request<ListPeopleRequest>,
    ) -> Result<Response<ListPeopleResponse>, Status> {
        let _ctx = require_auth(&request)?;

        let people_rows = sqlx::query!(
            r#"
            SELECT id, name, email, level
            FROM org.people
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let person_ids: Vec<Uuid> = people_rows.iter().map(|r| r.id).collect();

        let identity_rows = sqlx::query!(
            r#"
            SELECT person_id, platform, platform_username
            FROM org.platform_identities
            WHERE person_id = ANY($1)
            "#,
            &person_ids,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let identities: Vec<IdentityRow> = identity_rows
            .into_iter()
            .map(|i| IdentityRow {
                person_id: i.person_id,
                platform: i.platform,
                platform_username: i.platform_username,
            })
            .collect();

        let person_rows: Vec<PersonRow> = people_rows
            .into_iter()
            .map(|p| PersonRow {
                id: p.id,
                name: p.name,
                email: p.email,
                level: p.level,
            })
            .collect();

        let people = build_people(person_rows, &identities);

        Ok(Response::new(ListPeopleResponse { people }))
    }

    async fn import_directory(
        &self,
        request: Request<ImportDirectoryRequest>,
    ) -> Result<Response<ImportDirectoryResponse>, Status> {
        let _ctx = require_auth(&request)?;
        let req = request.into_inner();

        let content = String::from_utf8(req.file_content)
            .map_err(|_| Status::invalid_argument("file content is not valid UTF-8"))?;

        let records = parse_file_content(&content)?;

        let mut people_imported = 0i32;
        let mut teams_created = 0i32;
        let mut identities_mapped = 0i32;
        let mut warnings = Vec::new();

        let mut tx = self.pool.begin().await.map_err(db_err)?;

        for record in &records {
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
                .map_err(db_err)?;

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
                    .map_err(db_err)?;

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
                    .map_err(db_err)?;

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
                .map_err(db_err)?;

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
                .map_err(db_err)?;

                let team_id = if let Some(id) = team_id {
                    id
                } else {
                    let new_id = Uuid::now_v7();
                    sqlx::query!(
                        r#"
                        INSERT INTO org.teams (id, name, org_name)
                        VALUES ($1, $2, $3)
                        "#,
                        new_id,
                        team_name,
                        org_name,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?;

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
                .map_err(db_err)?;

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
                    .map_err(db_err)?;
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
                .map_err(db_err)?;

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
                    .map_err(db_err)?;
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
                    .map_err(db_err)?;

                    identities_mapped += 1;
                }
            }
        }

        tx.commit().await.map_err(db_err)?;

        info!(
            people_imported,
            teams_created,
            identities_mapped,
            warnings = warnings.len(),
            "directory import complete"
        );

        Ok(Response::new(ImportDirectoryResponse {
            people_imported,
            teams_created,
            identities_mapped,
            warnings,
        }))
    }
}

/// Detect file format and parse into `DirectoryRecord` entries.
#[allow(clippy::result_large_err)]
fn parse_file_content(content: &str) -> Result<Vec<DirectoryRecord>, Status> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('<') || trimmed.starts_with("<!") {
        // HTML: parse Canonical staff directory format
        let people = parse_directory_html(content);
        if people.is_empty() {
            return Err(Status::invalid_argument(
                "no valid entries found in HTML directory file",
            ));
        }
        Ok(people
            .into_iter()
            .map(|p| {
                let mut identities = vec![
                    DirectoryIdentity {
                        platform: "github".to_owned(),
                        username: p.github_username,
                    },
                    DirectoryIdentity {
                        platform: "launchpad".to_owned(),
                        username: p.launchpad_username,
                    },
                ];
                if let Some(mm) = p.mattermost_username {
                    identities.push(DirectoryIdentity {
                        platform: "mattermost".to_owned(),
                        username: mm,
                    });
                }
                DirectoryRecord {
                    name: p.display_name,
                    email: Some(p.email),
                    level: p.title,
                    directory_id: None,
                    team: p.group,
                    org: Some("Canonical".to_owned()),
                    identities,
                }
            })
            .collect())
    } else {
        // JSON
        serde_json::from_str(content)
            .map_err(|e| Status::invalid_argument(format!("invalid JSON: {e}")))
    }
}

/// A single record in a directory import file.
#[derive(Deserialize)]
struct DirectoryRecord {
    name: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    directory_id: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    org: Option<String>,
    #[serde(default)]
    identities: Vec<DirectoryIdentity>,
}

#[derive(Deserialize)]
struct DirectoryIdentity {
    platform: String,
    username: String,
}
