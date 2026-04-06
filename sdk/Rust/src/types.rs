use serde::{Deserialize, Serialize};

// ── Job lifecycle ─────────────────────────────────────────────────────────────

/// All possible job statuses returned by the gateway.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Proving,
    Done,
    Failed,
    Settled,
}

impl JobStatus {
    /// Returns true for terminal states that require no further polling.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Failed | Self::Settled)
    }

    /// Returns true if the job completed successfully.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Settled)
    }
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Proving => "proving",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Settled => "settled",
        };
        write!(f, "{s}")
    }
}

// ── /v1/jobs ─────────────────────────────────────────────────────────────────

/// Body for POST /v1/jobs.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SubmitJobRequest {
    pub input_data: Vec<Vec<f64>>,
    pub model_id: String,
}

/// Response from POST /v1/jobs.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SubmitJobResponse {
    pub job_id: String,
    pub status: String,
}

/// Response from GET /v1/jobs/{id}.
///
/// Note: `attestation_hash` requires the gateway's `JobStatusResponse` to
/// include the field. Add `attestation_hash: Option<String>` to that struct
/// and populate it from the DB row. Until then this field will always be None.
#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    pub job_id: String,
    pub status: JobStatus,
    pub proof_path: Option<String>,
    pub tx_hash: Option<String>,
    /// Populated once the gateway exposes it (see impl guide §6c).
    pub attestation_hash: Option<String>,
    /// Present when status is `failed`.
    pub reason: Option<String>,
}

// ── /v1/jobs/{id}/proof ──────────────────────────────────────────────────────

/// Response from GET /v1/jobs/{id}/proof.
#[derive(Debug, Clone, Deserialize)]
pub struct Proof {
    pub job_id: String,
    pub proof_hex: String,
    pub size_bytes: usize,
}

// ── /v1/models ───────────────────────────────────────────────────────────────

/// Body for POST /v1/models.
#[derive(Debug, Clone, Serialize)]
pub struct RegisterModelRequest {
    pub name: String,
    pub version: String,
    /// Base64-encoded ONNX model file.
    pub artifact_b64: String,
    /// Expected input dimensions, e.g. `[1, 4]`.
    pub input_shape: Vec<u64>,
}

/// Response from POST /v1/models.
#[derive(Debug, Clone, Deserialize)]
pub struct RegisterModelResponse {
    pub model_id: String,
    pub on_chain_model_id: String,
    pub ipfs_cid: String,
    pub gateway_url: String,
    pub on_chain_hash: String,
}

// ── /healthz ─────────────────────────────────────────────────────────────────

/// Response from GET /healthz.
#[derive(Debug, Clone, Deserialize)]
pub struct Health {
    pub status: String,
    pub version: String,
    pub settle_enabled: bool,
    pub db: String,
}

impl Health {
    /// Returns true when the gateway reports a healthy state.
    pub fn is_healthy(&self) -> bool {
        self.status == "ok" && self.db == "connected"
    }
}

// ── High-level result ─────────────────────────────────────────────────────────

/// Returned by `VeilClient::verify_inference`.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    /// Gateway job identifier.
    pub job_id: String,
    /// Terminal status (`done` or `settled`).
    pub status: JobStatus,
    /// On-chain transaction hash. Present once the proof is settled.
    pub tx_hash: Option<String>,
    /// keccak256(modelId ‖ inputHash ‖ outputHash).
    /// Populated once the gateway exposes the field (see impl guide §6c).
    pub attestation_hash: Option<String>,
    /// Wall-clock milliseconds from job submission to terminal state.
    pub elapsed_ms: u64,
}
