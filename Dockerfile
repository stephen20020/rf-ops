# syntax=docker/dockerfile:1
# RF Ops server image for Render (render.yaml). Builds the web client and the server, then runs
# the server on a slim image. The persistent disk is mounted at /data.

FROM node:22-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
# The optional `extra_ca` secret is only for building behind a TLS-inspecting proxy; Render
# doesn't pass it.
RUN --mount=type=secret,id=extra_ca,required=false \
    if [ -f /run/secrets/extra_ca ]; then export NODE_EXTRA_CA_CERTS=/run/secrets/extra_ca; fi; \
    npm ci
COPY web/ ./
RUN npm run build

FROM rust:1-bookworm AS server
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN --mount=type=secret,id=extra_ca,required=false \
    if [ -f /run/secrets/extra_ca ]; then export CARGO_HTTP_CAINFO=/run/secrets/extra_ca; fi; \
    cargo build --release --locked -p rfops-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates tini \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server /src/target/release/rfops-server /app/rfops-server
COPY --from=web /src/web/dist /app/web
ENV RFOPS_DB=sqlite:///data/rfops.db \
    RFOPS_BACKUP_DIR=/data/backups \
    RFOPS_WEB=/app/web \
    RUST_LOG=info
# Runs as root: Render mounts the persistent disk root-owned, and a permission error on /data
# would only show up on the first real deploy.
EXPOSE 8080
# tini forwards SIGTERM so the server drains and checkpoints the WAL. After a Take over the
# server exits on purpose and Render starts it again into the restored database.
ENTRYPOINT ["/usr/bin/tini", "--", "/app/rfops-server"]
