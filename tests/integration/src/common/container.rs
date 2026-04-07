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
//! # Docker socket resolution
//!
//! testcontainers (via bollard) defaults to `/var/run/docker.sock`, which may
//! be a different daemon than what the `docker` CLI uses (e.g. Docker Desktop
//! uses `~/.docker/desktop/docker.sock`). This module resolves the active
//! Docker CLI context and sets `DOCKER_HOST` accordingly before starting
//! any containers.
//!
//! # Configuration
//!
//! | Env var        | Effect |
//! |----------------|--------|
//! | `DATABASE_URL` | Skip the container entirely — connect to this PostgreSQL instance instead. Useful for CI with a sidecar DB or local development with a running Postgres. |
//! | `DOCKER_HOST`  | Override the Docker socket testcontainers connects to. |
//! | *(both unset)* | Automatically start a pgvector Docker container via testcontainers-rs. Requires a Docker-compatible runtime. |

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

/// Ensure `DOCKER_HOST` points at the same Docker daemon the CLI uses.
///
/// When Docker Desktop is installed, the CLI uses the `desktop-linux` context
/// (`~/.docker/desktop/docker.sock`) while the raw engine socket lives at
/// `/var/run/docker.sock`. testcontainers picks up the raw socket first,
/// creating containers on a daemon whose networking doesn't interop with
/// Docker Desktop — ports are mapped but unreachable from the host.
///
/// This reads `~/.docker/config.json` to detect a non-default context, then
/// asks `docker context inspect` for the endpoint URL.
fn ensure_docker_host() {
    if std::env::var("DOCKER_HOST").is_ok() {
        return;
    }

    let Some(context_name) = active_docker_context() else {
        return;
    };
    if context_name == "default" {
        return;
    }

    // Ask the Docker CLI for the endpoint — it already knows how to resolve
    // context metadata (hashed directory names, meta.json, etc.).
    if let Some(endpoint) = docker_context_endpoint(&context_name) {
        // SAFETY: called once during single-threaded container init (guarded
        // by OnceCell) before any other threads read DOCKER_HOST.
        unsafe { std::env::set_var("DOCKER_HOST", &endpoint) };
    }
}

/// Read `currentContext` from `~/.docker/config.json`.
fn active_docker_context() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let config_path = format!("{home}/.docker/config.json");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config_path).ok()?).ok()?;
    config.get("currentContext")?.as_str().map(String::from)
}

/// Ask `docker context inspect <name>` for the Docker endpoint URL.
fn docker_context_endpoint(context_name: &str) -> Option<String> {
    let output = std::process::Command::new("docker")
        .args([
            "context",
            "inspect",
            context_name,
            "--format",
            "{{.Endpoints.docker.Host}}",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let endpoint = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if endpoint.is_empty() {
        return None;
    }
    Some(endpoint)
}

/// Start a fresh pgvector container and register an `atexit` cleanup handler.
async fn start_pgvector_container() -> Result<SharedContainer, Box<dyn std::error::Error>> {
    ensure_docker_host();

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

/// `atexit` callback — stops and removes the Docker container on process exit.
///
/// Uses `docker stop` followed by `docker rm` via subprocesses because we're
/// outside the tokio runtime at this point (no async available). We explicitly
/// stop before removing so that the Docker daemon properly tears down the
/// container's network stack — including the `docker-proxy` port-forwarding
/// processes. A bare `docker rm -f` sends SIGKILL and can skip proxy cleanup,
/// leaving orphaned `docker-proxy` listeners on ephemeral ports.
extern "C" fn remove_container_on_exit() {
    if let Some(id) = CONTAINER_ID.get() {
        let _ = std::process::Command::new("docker")
            .args(["stop", "-t", "5", id])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", id])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}
