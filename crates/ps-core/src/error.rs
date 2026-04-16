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
                    || m.contains("error decoding")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_5xx_is_transient() {
        for status in [500, 502, 503, 504] {
            let e = Error::HttpStatus {
                status,
                message: "error".into(),
            };
            assert!(e.is_transient(), "HTTP {status} should be transient");
        }
    }

    #[test]
    fn http_4xx_not_transient() {
        for status in [400, 401, 403, 404, 422] {
            let e = Error::HttpStatus {
                status,
                message: "error".into(),
            };
            assert!(!e.is_transient(), "HTTP {status} should not be transient");
        }
    }

    #[test]
    fn rate_limit_not_transient() {
        let e = Error::RateLimit {
            retry_after_secs: 60,
        };
        assert!(!e.is_transient());
        assert!(e.is_rate_limit());
    }

    #[test]
    fn internal_timeout_transient() {
        for msg in [
            "request timed out",
            "connection timeout reached",
            "connection reset by peer",
            "connection closed before message completed",
            "broken pipe",
        ] {
            let e = Error::Internal(msg.into());
            assert!(e.is_transient(), "'{msg}' should be transient");
        }
    }

    #[test]
    fn internal_timeout_case_insensitive() {
        let e = Error::Internal("REQUEST TIMED OUT".into());
        assert!(e.is_transient());
    }

    #[test]
    fn internal_decode_error_transient() {
        let e = Error::Internal("jira response parse error: error decoding response body".into());
        assert!(e.is_transient());
    }

    #[test]
    fn internal_non_timeout_not_transient() {
        let e = Error::Internal("cursor serialisation failed".into());
        assert!(!e.is_transient());
    }

    #[test]
    fn other_variants_not_transient() {
        assert!(!Error::NotFound("x".into()).is_transient());
        assert!(!Error::Validation("x".into()).is_transient());
        assert!(!Error::Authentication("x".into()).is_transient());
        assert!(!Error::Encryption("x".into()).is_transient());
    }

    #[test]
    fn is_rate_limit_false_for_others() {
        assert!(
            !Error::HttpStatus {
                status: 429,
                message: "too many".into()
            }
            .is_rate_limit()
        );
        assert!(!Error::Internal("x".into()).is_rate_limit());
    }
}
