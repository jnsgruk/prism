pub mod activity;
pub mod auth;
pub mod config;
pub mod metrics;
pub mod org;
pub mod pagination;

pub use activity::ActivityRepo;
pub use auth::AuthRepo;
pub use config::ConfigRepo;
pub use metrics::MetricsRepo;
pub use org::OrgRepo;
pub use pagination::{PageCursor, PageRequest, PageResponse, SortDir, SortParams};

use sqlx::PgPool;

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
}

impl Repos {
    pub fn new(pool: PgPool) -> Self {
        Self {
            auth: AuthRepo::new(pool.clone()),
            config: ConfigRepo::new(pool.clone()),
            org: OrgRepo::new(pool.clone()),
            activity: ActivityRepo::new(pool.clone()),
            metrics: MetricsRepo::new(pool),
        }
    }
}
