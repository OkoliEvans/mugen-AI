// crates/prover-manager/src/job.rs

use serde::{Deserialize, Serialize};
use sp1_sdk::{SP1ProofWithPublicValues, SP1VerifyingKey};

/// Job lifecycle states.
///
/// State machine:
///   Queued → Running → Compressed (terminal success)
///                    ↘ Failed    (terminal failure, at any transition)
///
/// ## Why there is no Done state
///
/// Previously the pipeline ran Groth16 per job (Done = Groth16 written to disk).
/// That is the opposite of industry practice. SP1 docs and all production zkVM
/// deployments (OP-Succinct, SP1-Reth, RISC Zero) run compressed proofs per job
/// and batch them into a single Groth16 for on-chain settlement.
///
/// Compressed is now the terminal success state. The aggregator crate collects
/// N CompressedData entries, runs one Groth16, and settles once on-chain.
#[derive(Debug, Clone)]
pub enum JobState {
    /// Job is queued, not yet picked up by a worker.
    Queued,
    /// Worker acquired the semaphore slot and is actively proving.
    Running,
    /// Terminal success: compressed STARK proof ready.
    ///
    /// attestation_hash is available immediately and persisted to Postgres.
    /// CompressedData is stored in ProverManager.compressed_proofs for the
    /// batch collector to consume when the window fills.
    Compressed { attestation_hash: [u8; 32] },
    /// Terminal failure. reason contains the human-readable error.
    Failed { reason: String },
}

impl JobState {
    /// Returns true if the job has reached a terminal state (no further
    /// transitions will occur).
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobState::Compressed { .. } | JobState::Failed { .. })
    }
}

/// Stored after phase 1 (compressed) completes.
///
/// Contains the live SP1ProofWithPublicValues (Compressed variant) and vk
/// needed to build AggregationInput without re-proving. The aggregator crate
/// consumes these entries when batch_size or window_secs is reached.
pub struct CompressedData {
    /// The compressed SP1 proof — must be SP1Proof::Compressed.
    /// Passed directly to the aggregator guest via write_proof().
    pub proof: SP1ProofWithPublicValues,
    /// Verifying key for the inference ELF.
    /// Passed to the aggregator's write_proof() alongside the proof.
    pub vk: SP1VerifyingKey,
    /// keccak256(model_id || input_hash || output_hash)
    /// Available immediately — surfaced to the agent for bet placement.
    pub attestation_hash: [u8; 32],
    /// Raw public values bytes — [model_id(32) | input_hash(32) | output_hash(32) | ...]
    /// Preserved for aggregator public values construction.
    pub public_values: Vec<u8>,
}

impl Clone for CompressedData {
    fn clone(&self) -> Self {
        Self {
            proof: self.proof.clone(),
            vk: self.vk.clone(),
            attestation_hash: self.attestation_hash,
            public_values: self.public_values.clone(),
        }
    }
}

/// Aggregated batch proof result — produced by the aggregator crate, not per-job.
///
/// This is no longer a per-job artifact. The aggregator runs one Groth16 over
/// N compressed proofs and writes a single ProofData for the batch. The settler
/// consumes this to call submitAggregatedProof() on-chain once.
#[derive(Clone, Serialize, Deserialize)]
pub struct ProofData {
    /// Raw Groth16 proof bytes for the aggregated batch.
    /// Passed to InferenceVerifier.submitAggregatedProof().
    pub proof_bytes: Vec<u8>,
    /// Batch-level attestation: keccak256 of all job attestation_hashes in order.
    pub attestation_hash: [u8; 32],
    /// sha256(weights_bytes) — shared across all jobs in the batch (same model).
    pub model_id: [u8; 32],
    /// sha256(input_le_bytes) — of the representative/first job in the batch.
    pub input_hash: [u8; 32],
    /// sha256(output_le_bytes) — of the representative/first job in the batch.
    pub output_hash: [u8; 32],
    /// Aggregator verifying key — used to verify the batch proof on-chain.
    /// Skipped during serde; populated at runtime by the aggregator.
    #[serde(skip)]
    pub vk: Option<SP1VerifyingKey>,
}

impl std::fmt::Debug for ProofData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProofData")
            .field("attestation_hash", &hex::encode(self.attestation_hash))
            .field("model_id", &hex::encode(self.model_id))
            .field("input_hash", &hex::encode(self.input_hash))
            .field("output_hash", &hex::encode(self.output_hash))
            .field("proof_bytes_len", &self.proof_bytes.len())
            .field("vk", &"<SP1VerifyingKey>")
            .finish()
    }
}