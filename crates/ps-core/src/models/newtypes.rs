//! Newtype wrappers for domain identifiers.
//!
//! These provide compile-time safety against parameter confusion (e.g. passing
//! a handler name where a source name is expected). They deref to `str` for
//! ergonomic use with sqlx and other APIs that accept `&str`.

/// Generate a newtype wrapper around `String` with standard trait impls.
macro_rules! impl_string_newtype {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

// ---------------------------------------------------------------------------
// GitHubRepoCoord
// ---------------------------------------------------------------------------

/// A GitHub repository coordinate (org + repo name).
///
/// Replaces bare `(String, String)` tuples where org and repo could be
/// accidentally swapped.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct GitHubRepoCoord {
    pub org: String,
    pub repo: String,
}

impl GitHubRepoCoord {
    pub fn new(org: impl Into<String>, repo: impl Into<String>) -> Self {
        Self {
            org: org.into(),
            repo: repo.into(),
        }
    }
}

impl std::fmt::Display for GitHubRepoCoord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.org, self.repo)
    }
}

// ---------------------------------------------------------------------------
// String newtypes
// ---------------------------------------------------------------------------

impl_string_newtype!(
    SourceName,
    "The human-visible name of a data source (e.g. \"Ubuntu GitHub\")."
);

impl_string_newtype!(
    HandlerName,
    "The Restate handler class name (e.g. \"GithubIngestionHandler\")."
);

impl_string_newtype!(
    HandlerMethod,
    "The method on a Restate handler (e.g. \"run_ingestion\")."
);

impl_string_newtype!(
    PlatformId,
    "The external platform's unique identifier for a contribution (e.g. GitHub PR URL, Jira ticket key)."
);

impl_string_newtype!(
    PlatformUsername,
    "A username on an external platform (e.g. GitHub login, Jira account name)."
);
