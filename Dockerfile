# syntax=docker/dockerfile:1

# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM --platform=linux/amd64 rust:bookworm AS builder

WORKDIR /app

# Install build-time dependencies (libpq-dev for diesel/postgres)
RUN apt-get update && apt-get install -y libpq-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations

RUN cargo build --release --bin gateway

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM --platform=linux/amd64 debian:bookworm-slim AS runner

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    python3 \
    python3-pip \
    python3-venv \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy binary
COPY --from=builder /app/target/release/gateway /app/gateway

# Copy prover worker (excluding artifacts — fetched at runtime from R2)
COPY prover /app/prover

# Make fetch script executable
RUN chmod +x /app/prover/fetch_artifacts.sh

# Setup Python environment for ezkl
RUN python3 -m venv /app/prover/.venv && \
    /app/prover/.venv/bin/pip install --no-cache-dir --upgrade pip setuptools wheel && \
    /app/prover/.venv/bin/pip install --no-cache-dir ezkl

ENV PYTHON_BIN=/app/prover/.venv/bin/python3
ENV WORKER_SCRIPT=/app/prover/worker.py
ENV ARTIFACTS_DIR=/app/prover/artifacts
ENV HOST=0.0.0.0
ENV PORT=8080

EXPOSE 8080

CMD ["/bin/sh", "-c", "/app/prover/fetch_artifacts.sh && /app/gateway"]