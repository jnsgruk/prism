use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::Request;
use sqlx::PgPool;
use tonic::Status;
use tower::{Layer, Service};
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

/// Validate a bearer token against the database and return an `AuthContext`.
async fn validate_token(pool: &PgPool, token: &str) -> Result<AuthContext, Status> {
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

    Ok(AuthContext {
        user_id: session.user_id,
        username: session.username,
        display_name: session.display_name,
        role: session.role,
        session_id: session.session_id,
    })
}

/// Tower layer that validates session tokens and attaches `AuthContext` to requests.
#[derive(Clone)]
pub struct AuthLayer {
    pool: PgPool,
}

impl AuthLayer {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            pool: self.pool.clone(),
        }
    }
}

/// Tower service that validates auth before forwarding requests.
#[derive(Clone)]
pub struct AuthMiddleware<S> {
    inner: S,
    pool: PgPool,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for AuthMiddleware<S>
where
    S: Service<Request<ReqBody>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    ReqBody: Send + 'static,
    ResBody: Send + 'static,
{
    type Response = http::Response<ResBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // Standard tower pattern: clone the ready service
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);

        let pool = self.pool.clone();

        Box::pin(async move {
            let path = req.uri().path().to_owned();

            // Skip auth for public methods and non-gRPC paths (health checks, etc.)
            if PUBLIC_METHODS.contains(&path.as_str()) || !path.starts_with("/prism.v1.") {
                return inner.call(req).await;
            }

            // Extract bearer token from authorization header
            let token = req
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(std::borrow::ToOwned::to_owned);

            let token = if let Some(t) = token {
                t
            } else {
                warn!(path, "rejected request: missing authorization header");
                return inner.call(req).await;
            };

            match validate_token(&pool, &token).await {
                Ok(ctx) => {
                    req.extensions_mut().insert(ctx);
                    inner.call(req).await
                }
                Err(_status) => {
                    // Let the request through without AuthContext — individual
                    // service handlers call require_auth() and will return the
                    // appropriate gRPC error with correct framing.
                    inner.call(req).await
                }
            }
        })
    }
}
