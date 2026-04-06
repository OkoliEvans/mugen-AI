// crates/prover_manager/src/proving.rs

use alloy_primitives::keccak256;
use anyhow::Result;
use sha2::{Digest, Sha256};
use sp1_sdk::blocking::{ProveRequest, Prover, ProverClient};
use sp1_sdk::{Elf, SP1ProofWithPublicValues, SP1Stdin};
use tracing::info;

use crate::job::ProofData;
use crate::manager::ProverConfig;

fn build_stdin(
    weight_bytes: &[u8],
    input_data: Vec<Vec<f64>>,
    model_id: &[u8; 32],
) -> Result<SP1Stdin> {
    let weights: Vec<f32> = weight_bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();

    let input: Vec<f32> = input_data.into_iter().flatten().map(|v| v as f32).collect();

    let input_bytes =
        rkyv::to_bytes::<_, 256>(&input).map_err(|e| anyhow::anyhow!("rkyv input: {e}"))?;
    let weights_bytes =
        rkyv::to_bytes::<_, 256>(&weights).map_err(|e| anyhow::anyhow!("rkyv weights: {e}"))?;

    let mut stdin = SP1Stdin::new();
    stdin.write_slice(&input_bytes);
    stdin.write_slice(&weights_bytes);
    stdin.write(model_id);
    Ok(stdin)
}

// Single extract function used by ALL phases (compressed + groth16, mock + network).
//
// FIX 1: returns Result<ProofData> not a tuple — satisfies both _inner and network fn signatures.
// FIX 2: attestation_hash = keccak256(model_id || input_hash || output_hash)
//         matches InferenceVerifier.sol. Was wrongly keccak256(public_values).
fn extract_proof_data(proof: &SP1ProofWithPublicValues, model_id: [u8; 32]) -> Result<ProofData> {
    let pv = proof.public_values.as_slice();
    if pv.len() < 96 {
        return Err(anyhow::anyhow!(
            "public values too short: got {} bytes, expected >= 96",
            pv.len()
        ));
    }

    let model_id_out: [u8; 32] = pv[0..32].try_into()?;
    let input_hash:   [u8; 32] = pv[32..64].try_into()?;
    let output_hash:  [u8; 32] = pv[64..96].try_into()?;

    if model_id_out != model_id {
        return Err(anyhow::anyhow!(
            "model_id mismatch: guest committed {}, prover computed {}",
            hex::encode(model_id_out),
            hex::encode(model_id),
        ));
    }

    // keccak256(model_id || input_hash || output_hash) — matches InferenceVerifier.sol
    let mut preimage = Vec::with_capacity(96);
    preimage.extend_from_slice(&model_id_out);
    preimage.extend_from_slice(&input_hash);
    preimage.extend_from_slice(&output_hash);
    let attestation_hash: [u8; 32] = keccak256(&preimage).into();

    let proof_bytes =
        bincode::serialize(proof).map_err(|e| anyhow::anyhow!("proof serialize: {e}"))?;

    Ok(ProofData {
        proof_bytes,
        attestation_hash,
        model_id: model_id_out,
        input_hash,
        output_hash,
    })
}

// ── Phase 1 — Mock/cpu path (blocking) ───────────────────────────────────────
// Returns ProofData with attestation_hash. proof_bytes may be empty for mock.
// No tx hash — settlement is not triggered from phase 1.

fn prove_compressed_inner(
    elf_bytes: Vec<u8>,
    weight_bytes: Vec<u8>,
    job_id: &str,
    input_data: Vec<Vec<f64>>,
) -> Result<ProofData> {
    let model_id: [u8; 32] = Sha256::digest(&weight_bytes).into();
    let stdin = build_stdin(&weight_bytes, input_data, &model_id)?;

    let client = ProverClient::from_env();
    let pk = client.setup(Elf::Dynamic(elf_bytes.into()))?;

    info!(%job_id, "phase 1 — submitting compressed proof");
    let proof = client.prove(&pk, stdin).compressed().run()?;
    info!(%job_id, "phase 1 — compressed proof received");

    // FIX 3: was calling undefined extract_public_hashes_only — use extract_proof_data
    extract_proof_data(&proof, model_id)
}

// ── Phase 2 — Mock/cpu path (blocking) ───────────────────────────────────────
// Returns ProofData with real proof_bytes. Settlement fires after this returns.

