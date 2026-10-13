# syntax=docker/dockerfile:1
#
# Zhi-Mart backend — multi-stage build.
#   docker build -t zhimart-backend .
#   docker run --env-file .env -p 8080:8080 zhimart-backend
#
# No secrets are baked into the image: all configuration comes from the
# environment at runtime (see .env.example).

ARG RUST_VERSION=1.97

# ---------------------------------------------------------------------------
# Builder
# ---------------------------------------------------------------------------
FROM rust:${RUST_VERSION}-slim-bookworm AS builder
WORKDIR /app
ENV CARGO_TERM_COLOR=never \
    CARGO_INCREMENTAL=0

# 1) Dependencies only. This layer is reused until Cargo.toml/Cargo.lock change.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
 && echo 'fn main() {}' > src/main.rs \
 && touch src/lib.rs \
 && cargo build --release --locked \
 && rm -rf src \
           target/release/zhimart-backend* \
           target/release/deps/zhimart_backend-* \
           target/release/deps/libzhimart_backend-* \
           target/release/.fingerprint/zhimart-backend-*

# 2) The application. Migrations are embedded into the binary at compile time
#    by sqlx::migrate!, so they are only needed in this stage.
COPY migrations ./migrations
COPY src ./src
RUN cargo build --release --locked \
 && cp target/release/zhimart-backend /usr/local/bin/zhimart-backend

# ---------------------------------------------------------------------------
# Runtime: distroless (glibc + CA certificates, no shell, no package manager)
# ---------------------------------------------------------------------------
FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=builder /usr/local/bin/zhimart-backend /usr/local/bin/zhimart-backend

ENV APP_ENV=production \
    BIND_ADDR=0.0.0.0:8080

USER nonroot:nonroot
EXPOSE 8080

# No HEALTHCHECK: there is no shell/curl in distroless. Point your
# orchestrator / load balancer at GET /health/live and GET /health/ready.
ENTRYPOINT ["/usr/local/bin/zhimart-backend"]
CMD ["serve"]
