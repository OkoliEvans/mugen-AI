# ── Stage 1: Build (Rust Gateway) ─────────────────────────────────────────────
FROM rust:bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y \
    libpq-dev \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations

RUN cargo build --release --bin gateway

# ── Stage 2: Runtime & EZKL Native Compilation ───────────────────────────────
FROM debian:bookworm-slim AS runner
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    python3-full \
    python3-pip \
    python3-dev \
    curl \
    libgomp1 \
    build-essential \
    git \
    && rm -rf /var/lib/apt/lists/*

# 1. Install Rust 1.89.0 (Stability Pin)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.89.0
ENV PATH="/root/.cargo/bin:${PATH}"

# 2. Copy the Rust binary
COPY --from=builder /app/target/release/gateway /app/gateway

# 3. Setup Python Virtual Environment
RUN python3 -m venv /app/prover/.venv && \
    /app/prover/.venv/bin/pip install --no-cache-dir --upgrade pip setuptools wheel maturin

# 4. BUILD EZKL FROM SOURCE
RUN git clone --branch v23.0.5 https://github.com/zkonduit/ezkl.git /tmp/ezkl && \
    cd /tmp/ezkl && \
    VIRTUAL_ENV=/app/prover/.venv /app/prover/.venv/bin/maturin build \
        --release \
        --out dist \
        --interpreter /app/prover/.venv/bin/python3 && \
    /app/prover/.venv/bin/pip install dist/*.whl && \
    cd /app && rm -rf /tmp/ezkl

# 5. Copy Prover logic and script
COPY prover /app/prover
RUN chmod +x /app/prover/fetch_artifacts.sh

# 6. Install Python requirements (Torch CPU)
RUN /app/prover/.venv/bin/pip install --no-cache-dir \
        torch torchvision \
        --index-url https://download.pytorch.org/whl/cpu && \
    /app/prover/.venv/bin/pip install --no-cache-dir -r /app/prover/requirements.txt

# 7. Environment Variables
ENV ARTIFACTS_DIR=/app/prover/artifacts
ENV PYTHON_BIN=/app/prover/.venv/bin/python3
ENV WORKER_SCRIPT=/app/prover/worker.py
ENV EZKL_CACHE_DIR=/root/.ezkl
ENV HOST=0.0.0.0
ENV PORT=8080

# Create persistence directories
# Note: If using Koyeb Volumes, mount them to /app/prover/artifacts AND /root/.ezkl
RUN mkdir -p $ARTIFACTS_DIR && mkdir -p /root/.ezkl/srs

EXPOSE 8080

# 8. Start-up: Run your fetch script (which now handles R2 + SRS) then start Gateway
CMD ["/bin/sh", "-c", "/app/prover/fetch_artifacts.sh && /app/gateway"]