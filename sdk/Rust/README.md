# veil-sdk

Rust client for the Mugen Veil verifiable inference network.

Submit ML inference jobs to the Mugen gateway, receive SP1 ZK proofs, and verify them on HashKey Chain — from idiomatic async Rust.

---

## Requirements

- Rust 1.75+
- Tokio async runtime
- A running Mugen gateway

---

## Installation

```toml
[dependencies]
veil-sdk = { path = "sdk/Rust" }
tokio    = { version = "1", features = ["full"] }
```

---

## Quick Start

```rust
use veil_sdk::VeilClient;

#[tokio::main]
async fn main() -> veil_sdk::error::Result<()> {
    let client = VeilClient::builder()
        .base_url("https://your-gateway.xyz")
        .build()?;

    let result = client
        .verify_inference(
            "polymarket_mlp_v1",
            vec![vec![0.6, 0.4, 12000.0, 0.2]],
            Some("0xYourWallet".to_string()), // required if gateway has VAULT_ADDRESS set
        )
        .await?;

    println!("status:           {}", result.status);
    println!("attestation_hash: {:?}", result.attestation_hash);
    println!("tx_hash:          {:?}", result.tx_hash);
    println!("elapsed:          {}ms", result.elapsed_ms);

    Ok(())
}
```

---

## API

### `VeilClient::builder()` → `VeilClientBuilder`

Construct a client using the builder pattern.

```rust
use veil_sdk::VeilClient;
use std::time::Duration;

let client = VeilClient::builder()
    .base_url("http://localhost:8080")
    .timeout(Duration::from_secs(600))
    .poll_interval(Duration::from_secs(3))
    .build()?;
```

**Builder options:**

| Method | Type | Default | Description |
|---|---|---|---|
| `base_url` | `&str` | `"http://localhost:8080"` | Gateway base URL. Trailing slashes are stripped. |
| `timeout` | `Duration` | `600s` | Max wall-clock time for `verify_inference` to reach a terminal state. SP1 proving + batch aggregation takes 2–5 min. |
| `poll_interval` | `Duration` | `3s` | How often to poll `GET /v1/jobs/{id}` while waiting. |

`VeilClient` is cheap to clone — the underlying `reqwest::Client` uses an `Arc` internally and
shares the connection pool across clones.

---

### `verify_inference` → `VerifyResult`

The primary high-level method. Submits an inference job and blocks until it reaches a terminal
state (`settled` or `failed`), polling at the configured interval.

```rust
let result = client
    .verify_inference(
        "polymarket_mlp_v1",
        vec![vec![0.6, 0.4, 12000.0, 0.2]],
        Some("0xYourWallet".to_string()), // required if gateway has VAULT_ADDRESS set
    )
    .await?;
```

**Returns `VerifyResult`:**

```rust
pub struct VerifyResult {
    pub job_id:           String,           // UUID of the proof job
    pub status:           JobStatus,        // terminal status
    pub attestation_hash: Option<String>,   // keccak256(model_id||input_hash||output_hash)
    pub tx_hash:          Option<String>,   // HashKey testnet settlement tx hash
    pub elapsed_ms:       u64,              // total wall time in milliseconds
}
```

The `attestation_hash` is the cryptographic fingerprint binding the model, input, and output
together permanently. It is queryable on-chain via `InferenceVerifier.isVerified(outputHash)`.

**Errors:**

| Error | Condition |
|---|---|
| `VeilError::Timeout` | Polling exceeded configured `timeout` |
| `VeilError::JobFailed` | Gateway reported the job as failed |
| `VeilError::Api { status: 402, .. }` | Insufficient VeilVault balance — deposit HSK before submitting |
| `VeilError::Api` | Gateway returned a non-2xx HTTP response |
| `VeilError::Http` | Network-level failure |

---

### `submit_job` → `String`

Submit a job without waiting. Returns the `job_id` immediately.

```rust
let job_id = client
    .submit_job(
        "tiny_mlp_v1",
        vec![vec![0.1, 0.2, 0.3, 0.4]],
        Some("0xYourWallet".to_string()), // required if gateway has VAULT_ADDRESS set
    )
    .await?;
```

