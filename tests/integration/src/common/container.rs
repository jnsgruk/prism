//! Shared testcontainers PostgreSQL instance for integration tests.
//!
//! # How it works
//!
//! A single `pgvector/pgvector:pg17` container is started lazily on first use
//! and shared across all tests in the process. Each test creates its own
//! isolated database within the container, runs migrations, and drops the
//! database on teardown.
//!
//! The container is automatically removed when the test process exits (via
//! `libc::atexit`). This is necessary because Rust does not call destructors
//! on static variables, so the normal `ContainerAsync` `Drop` cleanup never
//! fires.
//!
//! # Configuration
//!
//! | Env var        | Effect |
//! |----------------|--------|
//! | `DATABASE_URL` | Skip the container entirely — connect to this PostgreSQL instance instead. Useful for CI with a sidecar DB or local development with a running Postgres. |
//! | *(unset)*      | Automatically start a pgvector Docker container via testcontainers-rs. Requires a Docker-compatible runtime (Docker Desktop, Podman, Colima, etc.). |

use std::sync::OnceLock;

use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tokio::sync::OnceCell;

/// The pgvector Docker image — includes PostgreSQL 17 + the `vector` extension
/// needed for embedding tests.
const PGVECTOR_IMAGE: &str = "pgvector/pgvector";
const PGVECTOR_TAG: &str = "pg17";

/// A lazily-started container that lives for the entire test process.
struct SharedContainer {
    /// Kept alive so the Docker container isn't removed mid-test.
    /// Actual cleanup happens via the `atexit` handler below.
    _container: ContainerAsync<Postgres>,
    /// Admin connection URL (connects to the default `postgres` database).
    database_url: String,
}

// SAFETY: SharedContainer is only written once (via OnceCell) and read
// thereafter. ContainerAsync is Send + Sync.
unsafe impl Send for SharedContainer {}
unsafe impl Sync for SharedContainer {}

/// Process-wide singleton. Initialised at most once per `cargo test` invocation.
static CONTAINER: OnceCell<Option<SharedContainer>> = OnceCell::const_new();

/// Container ID for cleanup on process exit. Written once during container
/// startup, read once in the `atexit` handler.
static CONTAINER_ID: OnceLock<String> = OnceLock::new();

/// Return a PostgreSQL connection URL for integration tests.
///
/// 1. If `DATABASE_URL` is set, returns it directly (no container started).
/// 2. Otherwise, starts (or reuses) a pgvector Docker container and returns
///    a connection URL pointing at it.
/// 3. Returns `None` only when Docker is unavailable *and* `DATABASE_URL` is
///    unset — callers should skip the test in this case.
pub async fn database_url() -> Option<String> {
    // Fast path: explicit DATABASE_URL takes precedence.
    if let Ok(url) = std::env::var("DATABASE_URL") {
        return Some(url);
    }

    let maybe = CONTAINER
        .get_or_init(|| async {
            match start_pgvector_container().await {
                Ok(shared) => Some(shared),
                Err(err) => {
                    eprintln!("Could not start pgvector container: {err}");
                    eprintln!(
                        "Set DATABASE_URL to use an external Postgres, \
                         or ensure Docker is running."
                    );
                    None
                }
            }
        })
        .await;

    maybe.as_ref().map(|s| s.database_url.clone())
}

/// Start a fresh pgvector container and register an `atexit` cleanup handler.
async fn start_pgvector_container() -> Result<SharedContainer, Box<dyn std::error::Error>> {
    let container = Postgres::default()
        .with_name(PGVECTOR_IMAGE)
        .with_tag(PGVECTOR_TAG)
        .start()
        .await?;

    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let database_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    // Verify connectivity before returning.
    let pool = sqlx::PgPool::connect(&database_url).await?;
    pool.close().await;

    // Register cleanup: Rust statics never call Drop, so we use libc::atexit
    // to force-remove the container when the test process exits.
    let container_id = container.id().to_string();
    CONTAINER_ID.set(container_id).ok();

    // SAFETY: atexit handlers run during normal process exit. The function
    // is extern "C" with no Rust-specific state dependencies.
    unsafe {
        libc::atexit(remove_container_on_exit);
    }

    Ok(SharedContainer {
        _container: container,
        database_url,
    })
}

/// `atexit` callback — force-removes the Docker container on process exit.
///
/// Uses `docker rm -f` via a subprocess because we're outside the tokio
/// runtime at this point (no async available). Ignores errors — if the
/// container is already gone or Docker is unreachable, that's fine.
extern "C" fn remove_container_on_exit() {
    if let Some(id) = CONTAINER_ID.get() {
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", id])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}
