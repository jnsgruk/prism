pub mod activity;
pub mod auth;
pub mod config;
pub mod metrics;
pub mod org;
pub mod pagination;
pub mod reasoning;

pub use activity::ActivityRepo;
pub use auth::AuthRepo;
pub use config::ConfigRepo;
pub use metrics::MetricsRepo;
pub use org::OrgRepo;
pub use pagination::{PageCursor, PageRequest, PageResponse, SortDir, SortParams};
pub use reasoning::ReasoningRepo;

use sqlx::PgPool;

/// Escape `LIKE`/`ILIKE` wildcard characters in user-supplied search terms.
///
/// `%` and `_` are wildcards in `LIKE` patterns — this escapes them so they
/// match literally. Call before wrapping a search term with `%..%`.
pub fn escape_like(input: &str) -> String {
    input.replace('%', "\\%").replace('_', "\\_")
}

/// Bundle of all repositories, constructed from a single `PgPool`.
///
/// Services and handlers take this instead of raw `PgPool`.
#[derive(Clone)]
pub struct Repos {
    pub auth: AuthRepo,
    pub config: ConfigRepo,
    pub org: OrgRepo,
    pub activity: ActivityRepo,
    pub metrics: MetricsRepo,
    pub reasoning: ReasoningRepo,
}

impl Repos {
    pub fn new(pool: PgPool) -> Self {
        Self {
            auth: AuthRepo::new(pool.clone()),
            config: ConfigRepo::new(pool.clone()),
            org: OrgRepo::new(pool.clone()),
            activity: ActivityRepo::new(pool.clone()),
            metrics: MetricsRepo::new(pool.clone()),
            reasoning: ReasoningRepo::new(pool),
        }
    }
}
