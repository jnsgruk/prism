use crate::interceptor::AuthContext;
use tonic::{Request, Status};
use tracing::error;

/// Extract the authenticated user context from a gRPC request.
///
/// Returns `Unauthenticated` if the auth interceptor did not attach a context
/// (i.e. the RPC is not on the public allow-list but no valid token was sent).
#[allow(clippy::result_large_err)]
pub fn require_auth<T>(request: &Request<T>) -> Result<AuthContext, Status> {
    request
        .extensions()
        .get::<AuthContext>()
        .cloned()
        .ok_or_else(|| Status::unauthenticated("not authenticated"))
}

/// Extract authenticated user context and verify the user has the admin role.
#[allow(clippy::result_large_err)]
pub fn require_admin<T>(request: &Request<T>) -> Result<AuthContext, Status> {
    let ctx = require_auth(request)?;
    if ctx.role != ps_core::models::Role::Admin {
        return Err(Status::permission_denied("admin role required"));
    }
    Ok(ctx)
}

/// Map a database/repo error to a gRPC `Internal` status.
///
/// Logs the full error server-side but returns a generic message to the client
/// to avoid leaking internal details (table names, constraints, query fragments).
pub fn db_err(e: impl std::fmt::Display) -> Status {
    error!(error = %e, "database error");
    Status::internal("internal error")
}

/// Map a backup I/O error to a gRPC `Internal` status.
pub fn backup_err(e: impl std::fmt::Display) -> Status {
    error!(error = %e, "backup error");
    Status::internal("internal error")
}
