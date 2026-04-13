// crates/aggregator/src/lib.rs

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use sp1_sdk::{
    Elf, HashableKey, ProveRequest, Prover, ProverClient, SP1Proof, SP1ProofWithPublicValues,
    SP1Stdin, SP1VerifyingKey,
};
use tracing::{info, warn};

pub struct AggregationInput {
    pub proof: SP1ProofWithPublicValues,
    pub vk: SP1VerifyingKey,
}

#[derive(Debug, Clone)]
pub struct AggregateProofData {
    /// bincode-serialized SP1ProofWithPublicValues — matches what settler.submit() expects.
    pub proof_bytes: Vec<u8>,
    /// Public values committed by the aggregator-guest: [merkle_root(32) | batch_size(4)].
    pub public_values: Vec<u8>,
    /// Individual output_hashes from each inference proof (bytes [64..96] of each pv).
    pub output_hashes: Vec<[u8; 32]>,
    /// Number of proofs aggregated.
    pub batch_size: usize,
}

/// Aggregate N compressed inference proofs into a single Groth16 proof.
///
/// `elf` — bytes of the aggregator-guest ELF (loaded from AGGREGATOR_ELF_PATH by caller).
///
/// Stdin layout — must match aggregator-guest/src/main.rs exactly:
///   1. write::<Vec<[u32;8]>>(&vkeys)
///   2. write::<Vec<Vec<u8>>>(&public_values)
///   3. write_proof(proof, vk) × N  ← consumed by prover, not read by guest
pub async fn aggregate_proofs(
    elf: &[u8],
    inputs: Vec<AggregationInput>,
) -> Result<AggregateProofData> {
    if inputs.is_empty() {
        return Err(anyhow!("no inputs to aggregate"));
    }

    // Validate all proofs are compressed before doing any work
    if inputs
        .iter()
        .any(|i| !matches!(i.proof.proof, SP1Proof::Compressed(_)))
    {
        return Err(anyhow!(
            "all inputs must be compressed proofs (SP1Proof::Compressed)"
        ));
    }

    let batch_size = inputs.len();
    info!("aggregating {batch_size} compressed proofs");

    // Pre-extract public_values and output_hashes — same across all attempts
    let public_values: Vec<Vec<u8>> = inputs
        .iter()
        .map(|i| i.proof.public_values.to_vec())
        .collect();

    let mut output_hashes = Vec::with_capacity(batch_size);
    for pv in &public_values {
        if pv.len() < 96 {
            return Err(anyhow!(
                "public_values too short: expected >= 96 bytes, got {}",
                pv.len()
            ));
        }
        let hash: [u8; 32] = pv[64..96]
            .try_into()
            .map_err(|_| anyhow!("failed to extract output_hash from public_values"))?;
        output_hashes.push(hash);
    }

    let vkeys: Vec<[u32; 8]> = inputs.iter().map(|i| i.vk.hash_u32()).collect();

    let mut last_err = anyhow!("no attempts made");

    for attempt in 1u64..=3 {
        // Rebuild stdin fresh on every attempt — SP1Stdin is not reusable
        let mut stdin = SP1Stdin::new();
        stdin.write::<Vec<[u32; 8]>>(&vkeys);
        stdin.write::<Vec<Vec<u8>>>(&public_values);
        for input in &inputs {
            let SP1Proof::Compressed(proof) = &input.proof.proof else {
                unreachable!(); // already validated above
            };
            stdin.write_proof(*proof.clone(), input.vk.vk.clone());
        }

        let client = tokio::task::spawn(async { ProverClient::builder().network().build().await })
            .await
            .map_err(|e| anyhow!("network client panicked: {e}"))?;

        let agg_pk = match client
            .setup(Elf::Dynamic(elf.to_vec().into()))
            .await
            .map_err(|e| anyhow!("aggregator setup failed: {e}"))
        {
            Ok(pk) => pk,
            Err(e) => {
                warn!("aggregation attempt {attempt}/3 failed at setup: {e}");
                last_err = e;
                tokio::time::sleep(std::time::Duration::from_secs(5 * attempt)).await;
                continue;
            }
        };

        info!("submitting aggregated proof request — attempt {attempt}/3, batch_size={batch_size}");

        let proof = match client
            .prove(&agg_pk, stdin)
            .groth16()
            .await
            .map_err(|e| anyhow!("aggregated Groth16 proof failed: {e}"))
        {
            Ok(p) => p,
            Err(e) => {
                warn!("aggregation attempt {attempt}/3 failed at prove: {e}");
                last_err = e;
                tokio::time::sleep(std::time::Duration::from_secs(5 * attempt)).await;
                continue;
            }
        };

        info!("aggregated Groth16 proof received — batch_size={batch_size}");

        if !matches!(proof.proof, SP1Proof::Groth16(_)) {
            return Err(anyhow!(
                "expected Groth16 proof from aggregation, got a different variant"
            ));
        }

        let public_values_out = proof.public_values.to_vec();
        let proof_bytes = bincode::serialize(&proof)
            .map_err(|e| anyhow!("failed to serialize aggregated proof: {e}"))?;

        return Ok(AggregateProofData {
            proof_bytes,
            public_values: public_values_out,
            output_hashes,
            batch_size,
        });
    }

    Err(last_err)
}

/// Compute the merkle root over a set of output_hashes.
/// MUST match the implementation in aggregator-guest/src/main.rs exactly.
pub fn merkle_root(hashes: &[[u8; 32]]) -> [u8; 32] {
    if hashes.is_empty() {
        return [0u8; 32];
    }
    if hashes.len() == 1 {
        return hashes[0];
    }
    let mut combined = Vec::with_capacity(hashes.len() * 32);
    for h in hashes {
        combined.extend_from_slice(h);
    }
    Sha256::digest(&combined).into()
}
