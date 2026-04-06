// crates/prover_manager/src/job.rs

use serde::{Deserialize, Serialize};

/// Job lifecycle states.
///
/// Gateway's spawn_settler loop depends on these variants — the variants
/// Queued, Running, Done, and Failed must remain stable. Compressed is new
/// and is inserted between Running and Done for the two-phase proving path.
///
/// State machine:
///   Queued → Running → Compressed → Done
///                    ↘ Failed (at any transition)
#[derive(Debug, Clone)]
pub enum JobState {
    /// Job is queued, not yet picked up by a worker.
    Queued,
    /// Worker acquired the semaphore slot and is actively proving.
    Running,
    /// Phase 1 complete: compressed STARK proof ready.
    /// attestation_hash is available. Groth16 wrapping is in progress.
    Compressed {
        attestation_hash: [u8; 32],
    },
    /// Phase 2 complete: Groth16 proof written to disk.
    /// Proof is now submittable on-chain via submitProof().
    Done {
        proof_path: String,
    },
    /// Terminal failure. reason contains the human-readable error.
    Failed {
        reason: String,
    },
}

impl JobState {
    /// Returns true for terminal states — no further transitions possible.
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobState::Done { .. } | JobState::Failed { .. })
    }
}

/// Internal proof result stored in memory after successful Groth16 proving.
/// Written to the ProofMap in ProverManager and persisted to Postgres
/// by the gateway's spawn_settler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofData {
    /// Raw Groth16 proof bytes — passed to InferenceVerifier.submitProof()
    pub proof_bytes:      Vec<u8>,
    /// sha256(model_id || input_hash || output_hash)
    /// Committed on-chain as the attestation key.
    /// NOTE: switch to keccak256 before mainnet for EVM alignment.
    pub attestation_hash: [u8; 32],
    /// sha256(weights_bytes) — the committed model identity
    pub model_id:         [u8; 32],
    /// sha256(input_le_bytes) — committed by the guest program
    pub input_hash:       [u8; 32],
    /// sha256(output_le_bytes) — committed by the guest program
    pub output_hash:      [u8; 32],
}