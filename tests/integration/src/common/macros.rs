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
            let _ = tracing_subscriber::fmt()
                .with_env_filter("warn")
                .with_test_writer()
                .try_init();

            let database_url = match std::env::var("DATABASE_URL") {
                Ok(url) => url,
                Err(_) => {
                    eprintln!("DATABASE_URL not set, skipping integration test");
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
            let admin_opts: sqlx::postgres::PgConnectOptions = database_url
                .parse()
                .expect("parse DATABASE_URL");
            let test_opts = admin_opts.database(&test_db);

            let pool = sqlx::PgPool::connect_with(test_opts)
                .await
                .expect("connect to test database");

            sqlx::migrate!("../../migrations")
                .run(&pool)
                .await
                .expect("run migrations");

            let $server = $crate::common::server::TestServer::start(pool).await;

            // Run the test
            $body

            // Cleanup: close pool and drop database
            $server.pool.close().await;

            let admin_pool = sqlx::PgPool::connect(&database_url)
                .await
                .expect("reconnect to admin database");
            sqlx::query(&format!("DROP DATABASE \"{test_db}\""))
                .execute(&admin_pool)
                .await
                .expect("drop test database");
            admin_pool.close().await;
        }
    };
}
