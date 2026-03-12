allow_k8s_contexts("docker-desktop")

# ---------------------------------------------------------------------------
# Secrets & infrastructure
# ---------------------------------------------------------------------------
k8s_yaml("k8s/secrets.yaml")
k8s_yaml("k8s/base/postgres.yaml")
k8s_yaml("k8s/base/restate.yaml")

# ---------------------------------------------------------------------------
# Migrations (depends on postgres)
# ---------------------------------------------------------------------------
k8s_yaml("k8s/base/ps-migrate.yaml")
k8s_resource("ps-migrate", resource_deps=["postgres"])

# ---------------------------------------------------------------------------
# Rust services — cargo watch + binary sync
# ---------------------------------------------------------------------------

# Build dev image once (used as base for live_update)
docker_build(
    "prism/ps-server",
    ".",
    dockerfile="Dockerfile.rust.dev",
    entrypoint=["ps-server"],
    live_update=[
        sync("target/debug/ps-server", "/usr/local/bin/ps-server"),
    ],
)

docker_build(
    "prism/ps-ingestion",
    ".",
    dockerfile="Dockerfile.rust.dev",
    entrypoint=["ps-ingestion"],
    live_update=[
        sync("target/debug/ps-ingestion", "/usr/local/bin/ps-ingestion"),
    ],
)

docker_build(
    "prism/ps-migrate",
    ".",
    dockerfile="Dockerfile.rust.dev",
    entrypoint=["ps-migrate"],
    live_update=[
        sync("target/debug/ps-migrate", "/usr/local/bin/ps-migrate"),
    ],
)

# Deploy services
k8s_yaml("k8s/base/ps-server.yaml")
k8s_resource(
    "ps-server",
    resource_deps=["ps-migrate"],
    port_forwards=["8080:8080"],
)

k8s_yaml("k8s/base/ps-ingestion.yaml")
k8s_resource(
    "ps-ingestion",
    resource_deps=["ps-migrate"],
    port_forwards=["9080:9080"],
)

# ---------------------------------------------------------------------------
# Envoy Gateway (optional — apply manually if CRDs are installed)
# ---------------------------------------------------------------------------
# k8s_yaml("k8s/gateway/gateway.yaml")

# ---------------------------------------------------------------------------
# Local cargo watch — rebuild on source changes
# ---------------------------------------------------------------------------
local_resource(
    "cargo-watch",
    serve_cmd="cargo watch -x 'build --bin ps-server --bin ps-ingestion --bin ps-migrate'",
    deps=["crates/", "Cargo.toml", "Cargo.lock"],
)

# ---------------------------------------------------------------------------
# Port forwards for convenience
# ---------------------------------------------------------------------------
k8s_resource("postgres", port_forwards=["5432:5432"])
k8s_resource("restate", port_forwards=["9070:9070"])
