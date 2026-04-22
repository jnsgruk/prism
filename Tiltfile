allow_k8s_contexts("docker-desktop")
allow_k8s_contexts("k8s")

default_registry("localhost:30500")

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
    "crates/ps-workers/Cargo.toml",
    "crates/ps-metrics/Cargo.toml",
    "crates/ps-migrate/Cargo.toml",
    "crates/ps-agent/Cargo.toml",
    "crates/ps-mcp/Cargo.toml",
    "crates/ps-backup/Cargo.toml",
    "crates/psctl/Cargo.toml",
    "crates/ps-reasoning/Cargo.toml",
    "tests/integration/Cargo.toml",
]

# Shared crates — changes here rebuild any service that depends on them.
_shared = ["crates/ps-core", "crates/ps-proto", "crates/ps-reasoning", "crates/ps-agent", "proto"]

docker_build(
    "prism/ps-server",
    ".",
    dockerfile="crates/Dockerfile",
    target="ps-server-dev",
    build_args={"PROFILE": "debug", "BIN": "ps-server"},
    only=_meta + _tomls + _shared + ["crates/ps-server", "crates/ps-metrics"],
)

docker_build(
    "prism/ps-workers",
    ".",
    dockerfile="crates/Dockerfile",
    target="ps-workers-dev",
    build_args={"PROFILE": "debug", "BIN": "ps-workers"},
    only=_meta + _tomls + _shared + ["crates/ps-workers", "crates/ps-metrics"],
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
# Agent container — ps-mcp binary + OpenCode + system tools
# ---------------------------------------------------------------------------
# Agent pods are created dynamically by ContainerManager, not from YAML that
# Tilt can rewrite. We build with $EXPECTED_REF (which Tilt pushes to the
# cluster registry) and additionally tag+push the stable
# localhost:30500/prism_prism-agent:latest ref so dynamically-created pods
# can pull from the in-cluster registry without needing a dynamic tag.
# ps-workers reads AGENT_IMAGE=localhost:30500/prism_prism-agent:latest.
custom_build(
    "prism/prism-agent",
    "docker build -t $EXPECTED_REF" +
    " -t prism/prism-agent:latest" +
    " -t localhost:30500/prism_prism-agent:latest" +
    " --target prism-agent-dev --build-arg PROFILE=debug" +
    " -f crates/ps-agent/agent-container/Dockerfile . " +
    " && docker push localhost:30500/prism_prism-agent:latest",
    deps=_meta + _tomls + _shared + ["crates/ps-mcp", "crates/ps-agent/agent-container"],
    skips_local_docker=False,
)

# ---------------------------------------------------------------------------
# Backup container — pg_dump + ps-backup binary
# ---------------------------------------------------------------------------
# Backup Jobs are created dynamically by ps-server, same pattern as agent pods.
custom_build(
    "prism/ps-backup",
    "docker build -t $EXPECTED_REF" +
    " -t prism/ps-backup:latest" +
    " -t localhost:30500/prism_ps-backup:latest" +
    " --target ps-backup-dev --build-arg PROFILE=debug --build-arg BIN=ps-backup" +
    " -f crates/Dockerfile . " +
    " && docker push localhost:30500/prism_ps-backup:latest",
    deps=_meta + _tomls + ["crates/ps-backup", "crates/ps-core"],
    skips_local_docker=False,
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
k8s_resource("ps-server", port_forwards=["8080:8080"], resource_deps=["ps-migrate"], labels=["prism"],)
k8s_resource("ps-workers", resource_deps=["ps-migrate"], labels=["prism"],)
k8s_resource("ps-frontend", resource_deps=["ps-server"], labels=["prism"])

k8s_resource(workload="envoy-gateway", labels=["gateway"])
k8s_resource(workload="eg-gateway-helm-certgen", labels=["gateway"])

k8s_resource("postgres", port_forwards=["5432:5432"], labels=["infra"],)
k8s_resource("restate",  port_forwards=["9070:9070"], labels=["infra"])
k8s_resource("prism-agent-image-builder", labels=["agent"])
k8s_resource("ps-backup-image-builder", labels=["backup"])
