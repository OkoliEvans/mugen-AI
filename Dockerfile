# syntax=docker/dockerfile:1

# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:bookworm AS builder

WORKDIR /app

# Cache dependencies separately from source
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY gateway ./gateway
COPY settler ./settler
COPY common ./common

RUN cargo build --release --bin gateway

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runner

WORKDIR /app

# Runtime deps: openssl + ca-certs for HTTPS (alloy RPC calls, Pinata, StarkNet)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy binary
COPY --from=builder /app/target/release/gateway /app/gateway

# Copy prover worker + artifacts
# The prover runs as a Python subprocess — needs python3 + ezkl in the image
# If you want a leaner image, move proof generation to a separate worker service
COPY prover /app/prover

RUN apt-get update && apt-get install -y \
    python3 \
    python3-pip \
    python3-venv \
    && rm -rf /var/lib/apt/lists/*

# Create venv and install ezkl inside the image
RUN python3 -m venv /app/prover/.venv && \
    /app/prover/.venv/bin/pip install --no-cache-dir ezkl

ENV PYTHON_BIN=/app/prover/.venv/bin/python3
ENV WORKER_SCRIPT=/app/prover/worker.py
ENV ARTIFACTS_DIR=/app/prover/artifacts
ENV HOST=0.0.0.0
ENV PORT=8080

EXPOSE 8080

CMD ["/app/gateway"]