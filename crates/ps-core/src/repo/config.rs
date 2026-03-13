use sqlx::PgPool;

/// Repository for the `config` schema: source configurations and encrypted secrets.
#[derive(Clone)]
pub struct ConfigRepo {
    pool: PgPool,
}

impl ConfigRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
