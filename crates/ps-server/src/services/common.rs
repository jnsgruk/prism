use crate::interceptor::AuthContext;
use tonic::{Request, Status};

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

/// Map a database/repo error to a gRPC `Internal` status.
pub fn db_err(e: impl std::fmt::Display) -> Status {
    Status::internal(format!("database error: {e}"))
}

/// Map a backup I/O error to a gRPC `Internal` status.
pub fn backup_err(e: impl std::fmt::Display) -> Status {
    Status::internal(format!("backup error: {e}"))
}

/// Convert an `OffsetDateTime` to a prost `Timestamp`.
pub fn to_timestamp(dt: time::OffsetDateTime) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: dt.unix_timestamp(),
        nanos: 0,
    }
}