Pass `None` only when the gateway has no vault configured (proofs are free).

```rust
let job_id = client
    .submit_job("tiny_mlp_v1", vec![vec![0.1, 0.2, 0.3, 0.4]], None)
    .await?;
```

---

### `get_job` → `Job`

Poll the current status of a job.

```rust
let job = client.get_job(&job_id).await?;
// job.status:           JobStatus
// job.attestation_hash: Option<String> — available once status is Proving
// job.tx_hash:          Option<String> — available once status is Settled
// job.reason:           Option<String> — populated on failure
```

---

### `get_proof` → `Proof`

Fetch attestation info for a completed job. Returns `HTTP 202` (surfaced as
`VeilError::Api { status: 202, .. }`) if the job is not yet complete.

```rust
let proof = client.get_proof(&job_id).await?;
// proof.attestation_hash: String
// proof.status:           String
```

---

### `register_model` → `RegisterModelResponse`

Register a model with the gateway — pins artifact to IPFS and records on-chain.

```rust
use veil_sdk::RegisterModelRequest;

let resp = client.register_model(RegisterModelRequest {
    name:         "my_model".into(),
    version:      "1.0.0".into(),
    artifact_b64: base64_encoded_weights,
    input_shape:  vec![1, 4],
}).await?;

println!("ipfs_cid:      {}", resp.ipfs_cid);
println!("on_chain_hash: {}", resp.on_chain_hash);
```

---

### `health_check` → `Health`

```rust
let health = client.health_check().await?;
if health.is_healthy() {
    println!("gateway v{} is online", health.version);
}
```

---

## Job Status Lifecycle

```
queued → running → proving → done → settled
                                  ↘ failed
```

| `JobStatus` | Meaning |
|---|---|
| `Queued` | Job accepted, waiting for SP1 prover slot |
| `Running` | SP1 prover executing inference inside the zkVM |
| `Proving` | Phase 1 complete — compressed STARK proof ready, `attestation_hash` available |
| `Done` | Proof queued in batch collector awaiting aggregation |
| `Settled` | Aggregated Groth16 proof verified on HashKey testnet, `tx_hash` available |
| `Failed` | Proving or settlement failed |

`JobStatus::is_terminal()` returns `true` for `Settled`, `Done`, and `Failed`.

The `attestation_hash` is available as soon as status reaches `Proving` (~60s). You do not need
to wait for full settlement to use it.

> **Fee note:** If the gateway has `VAULT_ADDRESS` configured, `wallet_address` must be
> `Some("0x...")` and the wallet must have sufficient HSK balance in the VeilVault contract.
> The gateway returns HTTP 402 if the balance check fails. Pass `None` only when the gateway
> has no vault configured — proofs are then free.

---

## Error Handling

All SDK errors implement `std::error::Error` and are defined in `veil_sdk::error::VeilError`.

```rust
use veil_sdk::{VeilClient, VeilError};

match client
    .verify_inference(
        "tiny_mlp_v1",
        vec![vec![0.1, 0.2, 0.3, 0.4]],
        Some("0xYourWallet".to_string()),
    )
    .await
{
    Ok(result) => println!("settled: {:?}", result.tx_hash),
    Err(VeilError::Timeout { job_id, elapsed_ms, last_status }) => {
        eprintln!("job {job_id} timed out after {elapsed_ms}ms (last status: {last_status})");
        eprintln!("increase timeout — SP1 proving + aggregation takes 2–5 min");
    }
    Err(VeilError::JobFailed { job_id, reason }) => {
        eprintln!("job {job_id} failed: {reason:?}");
    }
    Err(VeilError::Api { status: 402, message }) => {
        eprintln!("insufficient VeilVault balance — deposit HSK: {message}");
    }
    Err(VeilError::Api { status, message }) => {
        eprintln!("gateway error {status}: {message}");
    }
    Err(e) => eprintln!("unexpected error: {e}"),
}
```

**Error variants:**

