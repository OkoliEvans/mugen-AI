# Mugen

**Verifiable Inference Network**

> Prove any ML inference. On-chain.

Mugen generates ZK proofs of model inference using EZKL, verifies them on-chain via Halo2 KZG, and settles the attestation on StarkNet — all from a single SDK call.

---

## Table of Contents

- [How It Works](#how-it-works)
- [Architecture](#architecture)
- [Model Registry & IPFS](#model-registry--ipfs)
- [Deployed Contracts](#deployed-contracts)
- [Quick Start](#quick-start)
- [API Reference](#api-reference)
- [Deployment Guide](#deployment-guide)

---

## How It Works

Standard ML inference produces no cryptographic guarantees — there is no way to verify on-chain that a given output was produced by a specific model from a specific input. Mugen solves this using ZK proofs.

**The pipeline:**

1. A client submits an inference request to the Mugen gateway with a model ID and input data.
2. The gateway runs the model via EZKL, which executes the inference inside a ZK circuit and produces a Halo2 KZG proof alongside the output.
3. The proof is submitted to `InferenceBridge.sol` on Ethereum Sepolia. The contract runs the KZG pairing check on-chain, confirming the output was produced honestly by the committed model.
4. Upon successful verification, `InferenceBridge.sol` calls `IStarknetMessaging.sendMessageToL2()` — forwarding the attestation to StarkNet via the canonical L1→L2 messaging contract.
5. `InferenceVerifier.cairo` on StarkNet Sepolia consumes the message and permanently records the attestation: inference ID, model hash, submitter, and timestamp.
6. The SDK resolves with the EVM transaction hash and a proof-derived attestation hash. The settlement is now queryable on StarkNet.

**What gets proven:** That a specific model (identified by `keccak256(name, version)`) produced a specific output from a specific input. The proof is binding — the same public inputs always produce the same valid proof for a committed model.

---

## Architecture

```
Model Registration (once per model)
    │
    ├── POST /v1/models → Pinata IPFS → CID
    └── InferenceVerifier.sol.registerModel(modelId, ipfsCid, inputShapeHash)

Client (SDK)
    │
    ▼
Mugen Gateway  (Rust / Actix-web)
    │  POST /v1/jobs
    │  Queues job, persists to Postgres
    │  Runs EZKL proof generation (Python subprocess)
    │  Proof committed to registered modelId
    │
    ▼
InferenceBridge.sol  (Ethereum Sepolia)
    │  Verifies Halo2 KZG proof on-chain
    │  Emits InferenceVerified event
    │  Calls IStarknetMessaging.sendMessageToL2()
    │
    ▼  L1→L2 message (~1–3 min relay)
    │
InferenceVerifier.cairo  (StarkNet Sepolia)
    │  #[l1_handler] consume_inference_result()
    │  Validates L1 sender against whitelist
    │  Replay protection via inference_id
    │  Writes InferenceRecord to storage
    │  Emits InferenceVerified event
    │
    ▼
Settlement confirmed — SDK resolves
```

**Crate layout:**

```
crates/
├── gateway/        — Actix-web HTTP API, job lifecycle, DB persistence
├── settler/        — Alloy EVM submitter + StarkNet poller
├── prover_manager/ — EZKL subprocess orchestration
├── common/         — Diesel models, repo layer, migrations
└── ipfs/           — Pinata client for model artifact pinning

contracts/
├── evm/
│   ├── InferenceVerifier.sol   — on-chain proof registry
│   ├── InferenceBridge.sol     — KZG verifier + L1→L2 relay
│   └── script/Deploy.s.sol     — Foundry deployment script
└── starknet_l2/
    └── src/
        └── inference_verifier.cairo  — settlement consumer

sdk/
└── src/
    ├── client.ts   — ElenxisClient
    ├── types.ts    — shared types
    ├── poller.ts   — job polling loop
    └── http.ts     — Axios wrapper with retry
```

## Model Registry & IPFS

Before a model can be used for inference, it must be registered. Registration does two things: pins the model artifact to IPFS via Pinata, and records the model identity on-chain so the proof circuit can commit to it.

**What gets stored on IPFS:**

| Artifact | Format | Description |
|---|---|---|
| Model artifact | `.onnx` | The ONNX model file, base64-decoded from the registration request |

The file is pinned via the Pinata v2 API (`/pinning/pinFileToIPFS`) and returns a content-addressed CID (e.g. `QmXyz...` or `bafy...`). The CID is permanent — the same bytes always produce the same CID regardless of where they are pinned.

**What gets stored on-chain (`InferenceVerifier.sol`):**

| Field | Value |
|---|---|
| `modelId` | `keccak256(abi.encodePacked(name, version))` |
| `ipfsCid` | The Pinata CID string |
| `inputShapeHash` | `keccak256(abi_encode(uint256[]))` of the input dimensions |

The `modelId` is the binding key between the IPFS artifact and the ZK circuit. When EZKL generates a proof, it commits to this same `modelId` as a public input — so the on-chain verifier can confirm not just that _a_ valid inference was run, but that it was run on the _specific registered model_ at the _specific IPFS CID_.

**Registration flow:**

```
POST /v1/models  { name, version, artifact_b64, input_shape }
       │
       ├── 1. Decode base64 artifact bytes
       ├── 2. Pin to IPFS via Pinata → CID
       ├── 3. Call InferenceVerifier.registerModel(modelId, ipfsCid, inputShapeHash)
       └── 4. Persist to Postgres (model_id, ipfs_cid, on_chain_hash)
```

**IPFS gateway URL** — after registration the artifact is publicly accessible at:
```
https://<PINATA_GATEWAY_URL>/ipfs/<CID>
```

**Required env vars for IPFS:**
```dotenv
PINATA_JWT=eyJ...              # Pinata v2 JWT (Admin or pinFileToIPFS scope)
PINATA_GATEWAY_URL=<your-gateway>.mypinata.cloud
```

If `PINATA_JWT` is not set, `POST /v1/models` returns `503` — inference jobs can still run against models that were previously registered, but new model registration is unavailable.

---
## Deployed Contracts

### Ethereum Sepolia

| Contract | Address |
|---|---|
| Halo2Verifier | `0x7bcf4980868bA06A38AC561904aE6BDEd9Ee46D2` |
| InferenceVerifier | `0x37c5c1E314d2d895Dce71d2fbDBB49DDA74c8699` |
| InferenceBridge | `0x820fa9edB1DD0f248A4a8FB44693505417656480` |
| StarkNet Core (L1 messaging) | `0xE2Bb56ee936fd6433DC0F6e7e3b8365C906AA057` |

### StarkNet Sepolia

| Contract | Address |
|---|---|
| InferenceVerifier.cairo | `0x048e54ece6691ca3f76246895cc1ac7c073a9377e02518aeed908618eb5ec7ca` |

---

## Quick Start

### Prerequisites

- Node.js 18+
- A running Mugen gateway (see [Deployment Guide](#deployment-guide))

### Install the SDK

```bash
npm install @mugen/sdk
```

### Verify an inference

```typescript
import { ElenxisClient } from '@mugen/sdk';

const client = new ElenxisClient({
  gatewayUrl: 'http://localhost:8080',
  timeoutMs:  300_000,   // 5 min — accounts for L1→L2 relay time
});

const result = await client.verifyInference({
  modelId:   'tiny_mlp_v1',
  inputData: [[0.1, 0.2, 0.3, 0.4]],
});

console.log(result.txHash);           // Eth Sepolia tx hash
console.log(result.attestationHash);  // keccak256-derived proof fingerprint
console.log(result.elapsedMs);        // total wall time including StarkNet relay
```

### Run the e2e test

```bash
cd sdk
GATEWAY_URL=http://localhost:8080 TIMEOUT_MS=300000 npx tsx e2e_verify.ts
```

Expected output:

```
  ✅ PASS — Gateway is healthy
  ✅ PASS — result.jobId is a non-empty string
  ✅ PASS — result.txHash starts with 0x
  ✅ PASS — result.attestationHash starts with 0x and is 66 chars
  ✅ PASS — result.elapsedMs is a positive number
  ✅ PASS — job.status === "settled"
  ✅ PASS — job.txHash matches verifyInference result
  ✅ PASS — proof.proofHex is a non-empty hex string
  ✅ PASS — proof.sizeBytes > 0

  🎉 All assertions passed
```

---

## API Reference

### `POST /v1/jobs`

Submit an inference job.

**Request:**
```json
{
  "model_id":   "tiny_mlp_v1",
  "input_data": [[0.1, 0.2, 0.3, 0.4]]
}
```

**Response:**
```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "queued"
}
```

### `GET /v1/jobs/:id`

Poll job status.

**Response (settled):**
```json
{
  "job_id":     "550e8400-e29b-41d4-a716-446655440000",
  "status":     "settled",
  "tx_hash":    "0x9a6c10bed212d03dac524252aeaf7605cb960721b6a7a3afab462cad330ca81d",
  "proof_path": "prover/artifacts/550e8400.../proof.json"
}
```

Status values: `queued` → `running` → `done` → `settled` | `failed`

### `GET /v1/jobs/:id/proof`

Fetch raw proof bytes for a completed job.

**Response:**
```json
{
  "job_id":     "550e8400-e29b-41d4-a716-446655440000",
  "proof_hex":  "29acaa...",
  "size_bytes": 14203
}
```

### `POST /v1/models`

Register a model with IPFS pinning and on-chain registration.

**Request:**
```json
{
  "name":         "tiny_mlp_v1",
  "version":      "0.1.0",
  "artifact_b64": "<base64-encoded ONNX>",
  "input_shape":  [1, 4]
}
```

**Response:**
```json
{
  "model_id":          "uuid",
  "on_chain_model_id": "0xkeccak256...",
  "ipfs_cid":          "QmXyz...",
  "gateway_url":       "https://...",
  "on_chain_hash":     "0xtxhash..."
}
```

### `GET /healthz`

```json
{ "status": "ok", "version": "0.1.0", "settle_enabled": true, "db": "connected" }
```

---

## Deployment Guide

### Prerequisites

- Rust 1.75+
- Python 3.9+ with EZKL installed
- PostgreSQL
- Foundry (`forge`, `cast`)
- `sncast` (StarkNet Foundry)
- `starkli`

### 1. Environment

```dotenv
# Gateway
HOST=0.0.0.0
PORT=8080
DATABASE_URL=postgresql://user:pass@localhost:5432/mugen

# Prover
PYTHON_BIN=/path/to/.venv/bin/python3
WORKER_SCRIPT=prover/worker.py
ARTIFACTS_DIR=prover/artifacts
MAX_CONCURRENT=2
TIMEOUT_SECS=120

# Settler — must point to Eth Sepolia
SETTLER_RPC_URL=https://ethereum-sepolia-rpc.publicnode.com
SETTLER_PRIVATE_KEY=0x...
INFERENCE_VERIFIER_ADDRESS=0x37c5c1E314d2d895Dce71d2fbDBB49DDA74c8699

# StarkNet settlement
ETH_SEPOLIA_INFERENCE_BRIDGE=0x820fa9edB1DD0f248A4a8FB44693505417656480
STARKNET_RPC=https://starknet-sepolia.public.blastapi.io/rpc/v0_7
STARKNET_INFERENCE_VERIFIER=0x048e54ece6691ca3f76246895cc1ac7c073a9377e02518aeed908618eb5ec7ca
STARKNET_POLL_INTERVAL_SECS=15
STARKNET_MAX_POLL_ATTEMPTS=24
SETTLER_STARKNET_BRIDGE_FEE_WEI=30000000000000000

# IPFS — required for POST /v1/models
PINATA_JWT=eyJ...
PINATA_GATEWAY_URL=<your-gateway>.mypinata.cloud
```

### 2. Database

```bash
createdb mugen
cargo run -p gateway   # migrations run automatically on startup
```

### 3. Gateway

```bash
cargo build --release
./target/release/gateway
```

### 4. Deploy contracts (fresh deployment)

**StarkNet — declare and deploy `InferenceVerifier.cairo`:**

```bash
cd starknet_l2
scarb build

sncast --account <account> declare \
  --url $STARKNET_RPC \
  --contract-name InferenceVerifier

# Deploy — use 0x0 as placeholder for L1 bridge, whitelist after EVM deploy
sncast --account <account> deploy \
  --url $STARKNET_RPC \
  --class-hash <CLASS_HASH> \
  --arguments '<OWNER_ADDRESS>, 0x0'
```

**EVM — deploy `InferenceBridge.sol`:**

```bash
export HALO2_VERIFIER_ADDRESS=0x7bcf4980868bA06A38AC561904aE6BDEd9Ee46D2
export STARKNET_CORE_ADDRESS=0xE2Bb56ee936fd6433DC0F6e7e3b8365C906AA057
export CAIRO_DEST=$(python3 -c "print(int('<CAIRO_ADDR_WITHOUT_0x>', 16))")
export CAIRO_SELECTOR=$(starkli selector consume_inference_result)

forge script script/Deploy.s.sol:Deploy \
  --sig "deployBridge()" \
  --rpc-url $RPC_URL \
  --private-key $PRIVATE_KEY \
  --broadcast \
  --verify \
  --etherscan-api-key $ETHERSCAN_API_KEY
```

**Whitelist the bridge on `InferenceVerifier.cairo`:**

```bash
sncast --account <account> invoke \
  --url $STARKNET_RPC \
  --contract-address <INFERENCE_VERIFIER_CAIRO_ADDRESS> \
  --function add_l1_verifier \
  --calldata $(python3 -c "print(int('<BRIDGE_ADDRESS_WITHOUT_0x>', 16))")
```

Or invoke directly via Voyager — pass the ETH address in the `add_l1_verifier` field.

---

## License

MIT