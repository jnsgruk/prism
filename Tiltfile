allow_k8s_contexts("docker-desktop")

# ---------------------------------------------------------------------------
# Envoy Gateway — must come first so CRDs are available for base manifests
# ---------------------------------------------------------------------------
# Inflate the helm chart so the CRD file is available on disk. The chart
# directory name includes the version from kustomization.yaml, so we locate
# the CRD file dynamically to avoid hardcoding it.
local("kubectl kustomize --enable-helm k8s/gateway > /dev/null")
crds = str(local(
    "find k8s/gateway/charts -name gatewayapi-crds.yaml -path '*/crds/*' | head -1",
    quiet=True,
)).strip()

# Pre-apply CRDs so Tilt recognises Gateway API resources.
local("kubectl apply --server-side -f " + crds)
k8s_yaml(kustomize("k8s/gateway", flags=["--enable-helm"]))

# ---------------------------------------------------------------------------
# Base manifests (namespace: prism)
# ---------------------------------------------------------------------------
k8s_yaml(kustomize("k8s/base"))

# ---------------------------------------------------------------------------
# Rust services — build inside Docker with BuildKit cache mounts
# ---------------------------------------------------------------------------

# Each docker_build uses only= to scope which file changes trigger a rebuild.
# The Dockerfile stubs out workspace members whose source isn't in the context,
# so cargo can resolve the workspace without needing every crate's source.

# Workspace metadata — included in every build, changes trigger all services.
_meta = ["Cargo.toml", "Cargo.lock", ".cargo", ".sqlx", "crates/Dockerfile"]

# Every member's Cargo.toml — needed for workspace resolution even when the
# member's source is stubbed out. Rarely change so the rebuild cost is fine.
_tomls = [
    "crates/ps-core/Cargo.toml",
    "crates/ps-proto/Cargo.toml",
    "crates/ps-server/Cargo.toml",
    "crates/ps-ingestion/Cargo.toml",
    "crates/ps-metrics/Cargo.toml",
    "crates/ps-migrate/Cargo.toml",
    "crates/psctl/Cargo.toml",
    "tests/integration/Cargo.toml",
]

# Shared crates — changes here rebuild any service that depends on them.
_shared = ["crates/ps-core", "crates/ps-proto", "proto"]

docker_build(
    "prism/ps-server",
    ".",
    dockerfile="crates/Dockerfile",
    target="ps-server-dev",
    build_args={"PROFILE": "debug", "BIN": "ps-server"},
    only=_meta + _tomls + _shared + ["crates/ps-server", "crates/ps-metrics"],
)

docker_build(
    "prism/ps-ingestion",
    ".",
    dockerfile="crates/Dockerfile",
    target="ps-ingestion-dev",
    build_args={"PROFILE": "debug", "BIN": "ps-ingestion"},
    only=_meta + _tomls + _shared + ["crates/ps-ingestion"],
)

docker_build(
    "prism/ps-migrate",
    ".",
    dockerfile="crates/Dockerfile",
    target="ps-migrate-dev",
    build_args={"PROFILE": "debug", "BIN": "ps-migrate"},
    only=_meta + _tomls + ["crates/ps-migrate", "migrations"],
)

# ---------------------------------------------------------------------------
# Frontend — Next.js standalone build
# ---------------------------------------------------------------------------
docker_build(
    "prism/ps-frontend",
    "frontend",
    dockerfile="frontend/Dockerfile",
    target="ps-frontend-dev",
)

# ---------------------------------------------------------------------------
# Resource configuration
# ---------------------------------------------------------------------------
k8s_resource("ps-migrate", resource_deps=["postgres"], labels=["prism"])
k8s_resource("ps-server", resource_deps=["ps-migrate"], port_forwards=["8080:8080"],labels=["prism"],)
k8s_resource("ps-ingestion", resource_deps=["ps-migrate"], port_forwards=["9080:9080"],labels=["prism"],)
k8s_resource("ps-frontend", resource_deps=["ps-server"], port_forwards=["3000:3000"], labels=["prism"])

k8s_resource(workload="envoy-gateway", labels=["gateway"])
k8s_resource(workload="eg-gateway-helm-certgen", labels=["gateway"])

k8s_resource("postgres", port_forwards=["5432:5432"], labels=["infra"],)
k8s_resource("restate",  port_forwards=["9070:9070"], labels=["infra"])
