# === Build stage ===
FROM rust:1.94-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY dashboard/ dashboard/

RUN cargo build --release --bin quailsync-server

# === Runtime stage ===
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/quailsync-server /usr/local/bin/quailsync-server

# Working directory is /data so the relative "quailsync.db" path
# writes to /data/quailsync.db (persisted via volume mount)
WORKDIR /data

EXPOSE 3000 3443

CMD ["quailsync-server"]
