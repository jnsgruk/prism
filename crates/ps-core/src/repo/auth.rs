use crate::Error;
use crate::backup::UserBackupRow;
use crate::models::Role;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Repository for the `auth` schema: users and sessions.
#[derive(Clone)]
pub struct AuthRepo {
    pool: PgPool,
}

/// A validated session joined with user data.
pub struct SessionWithUser {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub is_active: bool,
    pub expires_at: Option<OffsetDateTime>,
}

/// Credentials returned when looking up a user by username.
pub struct UserCredentials {
    pub id: Uuid,
    pub password_hash: String,
    pub is_active: bool,
}

/// An API token row for listing.
pub struct ApiTokenRow {
    pub id: Uuid,
    pub token_name: Option<String>,
    pub created_at: OffsetDateTime,
    pub last_active_at: OffsetDateTime,
}

impl AuthRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Check whether any users exist in the database.
    pub async fn any_users_exist(&self) -> Result<bool, Error> {
        sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM auth.users)")
            .fetch_one(&self.pool)
            .await
            .map(|v| v.unwrap_or(false))
            .map_err(Error::from)
    }

    /// Create a new user.
    pub async fn create_user(
        &self,
        id: Uuid,
        username: &str,
        display_name: &str,
        password_hash: &str,
        role: Role,
    ) -> Result<(), Error> {
        let role_str = role.to_string();
        sqlx::query!(
            r#"
            INSERT INTO auth.users (id, username, display_name, password_hash, role)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            id,
            username,
            display_name,
            password_hash,
            role_str,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Find a user by username, returning credentials for password verification.
    pub async fn find_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<UserCredentials>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT id, password_hash, is_active
            FROM auth.users
            WHERE username = $1
            "#,
            username,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row.map(|r| UserCredentials {
            id: r.id,
            password_hash: r.password_hash,
            is_active: r.is_active,
        }))
    }

    /// Create a new session (browser or API token).
    pub async fn create_session(
        &self,
        id: Uuid,
        user_id: Uuid,
        token_hash: &str,
        session_type: &str,
        expires_at: Option<OffsetDateTime>,
        token_name: Option<&str>,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO auth.sessions (id, user_id, token_hash, session_type, expires_at, token_name)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            id,
            user_id,
            token_hash,
            session_type,
            expires_at,
            token_name,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// Delete a session by ID.
    pub async fn delete_session(&self, session_id: Uuid) -> Result<(), Error> {
        sqlx::query!("DELETE FROM auth.sessions WHERE id = $1", session_id)
            .execute(&self.pool)
            .await
            .map_err(Error::from)?;

        Ok(())
    }

    /// Validate a session by token hash, returning session + user data.
    pub async fn validate_session(
        &self,
        token_hash: &str,
    ) -> Result<Option<SessionWithUser>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT s.id as session_id, s.expires_at, u.id as user_id,
                   u.username, u.display_name, u.role, u.is_active
            FROM auth.sessions s
            JOIN auth.users u ON s.user_id = u.id
            WHERE s.token_hash = $1
            "#,
            token_hash,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        row.map(|r| -> Result<SessionWithUser, Error> {
            let role = r.role.parse::<Role>().map_err(Error::Validation)?;
            Ok(SessionWithUser {
                session_id: r.session_id,
                user_id: r.user_id,
                username: r.username,
                display_name: r.display_name,
                role,
                is_active: r.is_active,
                expires_at: r.expires_at,
            })
        })
        .transpose()
    }

    /// Update `last_active_at` for a session (fire-and-forget).
    pub async fn touch_session(&self, session_id: Uuid) -> Result<(), Error> {
        sqlx::query!(
            "UPDATE auth.sessions SET last_active_at = now() WHERE id = $1",
            session_id,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    /// List API tokens for a user.
    pub async fn list_api_tokens(&self, user_id: Uuid) -> Result<Vec<ApiTokenRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, token_name, created_at, last_active_at
            FROM auth.sessions
            WHERE user_id = $1 AND session_type = 'api_token'
            ORDER BY created_at DESC
            "#,
            user_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|t| ApiTokenRow {
                id: t.id,
                token_name: t.token_name,
                created_at: t.created_at,
                last_active_at: t.last_active_at,
            })
            .collect())
    }

    /// Delete an API token belonging to a user. Returns true if deleted.
    pub async fn delete_api_token(&self, token_id: Uuid, user_id: Uuid) -> Result<bool, Error> {
        let result = sqlx::query!(
            r#"
            DELETE FROM auth.sessions
            WHERE id = $1 AND user_id = $2 AND session_type = 'api_token'
            "#,
            token_id,
            user_id,
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(result.rows_affected() > 0)
    }

    /// Count users (for backup manifest).
    pub async fn count_users(&self) -> Result<i64, Error> {
        sqlx::query_scalar!("SELECT COUNT(*) FROM auth.users")
            .fetch_one(&self.pool)
            .await
            .map(|c| c.unwrap_or(0))
            .map_err(Error::from)
    }

    /// Export all users as JSON rows (for backup, without password hashes).
    pub async fn export_users(&self) -> Result<Vec<serde_json::Value>, Error> {
        let users = sqlx::query!(
            "SELECT id, username, display_name, role, is_active, person_id, created_at, updated_at FROM auth.users"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(users
            .iter()
            .map(|u| {
                serde_json::json!({
                    "id": u.id,
                    "username": u.username,
                    "display_name": u.display_name,
                    "role": u.role,
                    "is_active": u.is_active,
                    "person_id": u.person_id,
                    "created_at": u.created_at.to_string(),
                    "updated_at": u.updated_at.to_string(),
                })
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // Backup export/import (full — includes password hashes)
    // -----------------------------------------------------------------------

    /// Export all users for backup, **including password hashes**.
    ///
    /// This is intentionally separate from `export_users` (which omits hashes)
    /// and should only be called from the backup pipeline.
    pub async fn export_users_for_backup(&self) -> Result<Vec<UserBackupRow>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT id, username, display_name, password_hash, role, is_active,
                   person_id, created_at, updated_at
            FROM auth.users
            ORDER BY created_at
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(rows
            .into_iter()
            .map(|r| UserBackupRow {
                id: r.id,
                username: r.username,
                display_name: r.display_name,
                password_hash: r.password_hash,
                role: r.role,
                is_active: r.is_active,
                person_id: r.person_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    /// Import users from backup. Upserts on `username`.
    ///
    /// Existing users are updated with the backup values (including
    /// password hash), so the restored instance is a faithful copy.
    /// Returns the number of rows upserted.
    pub async fn import_users(&self, rows: &[UserBackupRow]) -> Result<i64, Error> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mut count: i64 = 0;
        for chunk in rows.chunks(1000) {
            let ids: Vec<Uuid> = chunk.iter().map(|r| r.id).collect();
            let usernames: Vec<&str> = chunk.iter().map(|r| r.username.as_str()).collect();
            let display_names: Vec<&str> = chunk.iter().map(|r| r.display_name.as_str()).collect();
            let password_hashes: Vec<&str> =
                chunk.iter().map(|r| r.password_hash.as_str()).collect();
            let roles: Vec<&str> = chunk.iter().map(|r| r.role.as_str()).collect();
            let is_actives: Vec<bool> = chunk.iter().map(|r| r.is_active).collect();
            let person_ids: Vec<Option<Uuid>> = chunk.iter().map(|r| r.person_id).collect();
            let created_ats: Vec<OffsetDateTime> = chunk.iter().map(|r| r.created_at).collect();
            let updated_ats: Vec<OffsetDateTime> = chunk.iter().map(|r| r.updated_at).collect();

            sqlx::query!(
                r#"
                INSERT INTO auth.users
                    (id, username, display_name, password_hash, role, is_active,
                     person_id, created_at, updated_at)
                SELECT
                    unnest($1::uuid[]),
                    unnest($2::text[]),
                    unnest($3::text[]),
                    unnest($4::text[]),
                    unnest($5::text[]),
                    unnest($6::bool[]),
                    unnest($7::uuid[]),
                    unnest($8::timestamptz[]),
                    unnest($9::timestamptz[])
                ON CONFLICT (username) DO UPDATE
                    SET display_name  = EXCLUDED.display_name,
                        password_hash = EXCLUDED.password_hash,
                        role          = EXCLUDED.role,
                        is_active     = EXCLUDED.is_active,
                        person_id     = EXCLUDED.person_id,
                        updated_at    = EXCLUDED.updated_at
                "#,
                &ids,
                &usernames as &[&str],
                &display_names as &[&str],
                &password_hashes as &[&str],
                &roles as &[&str],
                &is_actives,
                &person_ids as &[Option<Uuid>],
                &created_ats,
                &updated_ats,
            )
            .execute(&self.pool)
            .await
            .map_err(Error::from)?;

            count += i64::try_from(chunk.len()).unwrap_or(i64::MAX);
        }
        Ok(count)
    }

    /// Delete all users and sessions (used during full overwrite restore).
    pub async fn delete_all_users(&self) -> Result<(), Error> {
        let mut tx = self.pool.begin().await.map_err(Error::from)?;
        sqlx::query!("DELETE FROM auth.sessions")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!("DELETE FROM auth.users")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        tx.commit().await.map_err(Error::from)
    }

    /// Return the first active admin user, if any, ordered by creation time.
    ///
    /// Used after restore to find an account to re-issue a session for.
    pub async fn find_first_admin_user(&self) -> Result<Option<UserBackupRow>, Error> {
        let row = sqlx::query!(
            r#"
            SELECT id, username, display_name, password_hash, role, is_active,
                   person_id, created_at, updated_at
            FROM auth.users
            WHERE role = 'admin' AND is_active = true
            ORDER BY created_at
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)?;

        Ok(row.map(|r| UserBackupRow {
            id: r.id,
            username: r.username,
            display_name: r.display_name,
            password_hash: r.password_hash,
            role: r.role,
            is_active: r.is_active,
            person_id: r.person_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }
}
