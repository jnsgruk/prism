//! Nextest setup script: start a pgvector container and prepare a template DB.
//!
//! Called by nextest before integration tests run. Writes `DATABASE_URL` and
//! `PS_TEST_TEMPLATE` to `$NEXTEST_ENV` so every test process inherits them.
//!
//! The template database has all migrations pre-applied, so tests can use
//! `CREATE DATABASE ... TEMPLATE` instead of running migrations individually.
//!
//! Container lifecycle is managed via Docker CLI (not testcontainers) so the
//! container survives after this process exits. A background watchdog process
//! monitors the nextest runner and stops the container when it exits.

use std::io::Write;
use std::process::Command;

const CONTAINER_NAME: &str = "ps-nextest-postgres";
const IMAGE: &str = "pgvector/pgvector:pg17";
const TEMPLATE_DB: &str = "ps_template";

#[tokio::main]
async fn main() {
    // Re-invoked as watchdog — just monitor and clean up.
    if let Some(pid) = watchdog_pid_from_args() {
        run_watchdog(pid);
        return;
    }

    let nextest_env =
        std::env::var("NEXTEST_ENV").expect("NEXTEST_ENV must be set (run via nextest)");

    // If DATABASE_URL is already set externally (CI, custom setup), pass it through.
    if let Ok(url) = std::env::var("DATABASE_URL") {
        write_env(&nextest_env, &url, None);
        return;
    }

    let port = ensure_container();
    let database_url = format!("postgres://postgres:postgres@localhost:{port}/postgres");

    // Wait for postgres to accept connections (container may still be initializing).
    let pool = wait_for_postgres(&database_url).await;

    // Drop + recreate the template to pick up any new migrations.
    sqlx::query(
        "DO $$ BEGIN \
            IF EXISTS (SELECT 1 FROM pg_database WHERE datname = 'ps_template') THEN \
                ALTER DATABASE ps_template IS_TEMPLATE = false; \
            END IF; \
         END $$",
    )
    .execute(&pool)
    .await
    .expect("clear template flag");

    sqlx::query("DROP DATABASE IF EXISTS ps_template WITH (FORCE)")
        .execute(&pool)
        .await
        .expect("drop old template");

    sqlx::query("CREATE DATABASE ps_template")
        .execute(&pool)
        .await
        .expect("create template database");

    pool.close().await;

    // Run migrations on the template database.
    let template_url = format!("postgres://postgres:postgres@localhost:{port}/{TEMPLATE_DB}");
    let template_pool = sqlx::PgPool::connect(&template_url)
        .await
        .expect("connect to template database");

    sqlx::migrate!("../../migrations")
        .run(&template_pool)
        .await
        .expect("run migrations on template");

    template_pool.close().await;

    // Mark as template so CREATE DATABASE ... TEMPLATE works.
    let pool = sqlx::PgPool::connect(&database_url)
        .await
        .expect("reconnect to postgres");

    sqlx::query("ALTER DATABASE ps_template IS_TEMPLATE = true")
        .execute(&pool)
        .await
        .expect("mark as template");

    pool.close().await;

    write_env(&nextest_env, &database_url, Some(TEMPLATE_DB));

    // Spawn a watchdog that stops the container when nextest exits.
    spawn_watchdog();
}

// ---------------------------------------------------------------------------
// Container management
// ---------------------------------------------------------------------------

