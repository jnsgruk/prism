/// Top-level error type for the Prism core library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("authentication error: {0}")]
    Authentication(String),

    #[error("encryption error: {0}")]
    Encryption(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("backup error: {0}")]
    Backup(String),

    #[error("rate limited: retry after {retry_after_secs}s")]
    RateLimit { retry_after_secs: u64 },

    #[error("HTTP {status}: {message}")]
    HttpStatus { status: u16, message: String },

    #[error("{0}")]
    Internal(String),
}

impl Error {
    /// Check whether this error represents a rate-limit response.
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, Self::RateLimit { .. })
    }

    /// Check whether this error is transient and worth retrying.
    ///
    /// Covers server errors (5xx) from `HttpStatus`, and timeout/connection
    /// errors surfaced as `Internal` strings from reqwest.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::HttpStatus { status, .. } => *status >= 500,
            Self::Internal(msg) => {
                let m = msg.to_lowercase();
                m.contains("timed out")
                    || m.contains("timeout")
                    || m.contains("connection reset")
                    || m.contains("connection closed")
                    || m.contains("broken pipe")
            }
            _ => false,
        }
    }

    /// Check whether this error is a database unique-constraint violation.
    pub fn is_unique_violation(&self) -> bool {
        if let Self::Database(e) = self
            && let Some(pg) = e.as_database_error()
        {
            return pg.is_unique_violation();
        }
        false
    }
}

impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(Box::new(err))
    }
}
