# syntax=docker/dockerfile:1

# Multi-stage Dockerfile for Rust services.
# BuildKit cache mounts on target/ and the cargo registry give fast incremental
# rebuilds without cargo-chef.
#
# Build args:
#   PROFILE  — "debug" (Tilt) or "release" (CI/prod, default)
#   BIN      — binary to build: ps-server, ps-ingestion, or ps-migrate
#
# Dev targets (Tilt):
#   docker build -f crates/Dockerfile --target ps-server-dev --build-arg PROFILE=debug --build-arg BIN=ps-server .
#
# Prod targets (CI):
#   docker build -f crates/Dockerfile --target ps-server --build-arg BIN=ps-server .

ARG PROFILE=release

# ---------------------------------------------------------------------------
# Builder — toolchain, system deps, compile
# ---------------------------------------------------------------------------
FROM rust:1.94-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    libprotobuf-dev \
    pkg-config \
    libssl-dev \
    clang \
    mold \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY . .

# Stub out workspace members whose source is not in the build context.
# Tilt's only= scopes each service to its own crates, so other members only
# have their Cargo.toml present. Cargo needs every member to have at least a
# source file for workspace resolution.
RUN find crates tests -name Cargo.toml -exec sh -c '\
      dir=$(dirname "$1"); \
      if [ ! -f "$dir/src/main.rs" ] && [ ! -f "$dir/src/lib.rs" ]; then \
        mkdir -p "$dir/src" && touch "$dir/src/lib.rs"; \
      fi' _ {} \;

ENV SQLX_OFFLINE=true
ARG PROFILE
ARG BIN
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build \
        $([ "$PROFILE" = "release" ] && echo "--release") \
        --bin ${BIN} \
    && mkdir -p /out \
    && cp target/$([ "$PROFILE" = "release" ] && echo "release" || echo "debug")/${BIN} /out/${BIN}

# ---------------------------------------------------------------------------
# Dev runtime — Ubuntu base for Tilt
# ---------------------------------------------------------------------------
FROM ubuntu:24.04 AS dev-runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3t64 \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --shell /bin/bash prism

FROM dev-runtime AS ps-server-dev
COPY --from=builder /out/ps-server /usr/local/bin/ps-server
USER prism
ENTRYPOINT ["ps-server"]

FROM dev-runtime AS ps-ingestion-dev
COPY --from=builder /out/ps-ingestion /usr/local/bin/ps-ingestion
USER prism
ENTRYPOINT ["ps-ingestion"]

FROM dev-runtime AS ps-migrate-dev
COPY --from=builder /out/ps-migrate /usr/local/bin/ps-migrate
USER prism
ENTRYPOINT ["ps-migrate"]

# ---------------------------------------------------------------------------
# Chisel — minimal Ubuntu rootfs for prod
# ---------------------------------------------------------------------------
FROM ubuntu:24.04 AS chisel
RUN apt-get update && apt-get install -y chisel \
    && rm -rf /var/lib/apt/lists/*
RUN mkdir /rootfs && chisel cut --root /rootfs \
    base-files_base \
    base-files_release-info \
    ca-certificates_data \
    libgcc-s1_libs \
    libc6_libs \
    libssl3t64_libs \
    openssl_bins

# ---------------------------------------------------------------------------
# Prod runtime targets
# ---------------------------------------------------------------------------
FROM scratch AS ps-server
COPY --from=chisel /rootfs /
COPY --from=builder /out/ps-server /usr/local/bin/ps-server
USER 65534:65534
ENTRYPOINT ["ps-server"]

FROM scratch AS ps-ingestion
COPY --from=chisel /rootfs /
COPY --from=builder /out/ps-ingestion /usr/local/bin/ps-ingestion
USER 65534:65534
ENTRYPOINT ["ps-ingestion"]

FROM scratch AS ps-migrate
COPY --from=chisel /rootfs /
COPY --from=builder /out/ps-migrate /usr/local/bin/ps-migrate
USER 65534:65534
ENTRYPOINT ["ps-migrate"]