/// Ensure a postgres container is running and return its host port.
///
/// Reuses an existing container if already running; starts a fresh one otherwise.
/// Uses Docker CLI directly (not testcontainers) so the container outlives this
/// process — nextest tests connect to it, and the watchdog cleans it up.
fn ensure_container() -> u16 {
    // Check if already running.
    if let Some(port) = container_port() {
        return port;
    }

    // Remove any stopped leftover.
    let _ = Command::new("docker")
        .args(["rm", "-f", CONTAINER_NAME])
        .output();

    // Start fresh.
    let status = Command::new("docker")
        .args([
            "run",
            "-d",
            "--name",
            CONTAINER_NAME,
            "-e",
            "POSTGRES_PASSWORD=postgres",
            "-p",
            "0:5432",
            IMAGE,
        ])
        .output()
        .expect("failed to run docker");

    assert!(
        status.status.success(),
        "docker run failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );

    // Poll until port is mapped (container fully started).
    for _ in 0..30 {
        if let Some(port) = container_port() {
            return port;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    panic!("container started but port not mapped after 15s");
}

/// Get the host port for the container's 5432/tcp mapping, or None.
fn container_port() -> Option<u16> {
    let output = Command::new("docker")
        .args(["port", CONTAINER_NAME, "5432/tcp"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // Output is like "0.0.0.0:55259\n[::]:55259\n"
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().next()?.rsplit(':').next()?.parse().ok()
}

/// Poll until postgres accepts connections (up to 30s).
async fn wait_for_postgres(url: &str) -> sqlx::PgPool {
    for i in 0..30 {
        match sqlx::pool::PoolOptions::<sqlx::Postgres>::new()
            .max_connections(2)
            .acquire_timeout(std::time::Duration::from_secs(1))
            .connect(url)
            .await
        {
            Ok(pool) => return pool,
            Err(_) if i < 29 => tokio::time::sleep(std::time::Duration::from_secs(1)).await,
            Err(e) => panic!("postgres not ready after 30s: {e}"),
        }
    }
    unreachable!()
}

// ---------------------------------------------------------------------------
// Watchdog — monitors the nextest runner and stops the container when it exits
// ---------------------------------------------------------------------------

/// Spawn a detached copy of ourselves with `--watchdog <nextest-pid>`.
///
/// The watchdog runs as a fully independent process (double-forked, new session)
/// so it outlives both this setup binary and the nextest runner. It polls for
/// the nextest PID and stops the container once nextest is gone.
fn spawn_watchdog() {
    let nextest_pid = parent_pid();
    let self_exe = std::env::current_exe().expect("resolve own binary path");

    // Spawn detached — stdin/stdout/stderr all null so nextest doesn't wait for
    // us, and the process is fully independent. We intentionally don't wait on
    // this child — it outlives us by design.
    #[allow(clippy::zombie_processes)]
    Command::new(self_exe)
        .args(["--watchdog", &nextest_pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn watchdog process");
}

/// Check if we were invoked as `--watchdog <pid>`.
fn watchdog_pid_from_args() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--watchdog" {
        args[2].parse().ok()
    } else {
        None
    }
}

/// Monitor a PID and stop the container when it disappears.
fn run_watchdog(pid: u32) {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(2));

        // kill(pid, 0) checks if the process exists without sending a signal.
        // SAFETY: signal 0 is harmless — it's a standard existence check.
        let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
        if !alive {
            let _ = Command::new("docker")
                .args(["stop", "-t", "5", CONTAINER_NAME])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            break;
        }
    }
}

/// Get the parent PID (the nextest runner) via procfs.
fn parent_pid() -> u32 {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("read /proc/self/stat");
    // Format: "pid (comm) state ppid ..."
    // The comm field can contain spaces/parens, so find the last ')' first.
    let after_comm = &stat[stat.rfind(')').expect("parse /proc/self/stat") + 2..];
    let ppid_str = after_comm
        .split_whitespace()
        .nth(1) // state is field 0 after comm, ppid is field 1
        .expect("extract ppid from /proc/self/stat");
    ppid_str.parse().expect("parse ppid")
}

// ---------------------------------------------------------------------------
// Env file output
// ---------------------------------------------------------------------------

fn write_env(path: &str, database_url: &str, template: Option<&str>) {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open NEXTEST_ENV file");

    writeln!(f, "DATABASE_URL={database_url}").expect("write DATABASE_URL");
    if let Some(tmpl) = template {
        writeln!(f, "PS_TEST_TEMPLATE={tmpl}").expect("write PS_TEST_TEMPLATE");
    }
}
