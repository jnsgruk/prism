use sqlx::PgPool;
use tonic::{Request, Status};
use tracing::warn;
use uuid::Uuid;

/// Context extracted from a validated session, attached to request extensions.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub session_id: Uuid,
}

/// RPCs that do not require authentication.
const PUBLIC_METHODS: &[&str] = &[
    "/prism.v1.AuthService/GetSetupStatus",
    "/prism.v1.AuthService/CompleteSetup",
    "/prism.v1.AuthService/PreviewBackup",
    "/prism.v1.AuthService/RestoreBackup",
    "/prism.v1.AuthService/Login",
];

/// Validate the session token from request metadata and return an `AuthContext`.
/// Returns None for public RPCs (which don't need auth).
/// Returns Err(Status) for auth failures on protected RPCs.
pub async fn validate_request<T>(
    pool: &PgPool,
    request: &Request<T>,
    method: &str,
) -> Result<Option<AuthContext>, Status> {
    if PUBLIC_METHODS.contains(&method) {
        return Ok(None);
    }

    let token = request
        .metadata()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| Status::unauthenticated("missing or invalid authorization header"))?;

    let token_hash = ps_core::auth::hash_token(token);

    let session = sqlx::query!(
        r#"
        SELECT s.id as session_id, s.expires_at, u.id as user_id,
               u.username, u.display_name, u.role, u.is_active
        FROM auth.sessions s
        JOIN auth.users u ON s.user_id = u.id
        WHERE s.token_hash = $1
        "#,
        token_hash,
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        warn!(error = %e, "auth interceptor database error");
        Status::internal("internal server error")
    })?
    .ok_or_else(|| Status::unauthenticated("invalid session token"))?;

    if !session.is_active {
        return Err(Status::unauthenticated("account is disabled"));
    }

    if let Some(expires_at) = session.expires_at
        && expires_at < time::OffsetDateTime::now_utc()
    {
        return Err(Status::unauthenticated("session expired"));
    }

    // Fire-and-forget: update last_active_at
    let touch_pool = pool.clone();
    let sid = session.session_id;
    tokio::spawn(async move {
        let _ = sqlx::query!(
            "UPDATE auth.sessions SET last_active_at = now() WHERE id = $1",
            sid,
        )
        .execute(&touch_pool)
        .await;
    });

    Ok(Some(AuthContext {
        user_id: session.user_id,
        username: session.username,
        display_name: session.display_name,
        role: session.role,
        session_id: session.session_id,
    }))
}
