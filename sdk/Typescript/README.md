# @mugen-ai/sdk

TypeScript SDK for the Mugen Verifiable Inference Network.

Submit AI inference jobs, receive ZK proofs, and verify them on HashKey Chain — in a single function call.

---

## Requirements

- Node.js 18+
- A running Mugen gateway (see [gateway setup](#gateway-setup))

---

## Installation

```bash
npm install @mugen-ai/sdk
```

To use within the monorepo as a local dependency:

```json
{
  "dependencies": {
    "@mugen-ai/sdk": "file:../sdk/Typescript"
  }
}
```

---

## Quick Start

```typescript
import { VeilClient } from '@mugen-ai/sdk'

const client = new VeilClient({
  gatewayUrl:    'https://your-gateway.xyz',
  timeoutMs:     600_000,
  walletAddress: '0xYourWallet',  // required if gateway has VAULT_ADDRESS configured
})

const job = await client.verifyInference({
  modelId:   'polymarket_mlp_v1',
  inputData: [[0.6, 0.4, 12000, 0.2]],
})

console.log('Job ID:           ', job.jobId)
console.log('Attestation hash: ', job.attestationHash) // keccak256(model_id||input_hash||output_hash)
console.log('On-chain tx:      ', job.txHash)          // HashKey testnet settlement tx
console.log('Time taken:       ', job.elapsedMs, 'ms')
```

---

## API

### `new VeilClient(config)`

| Option | Type | Default | Description |
|---|---|---|---|
| `gatewayUrl` | `string` | required | Mugen gateway base URL |
| `timeoutMs` | `number` | `600000` | Max wait for job completion (ms). Use ≥600000 — SP1 proving + batch aggregation takes 2–5 min |
| `pollIntervalMs` | `number` | `1000` | Status polling interval (ms) |
| `maxRetries` | `number` | `3` | HTTP retry attempts on failure |
| `walletAddress` | `string` | `undefined` | Wallet address for VeilVault fee deduction. Required when the gateway has `VAULT_ADDRESS` configured. Can be overridden per-call in `submitJob`/`verifyInference` params. |

---

### `client.verifyInference(params)` → `VerifyResult`

The primary method. Submits an inference job, waits for SP1 ZK proof generation (phase 1 compressed ~60s), waits for batch aggregation and on-chain settlement on HashKey testnet, and returns the result.

```typescript
const job = await client.verifyInference({
  modelId:       'polymarket_mlp_v1',       // registered model identifier
  inputData:     [[0.6, 0.4, 12000, 0.2]], // 2D input array matching model input shape
  walletAddress: '0xYourWallet',            // overrides config-level walletAddress if set
})
```

**Returns: `VerifyResult`**

```typescript
{
  jobId:           string  // UUID of the proof job
  attestationHash: string  // keccak256(model_id||input_hash||output_hash) — 0x + 64 hex chars
  txHash:          string  // HashKey testnet settlement tx hash
  elapsedMs:       number  // total wall time from submit to on-chain confirmation
}
```

The `attestationHash` is the cryptographic fingerprint of the inference — it binds a specific model, specific input, and specific output together permanently. It is queryable on HashKey testnet via `InferenceVerifier.isVerified(outputHash)`.

---

### `client.submitJob(params)` → `string`

Submit a job without waiting. Returns the `jobId`.

```typescript
const jobId = await client.submitJob({
  modelId:       'tiny_mlp_v1',
  inputData:     [[0.1, 0.2, 0.3, 0.4]],
  walletAddress: '0xYourWallet',  // optional — overrides config-level walletAddress
})
```

---

### `client.getJob(jobId)` → `Job`

Get current job status.

```typescript
const job = await client.getJob(jobId)
// job.status: 'queued' | 'running' | 'proving' | 'done' | 'settled' | 'failed'
// job.attestationHash — available once status is 'proving' (phase 1 complete, ~60s)
// job.txHash          — available once status is 'settled'
```

---

### `client.waitForJob(jobId)` → `Job`

Block until a job reaches a terminal state (`settled` or `failed`).

```typescript
const job = await client.waitForJob(jobId)
if (job.status === 'settled') {
  console.log('tx_hash:', job.txHash)
}
```

---

### `client.getProof(jobId)` → `ProofData`

Fetch attestation info for a completed job. Proofs are batched — individual Groth16 bytes are not exposed per-job. Use the `txHash` to look up the batch on HashKey testnet.

```typescript
const proof = await client.getProof(jobId)
// proof.attestationHash — keccak256 fingerprint
// proof.status          — 'compressed' | 'settled'
```

---

### `client.healthCheck()` → `boolean`

Returns `true` if the gateway is reachable and healthy.

```typescript
const healthy = await client.healthCheck()
```

---

## Job Status Lifecycle
| Status | Meaning |
|---|---|
| `queued` | Job accepted, waiting for SP1 prover slot |
| `running` | SP1 prover is executing the inference inside the zkVM |
| `proving` | Phase 1 complete — compressed STARK proof ready, `attestationHash` available |
| `done` | Proof queued in batch collector, awaiting aggregation |
| `settled` | Aggregated Groth16 proof verified on HashKey testnet, `txHash` available |
| `failed` | Proving or settlement failed |

The `attestationHash` is returned as soon as status reaches `proving` (~60s). You do not need to wait for full settlement to use the attestation.

> **Fee note:** If the gateway has `VAULT_ADDRESS` configured, `walletAddress` must be provided
> either at client construction or per-call. The wallet must have sufficient HSK balance in the
> VeilVault contract. The gateway returns HTTP 402 with `"insufficient VeilVault balance"` if the
> balance check fails. If `VAULT_ADDRESS` is not set on the gateway, proofs are free and
> `walletAddress` is ignored.

---

## Error Handling

All SDK errors are instances of `VeilError`.

```typescript
import { VeilClient, VeilError } from '@mugen-ai/sdk'

try {
  await client.verifyInference({
    modelId:       'tiny_mlp_v1',
    inputData:     [[0.1, 0.2, 0.3, 0.4]],
    walletAddress: '0xYourWallet',
  })
} catch (err) {
  if (err instanceof VeilError) {
    switch (err.code) {
      case 'TIMEOUT':
        console.error('Job timed out — increase timeoutMs (SP1 proving takes 2–5 min)')
        break
      case 'JOB_FAILED':
        console.error('Proof generation or settlement failed:', err.message)
        break
      case 'SUBMIT_FAILED':
        console.error('Could not reach gateway or insufficient vault balance:', err.message)
        break
    }
  }
}
```

**Error codes:**

| Code | Description |
|---|---|
| `SUBMIT_FAILED` | Job submission failed — if vault is enabled and wallet has no balance, gateway returns HTTP 402 |
| `POLL_FAILED` | Status polling request failed |
| `JOB_FAILED` | Proof generation or on-chain settlement failed |
| `TIMEOUT` | Job did not complete within `timeoutMs` |
| `PROOF_FETCH_FAILED` | Could not retrieve proof data |
| `NETWORK_ERROR` | Unclassified network error |

---

## Advanced Usage

### Submit and poll manually

```typescript
// Submit without blocking
const jobId = await client.submitJob({
  modelId:       'polymarket_mlp_v1',
  inputData:     [[0.6, 0.4, 12000, 0.2]],
  walletAddress: '0xYourWallet',
})

console.log('Job submitted:', jobId)

// Attestation hash available after ~60s (phase 1)
let job = await client.getJob(jobId)
while (job.status === 'queued' || job.status === 'running') {
  await new Promise(r => setTimeout(r, 2000))
  job = await client.getJob(jobId)
}

if (job.attestationHash) {
  console.log('Proof attested:', job.attestationHash)
  // You can act on the attestation now — settlement continues in background
}

// Wait for full on-chain settlement
const settled = await client.waitForJob(jobId)
console.log('Settled, tx:', settled.txHash)
```

### Check on-chain via cast

After settlement, verify directly on HashKey testnet:

```bash
# Check if output hash is verified on-chain
cast call 0x69f77055e9A6e6B34539Db2BD733f9eB07F9f11f \
  "isVerified(bytes32)(bool)" \
  <output_hash> \
  --rpc-url https://testnet.hsk.xyz
```

### Run the e2e test

```bash
cd sdk/Typescript
GATEWAY_URL=http://localhost:8080 WALLET_ADDRESS=0xYourWallet TIMEOUT_MS=600000 npx tsx e2e_verify.ts
```

---

## Gateway Setup

The SDK communicates with the Mugen gateway. To run locally:

```bash
# from mugen/ root
SP1_PROVER=network cargo run -p gateway
```

The gateway defaults to `http://0.0.0.0:8080`. Set `CLIENT_URL=http://localhost:3000` if the
frontend is also running locally.

---

## Development

```bash
cd sdk/Typescript

# Install dependencies
npm install

# Build
npm run build

# Run tests
npm test

# Watch mode
npm run dev
```

---

## Project Structure

```
sdk/Typescript/
├── src/
│   ├── index.ts       — public exports
│   ├── client.ts      — VeilClient (main class)
│   ├── types.ts       — TypeScript types
│   ├── errors.ts      — VeilError class
│   ├── http.ts        — Axios client with retry logic
│   ├── poller.ts      — job status polling loop
│   └── client.test.ts — unit test suite
├── e2e_verify.ts      — live end-to-end test against a running gateway
├── package.json
├── tsconfig.json
└── jest.config.js
```

---

## See Also

- [Mugen Gateway README](../README.md) — full architecture, contract addresses, deployment guide
- [Rust SDK](../Rust/README.md) — `veil-sdk` Rust crate
- [Polymarket Veil Agent](https://github.com/OkoliEvans/polymarket-ai-agent/blob/main/README.md) — example consumer built on this SDK