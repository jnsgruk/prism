# syntax=docker/dockerfile:1

# Multi-stage Dockerfile for the Prism agent container.
# Stage 1: Build the ps-mcp binary using the same Rust builder as other crates.
# Stage 2: Ubuntu runtime with system tools + OpenCode + ps-mcp binary.
#
# Build context must be the repo root (same as crates/Dockerfile):
#   docker build -f crates/ps-agent/agent-container/Dockerfile --target prism-agent-dev --build-arg PROFILE=debug .

ARG PROFILE=release

# ---------------------------------------------------------------------------
# Builder — reuses the same pattern as crates/Dockerfile
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
RUN find crates tests -name Cargo.toml -exec sh -c '\
      dir=$(dirname "$1"); \
      if [ ! -f "$dir/src/main.rs" ] && [ ! -f "$dir/src/lib.rs" ]; then \
        mkdir -p "$dir/src" && touch "$dir/src/lib.rs"; \
      fi' _ {} \;

ENV SQLX_OFFLINE=true
ARG PROFILE
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build \
        $([ "$PROFILE" = "release" ] && echo "--release") \
        --bin ps-mcp \
    && mkdir -p /out \
    && cp target/$([ "$PROFILE" = "release" ] && echo "release" || echo "debug")/ps-mcp /out/ps-mcp

# ---------------------------------------------------------------------------
# Agent runtime — Ubuntu with system tools for code analysis
# ---------------------------------------------------------------------------
FROM ubuntu:24.04 AS agent-runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    git curl ca-certificates xz-utils \
    python3 python3-venv \
    && rm -rf /var/lib/apt/lists/*

# ripgrep
RUN ARCH="$(uname -m)" && \
    curl -sL "https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-${ARCH}-unknown-linux-musl.tar.gz" \
    | tar xz --strip-components=1 -C /usr/local/bin --wildcards '*/rg'

# tokei (code statistics)
RUN curl -sL https://github.com/XAMPPRocky/tokei/releases/download/v12.1.2/tokei-x86_64-unknown-linux-gnu.tar.gz \
    | tar xz -C /usr/local/bin

# uv (Python version + environment manager)
RUN curl -LsSf https://astral.sh/uv/install.sh | sh
ENV PATH="/root/.local/bin:$PATH"

# OpenCode — pinned version for stability
ARG OPENCODE_VERSION=0.1.0
RUN curl -fsSL https://opencode.ai/install | bash -s -- --no-modify-path
ENV PATH="/root/.opencode/bin:$PATH"

# ps-mcp binary from the builder stage
COPY --from=builder /out/ps-mcp /usr/local/bin/ps-mcp

# Workspace for cloned repos and generated files
RUN mkdir /workspace
WORKDIR /workspace

# OpenCode configuration + agent prompt (in WORKDIR so opencode discovers it)
COPY crates/ps-agent/agent-container/opencode.json /workspace/opencode.json
COPY crates/ps-agent/agent-container/.opencode/ /workspace/.opencode/

EXPOSE 4096

CMD ["opencode", "serve", "--port", "4096", "--hostname", "0.0.0.0"]

# ---------------------------------------------------------------------------
# Dev target for Tilt
# ---------------------------------------------------------------------------
FROM agent-runtime AS prism-agent-dev
