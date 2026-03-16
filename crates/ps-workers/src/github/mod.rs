pub mod client;
pub mod etag;
pub mod graphql;
pub mod repos;
pub mod source;
pub mod types;

pub use graphql::GitHubGraphQLClient;
pub use source::GitHubSource;

// REST client re-exported for team sync handler (which still uses REST).
pub use client::GitHubClient;
