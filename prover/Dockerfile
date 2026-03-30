FROM python:3.11-slim

WORKDIR /app

# System deps
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Python deps
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy prover scripts
COPY export_model.py .
COPY setup_circuit.py .
COPY worker.py .
COPY benchmark.py .
COPY gen_verifier.py .

# Artifacts directory (mounted from host or shared volume in prod)
RUN mkdir -p /app/artifacts

# Default entrypoint is the worker — Rust spawns this as a subprocess
ENTRYPOINT ["python", "worker.py"]