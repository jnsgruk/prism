use sqlx::PgPool;

/// Repository for the `activity` schema: contributions, ingestion watermarks,
/// `ETag` cache, and ingestion runs.
#[derive(Clone)]
pub struct ActivityRepo {
    pool: PgPool,
}

impl ActivityRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
