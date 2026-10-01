//! Atomic administrator-managed people and explicit account ownership operations.
use super::{IdentityRow, OrgRepo, PersonRow};
use crate::{
    Error,
    models::{Management, PersonId, Platform, ResolutionStatus, TeamId},
};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct IdentityInput {
    pub platform: Platform,
    pub username: String,
    pub platform_user_id: Option<String>,
}

pub struct CreatePersonParams {
    pub name: String,
    pub email: Option<String>,
    pub level: Option<String>,
    pub team_id: Option<TeamId>,
    pub identities: Vec<IdentityInput>,
}

pub struct UpdateIdentityParams {
    pub person_id: PersonId,
    pub identity_id: Uuid,
    pub username: Option<String>,
    /// None preserves the value; Some(None) clears it; Some(Some(id)) sets it.
    pub platform_user_id: Option<Option<String>>,
}

pub struct ManualPersonResult {
    pub person: PersonRow,
    pub identities: Vec<IdentityRow>,
}

fn validation(message: &str) -> Error {
    Error::Validation(message.into())
}

pub fn validate_person_fields(name: &str, email: Option<&str>) -> Result<(), Error> {
    if name.trim().is_empty() || name.len() > 200 || name.chars().any(char::is_control) {
        return Err(validation(
            "name must contain 1 to 200 printable characters",
        ));
    }
    if let Some(email) = email {
        let valid = email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains('@')
        });
        if email.len() > 254
            || email.chars().any(char::is_whitespace)
            || email.chars().any(char::is_control)
            || !valid
        {
            return Err(validation("invalid email"));
        }
    }
    Ok(())
}

impl IdentityInput {
    pub fn normalize(mut self) -> Result<Self, Error> {
        self.username = self.username.trim().to_owned();
        if self.username.is_empty()
            || self.username.len() > 254
            || (self.platform != Platform::Jira && self.username.chars().any(char::is_whitespace))
            || self.username.chars().any(char::is_control)
        {
            return Err(validation("invalid platform username"));
        }
        match &self.platform {
            Platform::Github => {
                if self.username.len() > 39
                    || self.username.starts_with('-')
                    || self.username.ends_with('-')
                    || self.username.contains("--")
                    || !self
                        .username
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                {
                    return Err(validation("invalid GitHub username"));
                }
            }
            Platform::Discourse(instance) => {
                if instance.is_empty()
                    || instance.len() > 100
                    || !instance
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                    || instance.starts_with('-')
                    || instance.ends_with('-')
                {
                    return Err(validation("invalid Discourse instance"));
                }
                if self.username.len() > 60
                    || !self
                        .username
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
                {
                    return Err(validation("invalid Discourse username"));
                }
            }
            _ => {}
        }
        // Usernames are lookup keys; opaque external IDs retain their exact case.
        self.username = self.username.to_lowercase();
        if let Some(id) = &self.platform_user_id
            && (id.is_empty()
                || id.len() > 255
                || id.chars().any(char::is_whitespace)
                || id.chars().any(char::is_control))
        {
            return Err(validation("invalid platform user ID"));
        }
        if self.platform == Platform::Jira && self.platform_user_id.is_none() {
            return Err(validation("Jira account ID is required"));
        }
        Ok(self)
    }
}

fn write_error(error: sqlx::Error) -> Error {
    if let Some(db) = error.as_database_error() {
        if db.is_unique_violation() {
            tracing::warn!(error = %error, "manual account ownership conflict");
            return Error::Conflict("account is already assigned".into());
        }
        if db.is_foreign_key_violation() {
            tracing::warn!(error = %error, "manual person references missing record");
            return Error::NotFound("person or team not found".into());
        }
    }
    Error::from(error)
}

