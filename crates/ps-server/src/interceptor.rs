use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::Request;
use ps_core::repo::auth::AuthRepo;
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
async fn validate_token(auth_repo: &AuthRepo, token: &str) -> Result<AuthContext, Status> {
    let token_hash = ps_core::auth::hash_token(token);

    let session = auth_repo
        .validate_session(&token_hash)
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
    let touch_repo = auth_repo.clone();
    let sid = session.session_id;
    tokio::spawn(async move {
        let _ = touch_repo.touch_session(sid).await;
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
    auth_repo: AuthRepo,
}

impl AuthLayer {
    pub fn new(auth_repo: AuthRepo) -> Self {
        Self { auth_repo }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            auth_repo: self.auth_repo.clone(),
        }
    }
}

/// Tower service that validates auth before forwarding requests.
#[derive(Clone)]
pub struct AuthMiddleware<S> {
    inner: S,
    auth_repo: AuthRepo,
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

        let auth_repo = self.auth_repo.clone();

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

            let Some(token) = token else {
                warn!(path, "rejected request: missing authorization header");
                return inner.call(req).await;
            };

            match validate_token(&auth_repo, &token).await {
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