fn prove_groth16_inner(
    elf_bytes: Vec<u8>,
    weight_bytes: Vec<u8>,
    job_id: &str,
    input_data: Vec<Vec<f64>>,
) -> Result<ProofData> {
    let model_id: [u8; 32] = Sha256::digest(&weight_bytes).into();
    let stdin = build_stdin(&weight_bytes, input_data, &model_id)?;

    let client = ProverClient::from_env();
    let pk = client.setup(Elf::Dynamic(elf_bytes.into()))?;

    info!(%job_id, "phase 2 — submitting groth16 proof");
    let proof = client.prove(&pk, stdin).groth16().run()?;
    info!(%job_id, "phase 2 — groth16 proof received");

    extract_proof_data(&proof, model_id)
}

// ── Phase 1 — Network path (async) ───────────────────────────────────────────

async fn prove_compressed_network(
    elf_bytes: Vec<u8>,
    weight_bytes: Vec<u8>,
    job_id: &str,
    input_data: Vec<Vec<f64>>,
) -> Result<ProofData> {
    use sp1_sdk::{ProveRequest, Prover, ProverClient as AsyncClient};

    let model_id: [u8; 32] = Sha256::digest(&weight_bytes).into();
    let stdin = build_stdin(&weight_bytes, input_data, &model_id)?;
    let elf = Elf::Dynamic(elf_bytes.into());

    let client = tokio::task::spawn(async { AsyncClient::builder().network().build().await })
        .await
        .map_err(|e| anyhow::anyhow!("network client panicked: {e}"))?;
    let pk = client
        .setup(elf.clone())
        .await
        .map_err(|e| anyhow::anyhow!("setup failed: {e}"))?;

    info!(%job_id, "phase 1 — submitting compressed proof (network)");
    let proof = client
        .prove(&pk, stdin)
        .compressed()
        .await
        .map_err(|e| anyhow::anyhow!("compressed prove failed: {e}"))?;
    info!(%job_id, "phase 1 — compressed proof received (network)");

    extract_proof_data(&proof, model_id)
}

// ── Phase 2 — Network path (async) ───────────────────────────────────────────

async fn prove_groth16_network(
    elf_bytes: Vec<u8>,
    weight_bytes: Vec<u8>,
    job_id: &str,
    input_data: Vec<Vec<f64>>,
) -> Result<ProofData> {
    use sp1_sdk::{ProveRequest, Prover, ProverClient as AsyncClient};

    let model_id: [u8; 32] = Sha256::digest(&weight_bytes).into();
    let stdin = build_stdin(&weight_bytes, input_data, &model_id)?;
    let elf = Elf::Dynamic(elf_bytes.into());

    let client = tokio::task::spawn(async { AsyncClient::builder().network().build().await })
        .await
        .map_err(|e| anyhow::anyhow!("network client panicked: {e}"))?;
    let pk = client
        .setup(elf.clone())
        .await
        .map_err(|e| anyhow::anyhow!("setup failed: {e}"))?;

    info!(%job_id, "phase 2 — submitting groth16 proof (network)");
    let proof = client
        .prove(&pk, stdin)
        .groth16()
        .await
        .map_err(|e| anyhow::anyhow!("groth16 prove failed: {e}"))?;
    info!(%job_id, "phase 2 — groth16 proof received (network)");

    extract_proof_data(&proof, model_id)
}

// ── Public API — branches on SP1_PROVER ──────────────────────────────────────

