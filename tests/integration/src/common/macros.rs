/// Set up an isolated test database and return `(pool, test_db_name, database_url)`.
///
/// Resolves the PostgreSQL connection in this order:
/// 1. `DATABASE_URL` env var → use that server directly
/// 2. Otherwise → start a shared pgvector Docker container via testcontainers
/// 3. If both fail → skip the test with a warning
///
/// Each test gets its own uniquely-named database with migrations applied.
#[macro_export]
macro_rules! setup_test_db {
    () => {{
        let _ = tracing_subscriber::fmt()
            .with_env_filter("warn")
            .with_test_writer()
            .try_init();

        let database_url = match $crate::common::container::database_url().await {
            Some(url) => url,
            None => {
                eprintln!("No database available, skipping integration test");
                return;
            }
        };

        // Create a unique database for this test
        let test_db = format!("ps_test_{}", uuid::Uuid::now_v7().simple());
        let admin_pool = sqlx::PgPool::connect(&database_url)
            .await
            .expect("connect to admin database");

        sqlx::query(&format!("CREATE DATABASE \"{test_db}\""))
            .execute(&admin_pool)
            .await
            .expect("create test database");
        admin_pool.close().await;

        // Connect to the test database using PgConnectOptions
        let admin_opts: sqlx::postgres::PgConnectOptions =
            database_url.parse().expect("parse DATABASE_URL");
        let test_opts = admin_opts.database(&test_db);

        let pool = sqlx::PgPool::connect_with(test_opts)
            .await
            .expect("connect to test database");

        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("run migrations");

        (pool, test_db, database_url)
    }};
}

/// Tear down the isolated test database.
#[macro_export]
macro_rules! teardown_test_db {
    ($pool:expr, $test_db:expr, $database_url:expr) => {{
        $pool.close().await;

        let admin_pool = sqlx::PgPool::connect(&$database_url)
            .await
            .expect("reconnect to admin database");
        sqlx::query(&format!("DROP DATABASE \"{}\" WITH (FORCE)", $test_db))
            .execute(&admin_pool)
            .await
            .expect("drop test database");
        admin_pool.close().await;
    }};
}

/// Define an integration test that starts a real PG database and gRPC server.
///
/// Usage:
/// ```ignore
/// define_api_test!(test_name, |server: TestServer| async move {
///     // test body with access to server.channel, server.pool
/// });
/// ```
#[macro_export]
macro_rules! define_api_test {
    ($name:ident, |$server:ident| async move $body:block) => {
        #[tokio::test]
        async fn $name() {
            let (pool, test_db, database_url) = $crate::setup_test_db!();

            let $server = $crate::common::server::TestServer::start(pool).await;

            // Run the test
            $body

            // Cleanup
            $crate::teardown_test_db!($server.pool, test_db, database_url);
        }
    };
}

/// Define a source-adapter integration test with wiremock + real PostgreSQL.
///
/// Sets up a wiremock `MockServer`, a `PgPool` with migrations, a `Repos` instance,
/// and provides a helper to build an `IngestionContext` pointing at the mock server.
///
/// Usage:
/// ```ignore
/// define_source_test!(test_name, |ctx: SourceTestContext| async move {
///     // ctx.mock_server — wiremock MockServer for mounting mock responses
///     // ctx.repos — Repos for direct DB access
///     // ctx.pool — PgPool for raw queries
///     // ctx.build_ingestion_ctx(platform, settings) — build IngestionContext
/// });
/// ```
#[macro_export]
macro_rules! define_source_test {
    ($name:ident, |$ctx:ident| async move $body:block) => {
        #[tokio::test]
        async fn $name() {
            let (pool, test_db, database_url) = $crate::setup_test_db!();
            let repos = ps_core::repo::Repos::new(pool.clone());
            let mock_server = wiremock::MockServer::start().await;

            let $ctx = $crate::common::wiremock_helpers::SourceTestContext {
                mock_server,
                repos,
                pool: pool.clone(),
            };

            // Run the test
            $body

            // Cleanup
            $crate::teardown_test_db!(pool, test_db, database_url);
        }
    };
}

/// Define a repository-layer integration test against real PostgreSQL.
///
/// Lighter than `define_api_test!` — no gRPC server, just a `PgPool` + `Repos`.
///
/// Usage:
/// ```ignore
/// define_repo_test!(test_name, |repos: Repos, pool: PgPool| async move {
///     // test body with direct repo access
/// });
/// ```
#[macro_export]
macro_rules! define_repo_test {
    ($name:ident, |$repos:ident, $pool:ident| async move $body:block) => {
        #[tokio::test]
        async fn $name() {
            let ($pool, test_db, database_url) = $crate::setup_test_db!();

            let $repos = ps_core::repo::Repos::new($pool.clone());

            // Run the test
            $body

            // Cleanup
            $crate::teardown_test_db!($pool, test_db, database_url);
        }
    };
}
