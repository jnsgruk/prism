use ps_core::auth::{generate_token, hash_password, hash_token};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Create the initial admin user and return `(user_id, session_token)`.
pub async fn create_admin_user(pool: &PgPool) -> (Uuid, String) {
    let user_id = Uuid::now_v7();
    let password_hash = hash_password("test-password-123").expect("hash password");

    sqlx::query!(
        r#"
        INSERT INTO auth.users (id, username, display_name, password_hash, role)
        VALUES ($1, 'admin', 'Test Admin', $2, 'admin')
        "#,
        user_id,
        password_hash,
    )
    .execute(pool)
    .await
    .expect("create admin user");

    let raw_token = generate_token();
    let token_hash = hash_token(&raw_token);
    let session_id = Uuid::now_v7();
    let expires_at = OffsetDateTime::now_utc() + time::Duration::days(7);

    sqlx::query!(
        r#"
        INSERT INTO auth.sessions (id, user_id, token_hash, session_type, expires_at)
        VALUES ($1, $2, $3, 'browser', $4)
        "#,
        session_id,
        user_id,
        token_hash,
        expires_at,
    )
    .execute(pool)
    .await
    .expect("create session");

    (user_id, raw_token)
}
