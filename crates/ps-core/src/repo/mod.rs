pub mod activity;
pub mod auth;
pub mod config;
pub mod org;

pub use activity::ActivityRepo;
pub use auth::AuthRepo;
pub use config::ConfigRepo;
pub use org::OrgRepo;

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
}

impl Repos {
    pub fn new(pool: PgPool) -> Self {
        Self {
            auth: AuthRepo::new(pool.clone()),
            config: ConfigRepo::new(pool.clone()),
            org: OrgRepo::new(pool.clone()),
            activity: ActivityRepo::new(pool),
        }
    }
}