| Variant | Fields | Condition |
|---|---|---|
| `InvalidUrl` | `String` | Base URL could not be parsed |
| `Http` | `reqwest::Error` | Network-level failure |
| `Api` | `status: u16, message: String` | Gateway returned non-2xx — status 402 means insufficient vault balance |
| `Timeout` | `job_id, elapsed_ms, last_status` | Polling exceeded timeout |
| `JobFailed` | `job_id, reason: Option<String>` | Gateway reported job failed |

---

## Advanced Usage

### Submit and poll manually

```rust
use veil_sdk::VeilClient;
use std::time::Duration;

#[tokio::main]
async fn main() -> veil_sdk::error::Result<()> {
    let client = VeilClient::builder()
        .base_url("http://localhost:8080")
        .build()?;

    // Submit without blocking
    let job_id = client
        .submit_job(
            "polymarket_mlp_v1",
            vec![vec![0.6, 0.4, 12000.0, 0.2]],
            Some("0xYourWallet".to_string()),
        )
        .await?;

    println!("job submitted: {job_id}");

    // Poll manually — act on attestation as soon as phase 1 completes
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let job = client.get_job(&job_id).await?;

        println!("status: {}", job.status);

        if let Some(hash) = &job.attestation_hash {
            println!("attestation available: {hash}");
            // You can act on the attestation now — settlement continues in background
        }

        if job.status.is_terminal() {
            println!("settled tx: {:?}", job.tx_hash);
            break;
        }
    }

    Ok(())
}
```

### Concurrent jobs

`VeilClient` is `Clone` — share a single client across multiple tasks:

```rust
use veil_sdk::VeilClient;
use std::sync::Arc;

let client = Arc::new(
    VeilClient::builder()
        .base_url("http://localhost:8080")
        .build()?
);

let mut handles = Vec::new();

for i in 0..5 {
    let c = Arc::clone(&client);
    handles.push(tokio::spawn(async move {
        c.verify_inference(
            "tiny_mlp_v1",
            vec![vec![i as f64 * 0.1, 0.2, 0.3, 0.4]],
            Some("0xYourWallet".to_string()),
        )
        .await
    }));
}

for handle in handles {
    let result = handle.await??;
    println!("attestation: {:?}", result.attestation_hash);
}
```

### Tracing / logging

`veil-sdk` emits structured logs via the `tracing` crate. Enable them with `tracing_subscriber`:

```rust
tracing_subscriber::fmt()
    .with_env_filter("veil_sdk=debug")
    .init();
```

---

## Run the e2e test

```bash
cd sdk/Rust
GATEWAY_URL=http://localhost:8080 WALLET_ADDRESS=0xYourWallet cargo run --bin e2e_verify
```

Or as an ignored integration test:

```bash
GATEWAY_URL=http://localhost:8080 WALLET_ADDRESS=0xYourWallet cargo test --test e2e -- --ignored
```

---

## Project Structure

```
sdk/Rust/
├── src/
│   ├── lib.rs          — crate root, public re-exports, module-level docs
│   ├── client.rs       — VeilClient + VeilClientBuilder
│   ├── types.rs        — Job, JobStatus, VerifyResult, Health, Proof, RegisterModelRequest
│   ├── error.rs        — VeilError enum, Result alias
│   └── e2e_verify.rs   — live end-to-end binary against a running gateway
├── Cargo.toml
└── README.md
```

---

## On-chain Verification

After settlement, verify directly on HashKey testnet:

```bash
# Check if output hash is verified on-chain
cast call 0x69f77055e9A6e6B34539Db2BD733f9eB07F9f11f \
  "isVerified(bytes32)(bool)" \
  <output_hash_from_attestation> \
  --rpc-url https://testnet.hsk.xyz
```

The `attestation_hash` returned by the SDK is `keccak256(model_id || input_hash || output_hash)`.
The `output_hash` component (`pv[64..96]`) is the on-chain lookup key stored in
`InferenceVerifier.isVerified`.

---

## See Also

- [Mugen Gateway README](../../README.md) — full architecture, contract addresses, deployment guide
- [TypeScript SDK](../Typescript/README.md) — `@mugen-ai/sdk` npm package
- [Polymarket Veil Agent](https://github.com/OkoliEvans/polymarket-ai-agent/blob/main/README.md) — example consumer built on this SDK

---

## License

MIT