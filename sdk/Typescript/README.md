# @mugen/sdk

TypeScript SDK for the [Mugen Verifiable Inference Network](https://mugen.xyz).

Submit AI inference jobs, receive ZK proofs, and verify them on StarkNet — in a single function call.

---

## Requirements

- Node.js 18+
- A running Mugen gateway (see [gateway setup](#gateway-setup))

---

## Installation

```bash
# from the mugen/ workspace root
cd sdk
npm install
npm run build
```

To use the SDK in another package within the monorepo, add it as a local dependency:

```json
{
  "dependencies": {
    "@mugen/sdk": "file:../sdk"
  }
}
```

---

## Quick start

```typescript
import { ElenxisClient } from '@mugen/sdk';

const client = new ElenxisClient({
  gatewayUrl: 'http://localhost:8080',
  timeoutMs:  300_000, // 5 min — accounts for L1→L2 StarkNet relay time
});

const result = await client.verifyInference({
  modelId:   'tiny_mlp_v1',
  inputData: [[0.1, 0.2, 0.3, 0.4]],
});

console.log('Job ID:           ', result.jobId);
console.log('Attestation hash: ', result.attestationHash);
console.log('On-chain tx:      ', result.txHash);
console.log('Time taken:       ', result.elapsedMs, 'ms');
```

---

## API

### `new ElenxisClient(config)`

| Option | Type | Default | Description |
|---|---|---|---|
| `gatewayUrl` | `string` | required | Gateway base URL |
| `timeoutMs` | `number` | `300000` | Max wait for job completion (ms). Use ≥300000 — StarkNet L1→L2 relay takes 1–3 min |
| `pollIntervalMs` | `number` | `1000` | Status polling interval (ms) |
| `maxRetries` | `number` | `3` | HTTP retry attempts on failure |

---

### `client.verifyInference(params)`

The primary method. Submits an inference, waits for ZK proof generation, waits for on-chain settlement on StarkNet, and returns the result.

```typescript
const result = await client.verifyInference({
  modelId:   'tiny_mlp_v1',            // registered model identifier
  inputData: [[0.1, 0.2, 0.3, 0.4]],  // 2D input array matching model input shape
});
```

**Returns: `VerifyResult`**

```typescript
{
  jobId:           string;  // UUID of the proof job
  attestationHash: string;  // keccak256-derived proof fingerprint (0x + 64 hex chars)
  txHash:          string;  // Eth Sepolia tx hash from InferenceBridge.verifyAndBridge()
  elapsedMs:       number;  // total wall time from submit to StarkNet confirmation
}
```

---

### `client.submitJob(params)` → `string`

Submit a job without waiting. Returns the `jobId`.

```typescript
const jobId = await client.submitJob({
  modelId:   'tiny_mlp_v1',
  inputData: [[0.1, 0.2, 0.3, 0.4]],
});
```

---

### `client.getJob(jobId)` → `Job`

Get current job status.

```typescript
const job = await client.getJob(jobId);
// job.status: 'queued' | 'running' | 'done' | 'settled' | 'failed'
```

---

### `client.waitForJob(jobId)` → `Job`

Block until a job reaches a terminal state.

```typescript
const job = await client.waitForJob(jobId);
```

---

### `client.getProof(jobId)` → `ProofData`

Fetch the raw ZK proof for a completed job.

```typescript
const proof = await client.getProof(jobId);
// proof.proofHex  — hex-encoded Halo2 KZG proof bytes
// proof.sizeBytes — proof size in bytes
```

---

### `client.healthCheck()` → `boolean`

Returns `true` if the gateway is reachable.

```typescript
const healthy = await client.healthCheck();
```

---

## Error handling

All errors thrown by the SDK are instances of `ElenxisError`.

```typescript
import { ElenxisClient, ElenxisError } from '@mugen/sdk';

try {
  await client.verifyInference({ modelId: 'tiny_mlp_v1', inputData: [[0.1]] });
} catch (err) {
  if (err instanceof ElenxisError) {
    switch (err.code) {
      case 'TIMEOUT':
        console.error('Job timed out — increase timeoutMs to ≥300000 for StarkNet');
        break;
      case 'JOB_FAILED':
        console.error('Proof generation or settlement failed:', err.message);
        break;
      case 'SUBMIT_FAILED':
        console.error('Could not reach gateway:', err.message);
        break;
    }
  }
}
```

**Error codes:**

| Code | Description |
|---|---|
| `SUBMIT_FAILED` | Job submission request failed |
| `POLL_FAILED` | Status polling request failed |
| `JOB_FAILED` | Proof generation or on-chain settlement failed |
| `TIMEOUT` | Job did not complete within `timeoutMs` |
| `PROOF_FETCH_FAILED` | Could not retrieve proof bytes |
| `NETWORK_ERROR` | Unclassified network error |

---

## Advanced usage

### Manual submit + poll

```typescript
// Submit without blocking
const jobId = await client.submitJob({
  modelId:   'tiny_mlp_v1',
  inputData: [[0.1, 0.2, 0.3, 0.4]],
});

console.log('Job submitted:', jobId);

// Do other work...

// Then wait for completion
const job = await client.waitForJob(jobId);
console.log('Settled, tx:', job.txHash);
```

### Run the e2e test

```bash
cd sdk
GATEWAY_URL=http://localhost:8080 TIMEOUT_MS=300000 npx tsx e2e_verify.ts
```

---

## Gateway setup

The SDK communicates with the Mugen gateway. To run locally:

```bash
# from mugen/ root
cargo run -p gateway
```

The gateway defaults to `http://0.0.0.0:8080`.

---

## Development

```bash
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

## Project structure

```
sdk/
├── src/
│   ├── index.ts       — public exports
│   ├── client.ts      — ElenxisClient (main class)
│   ├── types.ts       — all TypeScript types
│   ├── errors.ts      — ElenxisError class
│   ├── http.ts        — Axios client with retry logic
│   ├── poller.ts      — job status polling
│   └── client.test.ts — full test suite
├── e2e_verify.ts      — live end-to-end test
├── package.json
├── tsconfig.json
└── jest.config.js
```