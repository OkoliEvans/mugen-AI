# Mugen
## VEIL — Verifiable Execution and Inference Layer

**Verifiable Inference Network**

> Prove any ML inference. On-chain.

Mugen generates ZK proofs of ML model inference using SP1 (Succinct's zkVM), verifies them on-chain via Groth16, and settles attestations on HashKey Chain — all from a single SDK call.

> **MVP Note:** Proving is currently free. The fee mechanism (VeilVault) is deployed and functional but fee deduction is not yet enforced on inference submissions. This is intentional for the MVP phase — the full economic model will be activated in the production release.

---

## Table of Contents

- [Why Verifiable Inference](#why-verifiable-inference)
- [How It Works](#how-it-works)
- [Architecture](#architecture)
- [Fee Mechanism](#fee-mechanism)
- [Model Registry & IPFS](#model-registry--ipfs)
- [Proof Aggregation](#proof-aggregation)
- [Deployed Contracts](#deployed-contracts)
- [Quick Start](#quick-start)
- [API Reference](#api-reference)
- [Deployment Guide](#deployment-guide)

---

## Why Verifiable Inference

AI is making consequential decisions at scale — in trading, lending, hiring, medical diagnosis, and content moderation. But the institutions built around accountability were designed for human decision-making. When an algorithm denies a loan, recommends a trade, or flags a medical image, there is currently no cryptographic way to prove after the fact that a specific model produced a specific output from a specific input. Logs can be altered. Models can be silently updated. Audit trails are mutable.

Mugen closes this gap. By executing ML inference inside a ZK virtual machine, every decision becomes a proof: unforgeable, permanent, and verifiable by anyone without access to the original data.

**Prediction markets and finance.** Algorithmic trading funds are black boxes. A fund that claims 70% win rate has no way to prove the strategy wasn't back-adjusted or run on a different model than disclosed. With verifiable inference, every trade carries a proof tying it to a specific model version and specific inputs — a permanent, tamper-proof audit trail. This directly addresses MiFID II requirements for algorithmic trading accountability and enables genuinely trustless performance reporting.

**Credit scoring and lending.** When a bank denies a loan based on an AI model, regulators and courts currently have no way to confirm the disclosed model was actually used. A ZK proof of inference creates a legally defensible record: this exact model, this exact input, this exact decision, at this exact time. GDPR's right to explanation and the EU AI Act's transparency requirements for high-stakes AI decisions point directly at this problem. Mugen provides the cryptographic primitive that makes compliance possible by construction rather than by policy.

**Healthcare and diagnostics.** AI diagnostic tools that recommend treatments or flag abnormalities create liability questions that are currently resolved by mutable audit logs. When a patient outcome is disputed, the question is whether the AI actually recommended what the doctor claims, and whether it was the approved version of the model. With verifiable inference, every diagnostic output is a proof: model version, patient feature vector, recommendation, and timestamp — immutable on-chain. Post-market surveillance for FDA-regulated AI medical devices could shift from self-reporting to a complete verifiable record of every inference the device made.

**The key insight.** Verifiable inference does not require making the model public, exposing patient data, or revealing proprietary inputs. The ZK proof attests to the relationship between inputs and outputs without revealing either. You get accountability without sacrificing privacy or competitive advantage. That is the gap Mugen fills.

---

## How It Works

Standard ML inference produces no cryptographic guarantees — there is no way to verify on-chain that a given output was produced by a specific model from a specific input. Mugen solves this using ZK proofs of program execution via SP1.

**The pipeline:**

1. A client submits an inference request to the Mugen gateway with a model ID and input data.
2. The gateway sends the job to the SP1 prover network (Succinct). The inference is executed inside the SP1 zkVM, producing a compressed STARK proof alongside the output. The attestation hash is available after this phase (~60s).
3. A Groth16 SNARK wraps the STARK proof, producing a compact proof suitable for EVM verification (~120s additional).
4. Completed proofs are batched by the gateway's aggregator. When a batch threshold is reached (or a flush timer fires), all N compressed proofs are recursively verified inside a second SP1 program, producing one Groth16 proof that covers the entire batch.
5. The aggregated Groth16 proof is submitted to `InferenceVerifier.sol` on HashKey testnet via `submitAggregatedProof()`. The contract verifies the proof and emits `InferenceVerified` for each output hash in the batch.
6. The SDK resolves with the settlement tx hash and attestation hash.

**What gets proven:** That a specific model (identified by `sha256(weights_bytes)`) produced a specific output from a specific input. The proof is binding — the SP1 guest program commits `model_id`, `input_hash`, and `output_hash` as public values, and the on-chain verifier checks them against the registered model.

---

## Architecture

```
Model Registration (once per model)
    │
    ├── POST /v1/models → Pinata IPFS → CID
    └── InferenceVerifier.sol.registerModel(sha256(weights), ipfsCid, inputShapeHash)

Client (@mugen-ai/sdk or mugen-sdk Rust crate)
    │
    ▼
Mugen Gateway  (Rust / Actix-web)
    │  POST /v1/jobs
    │  Queues job, persists to Postgres
    │
    ▼
Prover Manager  (crates/prover-manager)
    │  Phase 1 — compressed STARK via SP1 Succinct Network (~60s)
    │    → attestation_hash available
    │    → CompressedData stored in memory
    │  Phase 2 — optional per-job Groth16 (currently bypassed by aggregation)
    │
    ▼
Batch Collector  (gateway/src/main.rs)
    │  Accumulates compressed proofs
    │  Flushes on BATCH_SIZE threshold OR BATCH_FLUSH_SECS timer
    │
    ▼
Aggregator  (crates/aggregator + crates/aggregator-guest)
    │  Recursively verifies N compressed proofs inside SP1 zkVM
    │  Commits merkle_root(output_hashes) + batch_size as public values
    │  Produces one Groth16 proof covering the entire batch
    │
    ▼
Settler  (crates/settler)
    │  submitAggregatedProof(proofBytes, publicValues, outputHashes[])
    │  InferenceVerifier.sol on HashKey testnet
    │  Marks each job "settled" in Postgres with tx_hash
    │
    ▼
Settlement confirmed — SDK resolves
```

**Crate layout:**

```
crates/
├── gateway/           — Actix-web HTTP API, job lifecycle, batch collector, DB persistence
├── settler/           — Alloy EVM submitter (individual + aggregated proofs)
├── prover-manager/    — SP1 two-phase proving orchestration (compressed → Groth16)
├── aggregator/        — Batch aggregation logic: builds stdin for aggregator-guest
├── aggregator-guest/  — SP1 zkVM program: recursively verifies N proofs, commits merkle root
├── guest/             — SP1 zkVM inference program: runs MLP, commits model_id/input_hash/output_hash
├── models/tiny_mlp/   — 4→8→2 MLP forward pass (Rust, no_std compatible)
├── common/            — Diesel models, repo layer, migrations
└── ipfs/              — Pinata client for model artifact pinning

contracts/evm/
├── src/InferenceVerifier.sol   — model registry + proof verifier + batch settlement
├── src/VeilVault.sol           — HSK credit vault for proof fee management
└── lib/sp1-contracts/          — SP1VerifierGateway + SP1VerifierGroth16 (v6.0.0)

sdk/
├── Rust/       — mugen-sdk Rust crate (VeilClient, verify_inference)
└── Typescript/ — @mugen-ai/sdk npm package (VeilClient)
```

---

## Fee Mechanism

Mugen uses a pre-funded credit vault model for proof fees. The `VeilVault.sol` contract is deployed on HashKey testnet and manages HSK balances for gateway users.

> **MVP status:** VeilVault is deployed and the full fee flow is implemented, but fee deduction is not enforced during the MVP phase. Proving is free. The vault contract is available for early depositors to test the deposit/withdraw flow ahead of production activation.

### Fee tiers

| Tier | Cost | Description |
|---|---|---|
| Standard | 2 HSK | Normal batch queue, settlement within 1–2 minutes |
| Priority | 5 HSK | Priority flush, faster batch settlement |

### How it works

1. Users deposit HSK into `VeilVault` once — no per-transaction approval required.
2. When a proof job is submitted, the gateway calls `VeilVault.deductFee(user, tier, jobId)` after the compressed proof is successfully submitted to the Succinct network. Failed proofs cost nothing.
3. Users can withdraw their remaining balance at any time — funds are never locked.
4. The gateway operator withdraws accumulated fees via `withdrawFees(treasury)`.

```solidity
// Deposit HSK to fund proofs
VeilVault.deposit{value: 20 ether}()  // funds 10 standard proofs

// Check balance
VeilVault.balanceOf(address user) → uint256  // wei

// Check if sufficient for a proof tier
VeilVault.canProve(address user, ProofTier tier) → bool

// Withdraw remaining balance
VeilVault.withdraw(uint256 amount)
```

### Account dashboard

The Mugen UI includes an account page (`/account`):

- Vault balance in HSK with USD equivalent
- Total proof count and cumulative HSK spent
- Balance history with deposit/deduct filtering
- Deposit and withdraw modals with proof count preview
- Real-time proof fee display (2 HSK standard / 5 HSK priority)

Connect any EVM wallet at `/account` to view your vault balance and transaction history.

### Security model

- Only the authorized gateway address can call `deductFee()` — enforced by the `onlyGateway` modifier
- Reentrancy guard on all state-changing functions
- Pausable for emergency stops
- `Ownable2Step` — two-transaction ownership transfer prevents accidents
- Gateway address is updatable by the owner for gateway version upgrades

---

## Model Registry & IPFS

Before a model can be used for inference, it must be registered. Registration pins the model weights to IPFS and records the model identity on-chain.

**What gets stored on IPFS:**

| Artifact | Format | Description |
|---|---|---|
| Model weights | raw binary | Flat f32 little-endian weight file |

**What gets stored on-chain (`InferenceVerifier.sol`):**

| Field | Value |
|---|---|
| `modelId` | `sha256(weights_bytes)` — must match what the SP1 guest commits |
| `ipfsCidHash` | `keccak256(ipfsCid)` |
| `inputShapeHash` | `keccak256(abi_encode(uint256[]))` of the input dimensions |

**Critical:** The `modelId` is `sha256(weights_bytes)`, not `keccak256(name + version)`. The SP1 guest computes `sha256(weights)` at proving time and commits it as a public value. The on-chain registry must use the same derivation or `submitProof` will revert with `ModelNotRegistered`.

**Registration flow:**

```
POST /v1/models  { name, version, artifact_b64, input_shape }
       │
       ├── 1. Decode base64 artifact bytes
       ├── 2. Pin to IPFS via Pinata → CID
       ├── 3. Compute modelId = sha256(artifact_bytes)
       ├── 4. Call InferenceVerifier.registerModel(modelId, ipfsCid, inputShapeHash)
       └── 5. Persist to Postgres (model_id, ipfs_cid, on_chain_hash)
```

**Required env vars for IPFS:**
```dotenv
PINATA_JWT=eyJ...
PINATA_GATEWAY_URL=<your-gateway>.mypinata.cloud
```

---

## Proof Aggregation

Mugen batches N inference proofs into a single on-chain transaction, making verifiable inference economically viable at scale.

**How it works:**

Each inference job produces a compressed SP1 proof (phase 1). Instead of submitting one Groth16 proof per job, the gateway accumulates compressed proofs in a batch collector. When the batch is full (or a flush timer fires), the aggregator-guest program verifies all N proofs recursively inside the SP1 zkVM and commits `merkle_root(output_hashes)` as its public output. One Groth16 proof covers the entire batch.

**On-chain:** `submitAggregatedProof(proofBytes, publicValues, outputHashes[])` on `InferenceVerifier.sol` verifies the batch proof and marks all N output hashes as settled in a single transaction.

**Batch configuration:**

```dotenv
BATCH_SIZE=10           # flush when N proofs are pending
BATCH_FLUSH_SECS=60     # flush timer interval
AGG_ELF_PATH=crates/aggregator-guest/elf/aggregator-guest
```

**Economic impact:** N inferences → 1 on-chain verification. At BATCH_SIZE=10, gas cost per inference drops by ~10x compared to individual settlement. Combined with the VeilVault fee model, this enables sub-cent per-proof economics at scale.

---

## Deployed Contracts

### HashKey Testnet (Chain ID 133)

| Contract | Address |
|---|---|
| InferenceVerifier | `0x69f77055e9A6e6B34539Db2BD733f9eB07F9f11f` |
| VeilVault | `0x47A7EA849d500625aa424bF90a5DF4814895C279` |
| SP1VerifierGateway | `0x0Be1C31a27F6477dd5DeB4eC4302B4cF199362CF` |
| SP1VerifierGroth16 (v6.0.0) | `0x75e3a5461eAa204a1fce8b54De3cf572aEEA9504` |

Explorer: [testnet-explorer.hsk.xyz](https://testnet-explorer.hsk.xyz)

### Registered Models

| Model | Version | modelId (sha256 of weights) | IPFS CID |
|---|---|---|---|
| tiny_mlp_v1 | 0.1.0 | `0x91db243a5b7956c0818e930c6abde3a47b14dd47a71928367409535f04aa32ed` | `QmYesbfR1uEzXyVVUwaiKvjJD3642f1w6WSsbNn4vpo47A` |
| polymarket_mlp_v1 | 0.1.0 | `0xac83407e4cd7e6336efa508e74680a6c667546f61cd54c3622b3027d8ea1a8e6` | `QmVGTbbYRRHwvKh2Lq84fhvYSZktKCQHG6jnu6sd55VNvf` |

---

## Quick Start

### TypeScript SDK

```bash
npm install @mugen-ai/sdk
```

```typescript
import { VeilClient } from '@mugen-ai/sdk'

const client = new VeilClient({
  gatewayUrl: 'https://your-gateway.xyz',
  timeoutMs:  600_000,
})

const job = await client.verifyInference({
  modelId:   'polymarket_mlp_v1',
  inputData: [[0.6, 0.4, 12000, 0.2]],
})

console.log(job.attestationHash) // keccak256(model_id||input_hash||output_hash)
console.log(job.txHash)          // HashKey testnet settlement tx
console.log(job.elapsedMs)       // total wall time
```

### Rust SDK

```toml
[dependencies]
mugen-sdk = { path = "sdk/Rust" }
```

```rust
use mugen_sdk::VeilClient;

let client = VeilClient::new("https://your-gateway.xyz");

let job = client
    .verify_inference("polymarket_mlp_v1", vec![vec![0.6, 0.4, 12000.0, 0.2]])
    .await?;

println!("attestation_hash: {}", job.attestation_hash);
println!("tx_hash:          {}", job.tx_hash);
```

### Run the e2e test

```bash
# TypeScript
cd sdk/Typescript
GATEWAY_URL=http://localhost:8080 npm run e2e

# Rust
cd sdk/Rust
GATEWAY_URL=http://localhost:8080 cargo test --test e2e -- --ignored
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
  "job_id":           "550e8400-e29b-41d4-a716-446655440000",
  "status":           "settled",
  "attestation_hash": "0xfac658be...",
  "tx_hash":          "0x11f18c8e..."
}
```

| Status | Meaning |
|---|---|
| `queued` | Job accepted, waiting for prover slot |
| `running` | SP1 prover is executing the inference |
| `proving` | Phase 1 complete — compressed proof ready, attestation_hash available |
| `done` | Proof queued in batch collector awaiting aggregation |
| `settled` | Aggregated proof verified on HashKey testnet, tx_hash available |
| `failed` | Proving or settlement failed |

### `GET /v1/jobs/:id/proof`

Returns attestation info for a completed job.

**Response:**
```json
{
  "job_id":           "550e8400-e29b-41d4-a716-446655440000",
  "status":           "compressed",
  "attestation_hash": "0xfac658be...",
  "note": "compressed proof is queued in the aggregator batch"
}
```

### `POST /v1/models`

Register a model with IPFS pinning and on-chain registration.

**Request:**
```json
{
  "name":         "tiny_mlp_v1",
  "version":      "0.1.0",
  "artifact_b64": "<base64-encoded weights binary>",
  "input_shape":  [1, 4]
}
```

**Response:**
```json
{
  "model_id":          "uuid",
  "on_chain_model_id": "0x91db243a...",
  "ipfs_cid":          "QmYesbfR...",
  "gateway_url":       "https://...",
  "on_chain_hash":     "0xtxhash..."
}
```

### `GET /v1/proofs`

Paginated list of all settled proofs. Used by the proof explorer.

```
GET /v1/proofs?page=1&limit=20
```

### `GET /v1/proofs/:attestation_hash_or_job_id`

Fetch a single proof by attestation hash or job UUID.

### `GET /v1/account/:wallet`

Returns vault balance, proof count, and HSK spent for a wallet address.

### `GET /v1/account/:wallet/history`

Paginated vault event history (deposits and deductions) for a wallet.

### `GET /healthz`

```json
{
  "status": "ok",
  "version": "0.1.0",
  "settle_enabled": true,
  "db": "connected"
}
```

---

## Deployment Guide

### Prerequisites

- Rust 1.75+ with SP1 toolchain (`cargo prove`)
- PostgreSQL
- Foundry (`forge`, `cast`)
- Succinct Network API key (`SP1_PRIVATE_KEY`)

### 1. Install SP1 toolchain

```bash
curl -L https://sp1up.dev | bash
sp1up --version 6.0.0
```

### 2. Build the guest ELFs

```bash
cd crates/guest && cargo prove build --output-directory elf --elf-name inference-guest
cd crates/aggregator-guest && cargo prove build --output-directory elf --elf-name aggregator-guest
```

### 3. Environment

```dotenv
# Gateway
HOST=0.0.0.0
PORT=8080
CLIENT_URL=http://localhost:3000
DATABASE_URL=postgresql://user:pass@localhost:5432/mugen

# Prover
SP1_PROVER=network
NETWORK_PRIVATE_KEY=0x...
GUEST_ELF_PATH=crates/guest/elf/inference-guest
MODEL_WEIGHTS_PATH=weights/tiny_mlp.bin
PROOFS_DIR=/tmp/mugen-proofs
MAX_CONCURRENT=1
TIMEOUT_SECS=300

# Aggregator
AGG_ELF_PATH=crates/aggregator-guest/elf/aggregator-guest
BATCH_SIZE=10
BATCH_FLUSH_SECS=60

# Settler — HashKey testnet
SETTLER_RPC_URL=https://testnet.hsk.xyz
SETTLER_PRIVATE_KEY=0x...
INFERENCE_VERIFIER_ADDRESS=0x69f77055e9A6e6B34539Db2BD733f9eB07F9f11f

# VeilVault (optional — fee deduction not enforced in MVP)
VEIL_VAULT_ADDRESS=0x47A7EA849d500625aa424bF90a5DF4814895C279

# IPFS
PINATA_JWT=eyJ...
PINATA_GATEWAY_URL=<your-gateway>.mypinata.cloud
```

### 4. Database

```bash
createdb mugen
cargo run -p gateway   # migrations run automatically on startup
```

### 5. Register a model

```bash
curl -X POST http://localhost:8080/v1/models \
  -H "Content-Type: application/json" \
  -d "{
    \"name\": \"tiny_mlp_v1\",
    \"version\": \"0.1.0\",
    \"artifact_b64\": \"$(base64 -i weights/tiny_mlp.bin)\",
    \"input_shape\": [1, 4]
  }"
```

### 6. Run the gateway

```bash
cargo build --release -p gateway
./target/release/gateway
```

### 7. Deploy contracts

```bash
cd contracts/evm

# Deploy InferenceVerifier
forge script script/Deploy.s.sol:Deploy \
  --rpc-url https://testnet.hsk.xyz \
  --private-key $PRIVATE_KEY \
  --broadcast \
  --verify \
  --verifier blockscout \
  --verifier-url https://testnet-explorer.hsk.xyz/api

# Deploy VeilVault
GATEWAY_WALLET_ADDRESS=$(cast wallet address --private-key $PRIVATE_KEY) \
OWNER_ADDRESS=<your-address> \
forge script script/DeployVault.s.sol:DeployVault \
  --rpc-url https://testnet.hsk.xyz \
  --private-key $PRIVATE_KEY \
  --broadcast \
  --verify \
  --verifier blockscout \
  --verifier-url https://testnet-explorer.hsk.xyz/api
```

### 8. Pre-flight checklist

```bash
# Confirm vkey matches current ELF
cargo prove vkey --elf crates/guest/elf/inference-guest
cast call $INFERENCE_VERIFIER_ADDRESS "inferenceVKey()(bytes32)" --rpc-url https://testnet.hsk.xyz

# Confirm settler is whitelisted
cast call $INFERENCE_VERIFIER_ADDRESS "isSettler(address)(bool)" $SETTLER_ADDRESS --rpc-url https://testnet.hsk.xyz

# Confirm model is registered
cast call $INFERENCE_VERIFIER_ADDRESS "isRegisteredModel(bytes32)(bool)" \
  0x91db243a5b7956c0818e930c6abde3a47b14dd47a71928367409535f04aa32ed \
  --rpc-url https://testnet.hsk.xyz

# Confirm VeilVault gateway is set correctly
cast call $VEIL_VAULT_ADDRESS "gateway()(address)" --rpc-url https://testnet.hsk.xyz
```

---

## Model Architecture

`tiny_mlp_v1` is a 4→8→2 MLP (58 parameters) used for MVP demonstration. It is designed to validate the full pipeline end-to-end: inference → SP1 proof → on-chain verification → HashKey attestation.

**Weights layout (232 bytes, flat f32 little-endian):**

```
[0..32]  W1: layer1 weights (4×8)
[32..40] b1: layer1 biases  (8)
[40..56] W2: layer2 weights (8×2)
[56..58] b2: layer2 biases  (2)
```

**Guest program public values (112 bytes):**

```
[0..32]   model_id    = sha256(weights_bytes)
[32..64]  input_hash  = sha256(input_le_bytes)
[64..96]  output_hash = sha256(output_le_bytes)
[96..112] output      = raw f32 logits (2 × 4 bytes)
```

**Attestation hash:** `keccak256(model_id || input_hash || output_hash)` — this is the value emitted on-chain and returned to the client.

---

## License

MIT