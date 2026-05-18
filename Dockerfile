###############################################################################
# rblog — multi-stage Dockerfile
#
# Stage layout:
#   admin-builder   : Node 22 + pnpm; builds the React SPA into ./dist
#   rust-builder    : rust 1.83-bookworm; compiles `rblog` with --features embed-admin
#                     so the SPA is baked into the final binary.
#   runtime         : debian-bookworm-slim with only the libraries wasmtime/sqlx
#                     need at runtime; ships the single binary + default config.
#
# Build:
#   docker build -t rblog:latest .
#
# Run (sqlite, all data ephemeral):
#   docker run --rm -p 8080:8080 rblog:latest
#
# Run (mysql via docker-compose):
#   docker compose up --build
###############################################################################

# ────────────────────────── admin SPA builder ────────────────────────────────
FROM node:22-bookworm-slim AS admin-builder
WORKDIR /admin

# Install pnpm via corepack so the version is pinned by the lockfile rather
# than the image. The base image ships corepack.
RUN corepack enable && corepack prepare pnpm@9.12.3 --activate

# Cache layer for dependencies. We copy the lockfile + manifest separately
# so changes to source code don't bust the install cache.
COPY admin/package.json admin/pnpm-lock.yaml* ./
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile || pnpm install

# Now copy the rest of the SPA and produce a static bundle in /admin/dist.
COPY admin/ ./
RUN pnpm build

# ────────────────────────── rust toolchain builder ───────────────────────────
FROM rust:1.83-bookworm AS rust-builder
WORKDIR /src

# Build-time deps for SQLx (rustls), MySQL bindings, tantivy fs ops, wasmtime.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev clang make cmake \
 && rm -rf /var/lib/apt/lists/*

# Copy the whole workspace. We rely on a `.dockerignore` to keep the
# context small (no target/, no admin/node_modules, etc.).
COPY . .

# Drop in the SPA bundle produced by the previous stage. The
# `embed-admin` Cargo feature reads from `admin/dist/` via `rust-embed`.
COPY --from=admin-builder /admin/dist ./admin/dist

# Compile in release mode with the SPA baked in.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --bin rblog --features rblog-http/embed-admin \
 && cp /src/target/release/rblog /usr/local/bin/rblog

# ────────────────────────── final runtime image ──────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 tini \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --system rblog \
 && useradd  --system --gid rblog --home /var/lib/rblog --shell /usr/sbin/nologin rblog \
 && mkdir -p /var/lib/rblog /etc/rblog \
 && chown -R rblog:rblog /var/lib/rblog /etc/rblog

# Copy the compiled binary and a sane default config.
COPY --from=rust-builder /usr/local/bin/rblog /usr/local/bin/rblog
COPY rblog.example.toml /etc/rblog/rblog.toml

ENV RBLOG__SERVER__BIND="0.0.0.0:8080" \
    RBLOG__PATHS__THEMES_ROOT="/var/lib/rblog/themes" \
    RBLOG__PATHS__UPLOADS_ROOT="/var/lib/rblog/uploads" \
    RBLOG__PATHS__SEARCH_ROOT="/var/lib/rblog/search-index" \
    RBLOG__PATHS__PLUGINS_ROOT="/var/lib/rblog/plugins" \
    RBLOG__DATABASE__URL="sqlite:///var/lib/rblog/rblog.db?mode=rwc" \
    RUST_LOG="info,sqlx=warn"

WORKDIR /var/lib/rblog
USER rblog
EXPOSE 8080

# tini reaps zombies; rblog itself handles graceful shutdown on SIGINT/SIGTERM.
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/rblog"]
