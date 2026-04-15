# ─────────────────────────────────────────────────────────────
# Stage 1: Builder (Rust)
# ─────────────────────────────────────────────────────────────
FROM rust:bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    libpq-dev \
    pkg-config \
    libssl-dev \
    curl \
    git \
    protobuf-compiler \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Stability for SP1 + git deps
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt

# Copy workspace
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations

# IMPORTANT: include SDK (both Rust + TS exist)
COPY sdk ./sdk

# Build gateway only
RUN cargo build --release -p gateway


# ─────────────────────────────────────────────────────────────
# Stage 2: Runtime (minimal)
# ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runner

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    && rm -rf /var/lib/apt/lists/*

# Gateway binary
COPY --from=builder /app/target/release/gateway /app/gateway

COPY crates/aggregator-guest/elf /app/elf/aggregator
COPY crates/guest/elf /app/elf/guest
COPY weights /app/weights

# Runtime env
ENV HOST=0.0.0.0
ENV PORT=8080

# Default expected paths (match code)
ENV AGG_ELF_PATH=/app/elf/aggregator/aggregator-guest
ENV GUEST_ELF_PATH=/app/elf/guest/inference-guest

EXPOSE 8080

CMD ["/app/gateway"]