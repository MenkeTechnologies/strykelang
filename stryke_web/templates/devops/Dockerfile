# Multi-stage Dockerfile for stryke web apps.
#
# Stage 1: cargo build of stryke + s_web (cached). Stage 2: minimal
# runtime that copies the binaries + the app directory.
#
# Usage:  docker build -t {{app_name}} .
#         docker run --rm -p 3000:3000 -e STRYKE_ENV=production {{app_name}}

FROM rust:1.83-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev clang && rm -rf /var/lib/apt/lists/*

# Build stryke + s_web. In a real registry deploy, replace with
# `cargo install stryke stryke_web` once published.
COPY . /build
RUN cargo build --release --bin stryke || true
RUN test -f target/release/stryke

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/stryke /usr/local/bin/stryke
COPY app /app/app
COPY config /app/config
COPY db /app/db
COPY public /app/public
COPY bin /app/bin

ENV STRYKE_ENV=production
ENV PORT=3000
EXPOSE 3000

# Run migrations on container start, then boot the server.
CMD ["sh", "-c", "stryke -e 'require \"./config/application.stk\"; web_db_connect(\"sqlite://db/${STRYKE_ENV}.sqlite3\"); for my $f (glob \"db/migrate/**/*.stk\") { require $f } web_migrate()' && stryke bin/server"]
