use sqlx::PgPool;

/// Repository for the `org` schema: people, teams, platform identities,
/// team memberships, and repositories.
#[derive(Clone)]
pub struct OrgRepo {
    pool: PgPool,
}

impl OrgRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
