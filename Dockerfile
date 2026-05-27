# syntax=docker/dockerfile:1
# ── Kooix Gate · multi-stage build ────────────────────────────
# cargo-chef for dependency caching · debian-slim runtime

# ── Stage 1: chef base ────────────────────────────────────────
FROM rust:1.88-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

# ── Stage 2: planner (generate recipe.json) ───────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3: builder (compile deps, then project) ─────────────
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin gate-server --bin kgctl

# ── Stage 4: web builder (SvelteKit) ──────────────────────────
FROM node:22-alpine AS web-builder
WORKDIR /app
COPY web/package*.json ./
RUN npm ci
COPY web/ .
RUN npm run build

# ── Stage 5: server runtime (minimal Rust) ────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY --from=builder /app/target/release/gate-server /usr/local/bin/
COPY --from=builder /app/target/release/kgctl /usr/local/bin/

# Migration files for `kgctl migrate`
COPY crates/gate-storage/migrations /app/migrations

ENV RUST_LOG=info
EXPOSE 8000

CMD ["gate-server"]

# ── Stage 6: web runtime (SvelteKit node server) ──────────────
# 0.4.183 修：runtime 用 node:22-alpine 真起 SvelteKit adapter-node handler。
# adapter-node 产物是 /app/build/handler.js，需要 node 进程托管，不能塞进 Rust 镜像。
# adapter-node 把 dependencies 标 external，runtime 必须 npm ci --omit=dev 装回 peer。
FROM node:22-alpine AS web-runtime
WORKDIR /app
COPY web/package.json web/package-lock.json ./
RUN npm ci --omit=dev && npm cache clean --force
COPY --from=web-builder /app/build ./build
ENV NODE_ENV=production
ENV PORT=8080
ENV HOST=0.0.0.0
EXPOSE 8080
CMD ["node", "build/index.js"]
