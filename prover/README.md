# Elenxis — Prover (Python/EZKL Layer)

This directory contains the Python prover worker. It is the only Python in the
entire Elenxis stack. Rust owns everything else.

## Files

| File               | Purpose                                                   |
| ------------------ | --------------------------------------------------------- |
| `export_model.py`  | Export PyTorch model → ONNX + sample input.json           |
| `setup_circuit.py` | Compile ONNX → ZK circuit, generate pk/vk keys (run once) |
| `worker.py`        | Main worker process — spawned by Rust per proof job       |
| `benchmark.py`     | Phase 1 spike benchmark — measures time, RAM, proof size  |
| `gen_verifier.py`  | Generate Solidity verifier contract from vk.key           |
| `requirements.txt` | Python dependencies                                       |
| `Dockerfile`       | Container for the prover worker                           |

## Setup

```bash
# Install deps
pip install -r requirements.txt

# 1. Export model
python export_model.py --model resnet18 --out-dir ./artifacts

# 2. Compile circuit + generate keys (slow, run once)
python setup_circuit.py --artifacts-dir ./artifacts

# 3. Run benchmark to validate environment
python benchmark.py --artifacts-dir ./artifacts --runs 3

# 4. Generate Solidity verifier
python gen_verifier.py --artifacts-dir ./artifacts --contracts-dir ../contracts/src
```

## Rust ↔ Python Interface

Rust spawns `worker.py` as a subprocess per proof job.

**stdin** (JSON):

```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "input_data": [[0.1, 0.2, ...]],
  "artifacts_dir": "./artifacts"
}
```

**stdout** (JSON, one line):

```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "ok",
  "proof_path": "./artifacts/550e8400-e29b-41d4-a716-446655440000/proof.json"
}
```

The `proof_path` is always `<artifacts_dir>/<job_id>/proof.json`. The Rust
settler reads this path directly to load proof bytes before submitting to
`InferenceBridge.verifyAndBridge()`.

**exit codes:** `0` = success, `1` = failure

## Artifacts Directory

After `setup_circuit.py`, the `artifacts/` directory contains:

```
artifacts/
├── model.onnx          # exported model
├── input.json          # sample input for calibration
├── settings.json       # circuit precision settings
├── model.compiled      # arithmetic circuit
├── vk.key              # verification key (public)
├── pk.key              # proving key (private, large)
├── benchmark_results.json  # after running benchmark.py
└── <job_id>/
    └── proof.json      # generated per job — read by settler.rs
```

`pk.key` can be several GB for large models. Mount it as a volume in production,
don't bake it into the Docker image.

## proof.json Structure

EZKL 23.x proof format — read by `ParsedProof::from_file()` in the settler crate:

```json
{
  "proof": [41, 172, 170, ...],
  "instances": [[
    "0d00000000000000000000000000000000000000000000000000000000000000",
    ...
  ]]
}
```

`proof` is an array of raw `u8` bytes. `instances` is a nested array of
32-byte little-endian hex strings representing the public inputs (model outputs
as field elements). The settler byte-reverses each instance before parsing as
`U256` for the on-chain call.
