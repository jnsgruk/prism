# syntax=docker/dockerfile:1

# Multi-stage Dockerfile for the Vite SPA frontend.
#
# Dev target (Tilt):
#   docker build --target ps-frontend-dev frontend/
#
# Prod target (CI):
#   docker build --target ps-frontend frontend/

# ---------------------------------------------------------------------------
# Builder — install deps, build static SPA
# ---------------------------------------------------------------------------
FROM oven/bun:1 AS builder
WORKDIR /app

# Install dependencies first (layer caching)
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile

# Copy source and build
COPY . .
RUN bun run build

# ---------------------------------------------------------------------------
# Dev runtime — Caddy on Ubuntu for Tilt
# ---------------------------------------------------------------------------
FROM ubuntu:24.04 AS dev-runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates caddy \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --shell /bin/bash prism

FROM dev-runtime AS ps-frontend-dev
COPY Caddyfile /etc/caddy/Caddyfile
COPY --from=builder /app/dist /srv
USER prism
EXPOSE 3000
CMD ["caddy", "run", "--config", "/etc/caddy/Caddyfile"]

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
    libstdc++6_libs \
    zlib1g_libs

# ---------------------------------------------------------------------------
# Caddy binary — grab from official image
# ---------------------------------------------------------------------------
FROM caddy:2 AS caddy

# ---------------------------------------------------------------------------
# Prod runtime
# ---------------------------------------------------------------------------
FROM scratch AS ps-frontend
COPY --from=chisel /rootfs /
COPY --from=caddy /usr/bin/caddy /usr/bin/caddy
WORKDIR /srv
COPY Caddyfile /etc/caddy/Caddyfile
COPY --from=builder /app/dist /srv
USER 65534:65534
EXPOSE 3000
CMD ["caddy", "run", "--config", "/etc/caddy/Caddyfile"]
