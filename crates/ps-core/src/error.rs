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

    #[error("{0}")]
    Internal(String),
}

impl Error {
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