pub async fn prove_compressed(
    config: &ProverConfig,
    job_id: &str,
    input_data: Vec<Vec<f64>>,
) -> Result<ProofData> {
    let elf_bytes = tokio::fs::read(&config.guest_elf_path)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read guest ELF: {e}"))?;
    let weight_bytes = tokio::fs::read(&config.weights_path)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read weights: {e}"))?;

    if std::env::var("SP1_PROVER").as_deref() == Ok("network") {
        prove_compressed_network(elf_bytes, weight_bytes, job_id, input_data).await
    } else {
        let job_id = job_id.to_string();
        tokio::task::spawn_blocking(move || {
            prove_compressed_inner(elf_bytes, weight_bytes, &job_id, input_data)
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking join error: {e}"))?
    }
}

pub async fn prove_groth16(
    config: &ProverConfig,
    job_id: &str,
    input_data: Vec<Vec<f64>>,
) -> Result<ProofData> {
    let elf_bytes = tokio::fs::read(&config.guest_elf_path)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read guest ELF: {e}"))?;
    let weight_bytes = tokio::fs::read(&config.weights_path)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read weights: {e}"))?;

    if std::env::var("SP1_PROVER").as_deref() == Ok("network") {
        prove_groth16_network(elf_bytes, weight_bytes, job_id, input_data).await
    } else {
        let job_id = job_id.to_string();
        tokio::task::spawn_blocking(move || {
            prove_groth16_inner(elf_bytes, weight_bytes, &job_id, input_data)
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking join error: {e}"))?
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const GUEST_ELF: &[u8] = include_bytes!(
        "/Users/MAC/mugen/target/elf-compilation/riscv64im-succinct-zkvm-elf/release/inference-guest"
    );

    fn mock_weight_bytes() -> Vec<u8> {
        let weights = vec![0.1f32; tiny_mlp::WEIGHTS_LEN];
        weights.iter().flat_map(|f: &f32| f.to_le_bytes()).collect()
    }

    async fn prove_mock(input_data: Vec<Vec<f64>>) -> Result<ProofData> {
        std::env::set_var("SP1_PROVER", "mock");
        let weight_bytes = mock_weight_bytes();
        let job_id = "test-job".to_string();
        tokio::task::spawn_blocking(move || {
            prove_compressed_inner(GUEST_ELF.to_vec(), weight_bytes, &job_id, input_data)
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking join error: {e}"))?
    }

    #[tokio::test]
    async fn mock_proof_round_trip() {
        let result = prove_mock(vec![vec![0.5, 0.3, 0.8, 0.1]]).await;
        assert!(result.is_ok(), "mock proof failed: {:?}", result.err());
        let data = result.unwrap();
        assert_eq!(data.attestation_hash.len(), 32);
        assert!(!data.proof_bytes.is_empty());
    }

    #[tokio::test]
    async fn attestation_hash_is_deterministic() {
        let d1 = prove_mock(vec![vec![0.5, 0.3, 0.8, 0.1]])
            .await
            .expect("first proof failed");
        let d2 = prove_mock(vec![vec![0.5, 0.3, 0.8, 0.1]])
            .await
            .expect("second proof failed");
        assert_eq!(d1.attestation_hash, d2.attestation_hash);
    }

    /// Real two-phase network test — costs $PROVE tokens.
    /// Run with:
    ///   SP1_PROVER=network SP1_PRIVATE_KEY=0x... \
    ///     cargo test -p prover-manager -- --ignored prove_real --nocapture
    #[tokio::test]
    #[ignore]
    async fn prove_real() {
        assert_eq!(
            std::env::var("SP1_PROVER").as_deref().unwrap_or(""),
            "network",
            "SP1_PROVER must be set to 'network' in the shell before running this test"
        );
        assert!(
            std::env::var("SP1_PRIVATE_KEY")
                .map(|k| k.starts_with("0x") && k.len() > 10)
                .unwrap_or(false),
            "SP1_PRIVATE_KEY must be set to your Succinct network key"
        );

        let weight_bytes = mock_weight_bytes();
        let input = vec![vec![0.5, 0.3, 0.8, 0.1]];

        // Phase 1 — compressed (attestation_hash available early, no tx hash)
        let compressed = prove_compressed_network(
            GUEST_ELF.to_vec(),
            weight_bytes.clone(),
            "real-phase1",
            input.clone(),
        )
        .await
        .expect("phase 1 network proof failed");

        println!("phase 1 attestation_hash: 0x{}", hex::encode(compressed.attestation_hash));
        println!("phase 1 proof_bytes size: {} bytes", compressed.proof_bytes.len());

        // Phase 2 — groth16 (real proof bytes, settlement fires after this)
        let groth16 = prove_groth16_network(
            GUEST_ELF.to_vec(),
            weight_bytes.clone(),
            "real-phase2",
            input.clone(),
        )
        .await
        .expect("phase 2 network proof failed");

        println!("phase 2 attestation_hash: 0x{}", hex::encode(groth16.attestation_hash));
        println!("phase 2 proof_bytes size: {} bytes", groth16.proof_bytes.len());

        assert_eq!(
            compressed.attestation_hash,
            groth16.attestation_hash,
            "attestation_hash mismatch between phases — guest is non-deterministic"
        );
    }
}