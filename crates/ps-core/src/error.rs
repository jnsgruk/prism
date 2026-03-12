/// Top-level error type for the Prism core library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(String),

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

    #[error("{0}")]
    Internal(String),
}

impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err.to_string())
    }
}
