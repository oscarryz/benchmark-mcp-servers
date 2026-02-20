# ── Build stage ────────────────────────────────────────────────────────────────
FROM rust:1.93.1-alpine AS builder

WORKDIR /app

# Cache dependencies before copying source
COPY Cargo.toml Cargo.lock ./

COPY src ./src
RUN cargo build --release

# ── Runtime stage ──────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# reqwest needs TLS (native-tls or rustls) — install ca-certificates
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*


WORKDIR /app

COPY --from=builder /app/target/release/rust-server .

EXPOSE 8084

CMD ["./rust-server"]