async fn lock_person(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<(), Error> {
    sqlx::query_scalar!("SELECT id FROM org.people WHERE id = $1 FOR UPDATE", id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| Error::NotFound("person not found".into()))?;
    Ok(())
}

async fn insert_accounts(
    tx: &mut Transaction<'_, Postgres>,
    person: Uuid,
    identities: Vec<IdentityInput>,
) -> Result<(), Error> {
    if identities.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = identities.iter().map(|_| Uuid::now_v7()).collect();
    let platforms: Vec<String> = identities.iter().map(|i| i.platform.to_string()).collect();
    let usernames: Vec<String> = identities.iter().map(|i| i.username.clone()).collect();
    let user_ids: Vec<Option<String>> =
        identities.into_iter().map(|i| i.platform_user_id).collect();
    let management = Management::Manual;
    sqlx::query!(r#"INSERT INTO org.platform_identities (id,person_id,platform,platform_username,platform_user_id,management)
        SELECT id,$2,platform,username,user_id,$6 FROM UNNEST($1::uuid[],$3::text[],$4::text[],$5::text[]) AS input(id,platform,username,user_id)"#,
        &ids,person,&platforms,&usernames,&user_ids as &[Option<String>], management as Management)
        .execute(&mut **tx).await.map_err(write_error)?;
    let status = ResolutionStatus::Manual;
    sqlx::query!("INSERT INTO org.identity_resolutions (person_id,platform,status) SELECT $1,platform,$3 FROM UNNEST($2::text[]) AS input(platform) GROUP BY platform ON CONFLICT (person_id,platform) DO UPDATE SET status=EXCLUDED.status",person,&platforms,status as ResolutionStatus)
        .execute(&mut **tx).await?;
    Ok(())
}

async fn mark_manual(
    tx: &mut Transaction<'_, Postgres>,
    person_id: Uuid,
    platform: &str,
) -> Result<(), Error> {
    let status = ResolutionStatus::Manual;
    sqlx::query!("INSERT INTO org.identity_resolutions (person_id,platform,status) VALUES ($1,$2,$3) ON CONFLICT (person_id,platform) DO UPDATE SET status=EXCLUDED.status",person_id,platform,status as ResolutionStatus)
        .execute(&mut **tx).await?;
    Ok(())
}

// Read the complete response before commit while the person's write lock is held.
async fn manual_result(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<ManualPersonResult, Error> {
    let person = sqlx::query_as!(PersonRow, r#"
        SELECT p.id,p.name,p.email,p.level,p.active,tm.team_id AS "team_id?",t.name AS "team_name?"
        FROM org.people p
        LEFT JOIN org.team_memberships tm ON tm.person_id=p.id AND (tm.end_date IS NULL OR tm.end_date>CURRENT_DATE)
        LEFT JOIN org.teams t ON t.id=tm.team_id WHERE p.id=$1
        "#,id).fetch_one(&mut **tx).await?;
    let identities = sqlx::query_as!(IdentityRow, r#"
        SELECT id,person_id,platform,platform_username,platform_user_id,management AS "management!: Management"
        FROM org.platform_identities WHERE person_id=$1 ORDER BY id
        "#,id).fetch_all(&mut **tx).await?;
    Ok(ManualPersonResult { person, identities })
}

impl OrgRepo {
    pub async fn create_person(
        &self,
        params: CreatePersonParams,
    ) -> Result<ManualPersonResult, Error> {
        validate_person_fields(&params.name, params.email.as_deref())?;
        if params
            .level
            .as_ref()
            .is_some_and(|l| l.len() > 200 || l.chars().any(char::is_control))
        {
            return Err(validation("invalid level"));
        }
        let identities: Vec<IdentityInput> = params
            .identities
            .into_iter()
            .map(IdentityInput::normalize)
            .collect::<Result<_, _>>()?;
        let mut tx = self.pool.begin().await?;
        let id = Uuid::now_v7();
        let management = Management::Manual;
        sqlx::query!("INSERT INTO org.people (id,name,email,level,membership_management) VALUES ($1,$2,$3,$4,$5)", id,params.name.trim(),params.email,params.level,management as Management)
            .execute(&mut *tx).await.map_err(write_error)?;
        insert_accounts(&mut tx, id, identities).await?;
        if let Some(team) = params.team_id {
            let membership = Uuid::now_v7();
            let team = team.into_inner();
            sqlx::query!("INSERT INTO org.team_memberships (id,person_id,team_id,start_date) VALUES ($1,$2,$3,CURRENT_DATE)",membership,id,team)
                .execute(&mut *tx).await.map_err(write_error)?;
        }
        let result = manual_result(&mut tx, id).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn add_person_identity(
        &self,
        person_id: PersonId,
        identity: IdentityInput,
    ) -> Result<ManualPersonResult, Error> {
        let identity = identity.normalize()?;
        let id = person_id.into_inner();
        let mut tx = self.pool.begin().await?;
        lock_person(&mut tx, id).await?;
        insert_accounts(&mut tx, id, vec![identity]).await?;
        let result = manual_result(&mut tx, id).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn update_person_identity(
        &self,
        params: UpdateIdentityParams,
    ) -> Result<ManualPersonResult, Error> {
        let id = params.person_id.into_inner();
        let mut tx = self.pool.begin().await?;
        lock_person(&mut tx, id).await?;
        let row = sqlx::query!("SELECT platform,platform_username,platform_user_id FROM org.platform_identities WHERE id=$1 AND person_id=$2 FOR UPDATE",params.identity_id,id)
            .fetch_optional(&mut *tx).await?.ok_or_else(|| Error::NotFound("identity not found for person".into()))?;
        let identity = IdentityInput {
            platform: row
                .platform
                .parse()
                .map_err(|_| Error::Internal("invalid persisted platform".into()))?,
            username: params.username.unwrap_or(row.platform_username),
            platform_user_id: params.platform_user_id.unwrap_or(row.platform_user_id),
        }
        .normalize()?;
        let management = Management::Manual;
        sqlx::query!("UPDATE org.platform_identities SET platform_username=$3,platform_user_id=$4,management=$5 WHERE id=$1 AND person_id=$2",params.identity_id,id,identity.username,identity.platform_user_id,management as Management)
            .execute(&mut *tx).await.map_err(write_error)?;
        mark_manual(&mut tx, id, &row.platform).await?;
        let result = manual_result(&mut tx, id).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn remove_person_identity(
        &self,
        person_id: PersonId,
        identity_id: Uuid,
    ) -> Result<ManualPersonResult, Error> {
        let id = person_id.into_inner();
        let mut tx = self.pool.begin().await?;
        lock_person(&mut tx, id).await?;
        let result = sqlx::query!(
            "DELETE FROM org.platform_identities WHERE id=$1 AND person_id=$2 RETURNING platform",
            identity_id,
            id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| Error::NotFound("identity not found for person".into()))?;
        mark_manual(&mut tx, id, &result.platform).await?;
        let result = manual_result(&mut tx, id).await?;
        tx.commit().await?;
        Ok(result)
    }
}
