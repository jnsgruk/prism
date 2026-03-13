use crate::Error;
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
    pub role: String,
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
            .map_err(|e| Error::Database(e.to_string()))
    }

    /// Create a new user.
    pub async fn create_user(
        &self,
        id: Uuid,
        username: &str,
        display_name: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO auth.users (id, username, display_name, password_hash, role)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            id,
            username,
            display_name,
            password_hash,
            role,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

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
        .map_err(|e| Error::Database(e.to_string()))?;

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
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Delete a session by ID.
    pub async fn delete_session(&self, session_id: Uuid) -> Result<(), Error> {
        sqlx::query!("DELETE FROM auth.sessions WHERE id = $1", session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

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
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(row.map(|r| SessionWithUser {
            session_id: r.session_id,
            user_id: r.user_id,
            username: r.username,
            display_name: r.display_name,
            role: r.role,
            is_active: r.is_active,
            expires_at: r.expires_at,
        }))
    }

    /// Update `last_active_at` for a session (fire-and-forget).
    pub async fn touch_session(&self, session_id: Uuid) -> Result<(), Error> {
        sqlx::query!(
            "UPDATE auth.sessions SET last_active_at = now() WHERE id = $1",
            session_id,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

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
        .map_err(|e| Error::Database(e.to_string()))?;

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
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    /// Count users (for backup manifest).
    pub async fn count_users(&self) -> Result<i64, Error> {
        sqlx::query_scalar!("SELECT COUNT(*) FROM auth.users")
            .fetch_one(&self.pool)
            .await
            .map(|c| c.unwrap_or(0))
            .map_err(|e| Error::Database(e.to_string()))
    }

    /// Export all users as JSON rows (for backup, without password hashes).
    pub async fn export_users(&self) -> Result<Vec<serde_json::Value>, Error> {
        let users = sqlx::query!(
            "SELECT id, username, display_name, role, is_active, person_id, created_at, updated_at FROM auth.users"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

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
}
