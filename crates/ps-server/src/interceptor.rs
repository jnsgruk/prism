use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::Request;
use ps_core::repo::auth::AuthRepo;
use tonic::Status;
use tower::{Layer, Service};
use tracing::{info, warn};
use uuid::Uuid;

/// Build an HTTP response that encodes a gRPC UNAUTHENTICATED error.
///
/// gRPC-over-HTTP uses HTTP 200 with status in trailers/headers. This lets
/// the auth middleware reject requests without depending on tonic's body type.
fn grpc_unauthenticated<B: Default>(message: &str) -> http::Response<B> {
    let mut response = http::Response::new(B::default());
    response.headers_mut().insert(
        "content-type",
        http::HeaderValue::from_static("application/grpc"),
    );
    // 16 = UNAUTHENTICATED in the gRPC status code space
    response
        .headers_mut()
        .insert("grpc-status", http::HeaderValue::from_static("16"));
    if let Ok(val) = http::HeaderValue::from_str(message) {
        response.headers_mut().insert("grpc-message", val);
    }
    response
}

/// Context extracted from a validated session, attached to request extensions.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: ps_core::models::Role,
    pub session_id: Uuid,
}

/// RPCs that do not require authentication.
const PUBLIC_METHODS: &[&str] = &[
    "/canonical.prism.v1.AuthService/GetSetupStatus",
    "/canonical.prism.v1.AuthService/CompleteSetup",
    "/canonical.prism.v1.AuthService/Login",
];

/// RPCs that require authentication only on initialised instances (at least
/// one user exists). On fresh/uninitialised instances these are open so that
/// backup preview and restore can work before any admin account is created.
const CONDITIONALLY_PUBLIC_METHODS: &[&str] = &[
    "/canonical.prism.v1.BackupService/PreviewBackup",
    "/canonical.prism.v1.BackupService/RestoreBackup",
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
    ResBody: Default + Send + 'static,
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
            if PUBLIC_METHODS.contains(&path.as_str()) || !path.starts_with("/canonical.prism.v1.")
            {
                return inner.call(req).await;
            }

            // Conditionally public: skip auth only when no users exist (fresh instance)
            if CONDITIONALLY_PUBLIC_METHODS.contains(&path.as_str()) {
                let setup_complete = auth_repo.any_users_exist().await.unwrap_or(true); // fail-closed: assume initialised on DB error
                if !setup_complete {
                    return inner.call(req).await;
                }
                // Fall through to normal auth validation for live instances
            }

            // Extract bearer token from authorization header
            let token = req
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(std::borrow::ToOwned::to_owned);

            let Some(token) = token else {
                info!(path, "rejected request: missing authorization header");
                return Ok(grpc_unauthenticated("missing authorization header"));
            };

            match validate_token(&auth_repo, &token).await {
                Ok(ctx) => {
                    req.extensions_mut().insert(ctx);
                    inner.call(req).await
                }
                Err(status) => {
                    info!(path, status = %status.code(), "rejected request: {}", status.message());
                    Ok(grpc_unauthenticated(status.message()))
                }
            }
        })
    }
}
