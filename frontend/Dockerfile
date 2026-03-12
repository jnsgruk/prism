# syntax=docker/dockerfile:1

# Multi-stage Dockerfile for the Next.js frontend.
#
# Dev target (Tilt):
#   docker build --target ps-frontend-dev frontend/
#
# Prod target (CI):
#   docker build --target ps-frontend frontend/

# ---------------------------------------------------------------------------
# Builder — install deps, build Next.js standalone
# ---------------------------------------------------------------------------
FROM oven/bun:1 AS builder
WORKDIR /app

# Install dependencies first (layer caching)
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile

# Copy source and build
COPY . .
ENV NEXT_TELEMETRY_DISABLED=1
RUN bun run build

# ---------------------------------------------------------------------------
# Dev runtime — Ubuntu base for Tilt
# ---------------------------------------------------------------------------
FROM ubuntu:24.04 AS dev-runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates nodejs \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --shell /bin/bash prism

FROM dev-runtime AS ps-frontend-dev
WORKDIR /app
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
COPY --from=builder /app/public ./public
USER prism
ENV PORT=3000 HOSTNAME=0.0.0.0
EXPOSE 3000
CMD ["node", "server.js"]

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
    zlib1g_libs \
    nodejs_bins

# ---------------------------------------------------------------------------
# Prod runtime
# ---------------------------------------------------------------------------
FROM scratch AS ps-frontend
COPY --from=chisel /rootfs /
WORKDIR /app
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
COPY --from=builder /app/public ./public
USER 65534:65534
ENV PORT=3000 HOSTNAME=0.0.0.0
EXPOSE 3000
CMD ["node", "server.js"]
