use ps_core::repo::Repos;
use sqlx::PgPool;
use sqlx::postgres::PgConnectOptions;

/// An isolated test database backed by a real PostgreSQL instance.
///
/// Integration tests must be run via `cargo nextest run`, which executes the
/// `setup-test-db` binary to start a pgvector container and prepare a
/// pre-migrated template database. Each `TestDb` creates a unique database
/// from that template (near-instant filesystem copy) and drops it on teardown.
pub struct TestDb {
    pub pool: PgPool,
    test_db: String,
    database_url: String,
}

impl TestDb {
    pub async fn new() -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("warn")
            .with_test_writer()
            .try_init();

        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            panic!(
                "DATABASE_URL is not set. Integration tests must be run via \
                 `cargo nextest run` which starts a database automatically. \
                 See tests/integration/src/bin/setup-test-db.rs for details."
            )
        });

        let test_db = format!("ps_test_{}", uuid::Uuid::now_v7().simple());
        let admin_pool = PgPool::connect(&database_url)
            .await
            .expect("connect to admin database");

        let template = std::env::var("PS_TEST_TEMPLATE").ok();
        let create_sql = match &template {
            Some(tmpl) => format!("CREATE DATABASE \"{test_db}\" TEMPLATE \"{tmpl}\""),
            None => format!("CREATE DATABASE \"{test_db}\""),
        };
        sqlx::query(&create_sql)
            .execute(&admin_pool)
            .await
            .expect("create test database");
        admin_pool.close().await;

        let admin_opts: PgConnectOptions = database_url.parse().expect("parse DATABASE_URL");
        let test_opts = admin_opts.database(&test_db);

        let pool = PgPool::connect_with(test_opts)
            .await
            .expect("connect to test database");

        // Only run migrations if we didn't use a template (template already
        // has them applied).
        if template.is_none() {
            sqlx::migrate!("../../migrations")
                .run(&pool)
                .await
                .expect("run migrations");
        }

        Self {
            pool,
            test_db,
            database_url,
        }
    }

    pub async fn teardown(self) {
        self.pool.close().await;

        let admin_pool = PgPool::connect(&self.database_url)
            .await
            .expect("reconnect to admin database");
        sqlx::query(&format!("DROP DATABASE \"{}\" WITH (FORCE)", self.test_db))
            .execute(&admin_pool)
            .await
            .expect("drop test database");
        admin_pool.close().await;
    }
}

/// Test context for repository-layer tests against real PostgreSQL.
///
/// Provides a `Repos` instance and raw `PgPool` without a gRPC server.
pub struct RepoTestContext {
    pub repos: Repos,
    pub pool: PgPool,
    db: TestDb,
}

impl RepoTestContext {
    pub async fn new() -> Self {
        let db = TestDb::new().await;
        let repos = Repos::new(db.pool.clone());
        Self {
            repos,
            pool: db.pool.clone(),
            db,
        }
    }

    pub async fn teardown(self) {
        self.db.teardown().await;
    }
}
