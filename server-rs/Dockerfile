# Cart import server, Rust edition.
# Build: docker build -t kroger-cart-server server-rs
# Run:   docker compose up   (see ../docker-compose.yml)

FROM rust:1.93-slim AS builder

WORKDIR /app

# 1. Build dependencies first (cached unless Cargo.toml/Cargo.lock change).
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

# 2. Add the source and build the real binary.
COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/kroger-cart-server /usr/local/bin/kroger-cart-server

# Persist OAuth token + UPC cache on a volume so they survive restarts.
ENV KROGER_TOKEN_STORE=/data/token.json \
    KROGER_UPC_CACHE=/data/upc_cache.json
VOLUME ["/data"]

EXPOSE 8000
EXPOSE 8088

# Default: serve the API. Override for one-time login (see docker-compose.yml).
CMD ["kroger-cart-server"]
